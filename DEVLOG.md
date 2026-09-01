# 开发日志 DEVLOG

本文件按步骤记录每阶段实际做了什么。面向用户复核。

---

## P0 仓库脚手架（已完成）

### 步骤 1：工具链与目录确认
- 环境已具备：`cargo 1.89.0` / `rustc 1.89.0` / `rustfmt 1.8.0`，无需额外安装。
- 起始目录内容：`AGENTS.md`、`PLAN.md`、`.gitignore`、`secret/`（空）。全程未操作仓库外任何文件。

### 步骤 2：Cargo workspace 骨架
- 新增 `Cargo.toml`（workspace，`resolver=2`，6 个成员 crate），统一 `[workspace.package]`（edition 2021、rust-version 1.85、license MIT）与 `[workspace.dependencies]`（serde/tokio/anyhow/thiserror/tracing/reqwest/ratatui/crossterm/toml/dotenvy/clap 等，版本集中管理；内部 crate 以 path 互引）。
- 新增 6 个 crate：
  | crate | 作用 | 当前内容 |
  |---|---|---|
  | `sts2-core` | 领域模型+配置 | `state.rs`/`action.rs`/`config.rs` 类型骨架 |
  | `sts2-mcp` | MCP 客户端+Mock | 模块占位（P2 实现） |
  | `sts2-decision` | 可插拔决策引擎 | `DecisionEngine` trait + `RuleBasedEngine` 空实现（P3 填规则） |
  | `sts2-llm` | OpenAI 兼容客户端 | 模块占位（P4 实现） |
  | `sts2-agent` | 编排主控 | `load_config()` 已可用 |
  | `sts2-tui` | 终端界面 | 二进制入口，支持 `--check` 校验配置 |
- 关键设计落地：
  - `Action` 用 `#[serde(tag="action")]`，可直接序列化为 MCP/HTTP 动作请求体。
  - `GameState` 用 `#[serde(flatten)] extra` 保留未识别字段（宽进严出，P1 扩展具体子结构）。
  - `StateType` 含 `#[serde(other)] Unknown` 兜底未来新屏。

### 步骤 3：配置模板与格式基线
- 新增 `config/config.example.toml`（model/mcp/decision/budget/storage，含 STS2MCP 真实启动命令）、`config/.env.example`。
- 新增 `rustfmt.toml`（edition 2021，max_width 100）。
- 本地测试用 `config/config.toml`（从示例复制，已被 `.gitignore` 忽略，不入库）。

### 步骤 4：工具链验证（fmt / build / clippy / test）
- 首次 build 因 reqwest 默认走 OpenSSL 缺 `pkg-config`/`libssl-dev` 失败 → **改用 rustls-tls**（`default-features=false` + `rustls-tls`），纯 Rust TLS，零系统依赖，未触及仓库外。
- 结果：`cargo fmt --check` ✅、`cargo build --workspace` ✅、`cargo clippy --all-targets -- -D warnings` ✅ 无告警、`cargo test --workspace` ✅。
- 运行 `sts2-tui --check` 成功加载并打印配置：`config ok: model=gpt-4o-mini, mcp.command=uv`。

### 当前可复用命令
```bash
cargo fmt                                  # 格式化
cargo fmt --check                          # 格式校验
cargo build --workspace                    # 构建
cargo clippy --all-targets -- -D warnings # 严格 lint
cargo test --workspace                     # 测试
cargo run -p sts2-tui -- --check           # 校验本地配置
```

---

## P1 sts2-core 完整类型 + serde 往返单测（已完成）

### 步骤 1：拆分模块
- `sts2-core/src` 由单 `state.rs` 拆为：`state.rs`（顶层 GameState/StateType/RunInfo/Player）、`combat.rs`（Battle/Enemy/Intent/Card/PileCard/Power/Keyword/Orb/Relic/Potion/Pet/Turn）、`screens.rs`（各 state_type 负载：HandSelect/Rewards/CardReward/MapState/Event/RestSite/Shop/FakeMerchant/Treasure/CardSelect/BundleSelect/RelicSelect/CrystalSphere/GameOver/Overlay 及子项）、`action.rs`/`config.rs`。
- `lib.rs` 以 `pub mod` + `pub use *` 统一导出。

### 步骤 2：按 raw-full.md 填全类型
- `Player` 补齐战斗字段（energy/max_energy/stars/hand/piles/orbs/orb_slots/pets/status/relics/potions/max_potion_slots），战斗字段全 `Option`+`#[serde(default)]`。
- `Card`/`PileCard`/`Power`/`Keyword`/`Enemy`/`Intent`/`Orb`/`Relic`/`Potion`/`Pet` 字段对齐上游命名；JSON `type` 关键字用 `#[serde(rename="type")] kind`。
- `Action` 枚举补全 28 个动作（play_card/end_turn/use_potion/.../menu_select），`#[serde(tag="action")]` 可直出请求体。
- `Turn` 用 `#[serde(other)] Unknown` 兜底未知值；`StateType` 同样 `#[serde(other)] Unknown`。
- 设计原则贯穿：每个聚合根用 `#[serde(flatten)] extra: serde_json::Value` 兜底未识别字段（宽进严出），供 LLM 上下文与调试。

### 步骤 3：serde 往返单测
- 新增 `crates/sts2-core/tests/serde_roundtrip.rs`（7 测试，全绿）：
  - `parse_combat_state`：战斗样本断言 entity_id/intent/energy/hand/relic.counter=null/potion.slot/未识别字段进 extra。
  - `roundtrip_combat_state`：序列化→反序列化关键字段不丢、extra 保留。
  - `parse_menu_state`：菜单态、options 异构数组、run/player 缺失。
  - `unknown_state_type_falls_back`：未知 state_type → Unknown。
  - `state_type_serde_variants`：monster/boss/weird 映射。
  - `action_serializes_to_request_body`：PlayCard→`{"action":"play_card",...}`；UsePotion 无 target 时省略字段；EndTurn→`{"action":"end_turn"}`。
  - `action_roundtrip`：5 个 Action 往返相等。
- 为集成测试在 `sts2-core/Cargo.toml` 加 `[dev-dependencies] serde_json`。

### 步骤 4：验证
- `cargo fmt --check` ✅、`cargo build -p sts2-core` ✅、`cargo clippy --all-targets -- -D warnings` ✅、`cargo test --workspace` ✅（sts2-core 7 passed，其余 0 失败）。
- 全程未操作仓库外文件；未引入新依赖（仅复用 workspace 内 serde/serde_json/thiserror）。

---

## 待你确认/配合的事项

- 暂无阻塞项。后续 P4（LLM 客户端）需要真实 API key，届时请通过 `config/.env`（`STS2_OPENAI_API_KEY=...`）或 `secret/` 提供——**不要**直接贴在对话里，也勿写入会被提交的文件。
- 下一步可进入 **P2：sts2-mcp MCP 客户端 + Mock server**（stdio 拉起 Python MCP server，实现 initialize/tools-list/tools-call 子集，按 §9 取状态/发动作；Mock 同构）。是否继续？是否需要我把 P0+P1 一并 commit 推送到 GitHub？
