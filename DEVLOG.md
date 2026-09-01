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

## 待你确认/配合的事项

- 暂无阻塞项。后续 P4（LLM 客户端）需要真实 API key，届时请通过 `config/.env`（`STS2_OPENAI_API_KEY=...`）或 `secret/` 提供——**不要**直接贴在对话里，也勿写入会被提交的文件。
- 下一步可进入 **P1：sts2-core 完整类型**（按 `STS2MCP/docs/raw-full.md` 填全 Player/Card/Enemy/Intent/Power/Orb/Relic/Potion/Battle 及各 state_type 负载 + serde 往返单测）。是否继续？
