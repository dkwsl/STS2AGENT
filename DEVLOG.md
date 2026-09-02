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

## P2 sts2-mcp MCP 客户端 + Mock server（已完成）

### 步骤 1：MCP 客户端（`client.rs`）
- `McpClient`：以 tokio 子进程拉起 MCP server（真实 Python 或 Rust Mock），通过 stdio JSON-RPC 2.0 通信。
- 实现 MCP 协议子集：`initialize` → `notifications/initialized` → `tools/list` → `tools/call`。
- `read_response` 逐行读取，跳过通知（无 id），按 id 匹配响应。
- 便捷方法：`get_game_state(format)` 返回状态 JSON 字符串；`call_tool(name, args)` 通用工具调用；`shutdown()` 终止子进程。
- `Drop` 自动 `start_kill` 防泄漏。

### 步骤 2：Mock 游戏状态机（`mock.rs`）
- `MockGame`：与真实 STS2MCP 契约同构的 Rust MCP server，脚本化战斗序列。
  - 流程：Map → choose_node(0) → Combat（Jaw Worm 12HP）→ 2× Strike 杀敌 → Rewards → proceed → Map。
  - 出牌结算（Strike 伤 6 / Defend 防 5）、能量管理、手牌索引左移、end_turn 敌方攻击 + 新回合。
  - `get_game_state(format="json")` 返回可被 `sts2-core::GameState` 反序列化的完整 JSON；`format="markdown"` 返回摘要。
- 暴露 11 个工具（get_game_state/combat_play_card/combat_end_turn/map_choose_node/rewards_claim/proceed_to_map/…），未实现的工具返回 isError。
- 单元测试 `full_combat_scenario` 覆盖完整流程。

### 步骤 3：Mock 二进制（`src/bin/sts2-mcp-mock.rs`）
- 同步 stdio 循环：逐行读 stdin JSON-RPC → `MockGame::handle_message` → 逐行写 stdout 响应。
- 通知（无 id）不回复。可独立运行：`echo '{"jsonrpc":...}' | ./target/debug/sts2-mcp-mock`。

### 步骤 4：集成测试（`tests/integration.rs`）
- `mock_full_flow`：拉起 Mock 二进制 → McpClient 握手 → get_game_state 反序列化为 GameState → 断言 Map → choose_node → 断言 Monster/enemy 12HP → play_card → 断言 enemy 6HP → play_card → 断言 Rewards → claim gold → proceed → 断言回到 Map。
- `CARGO_BIN_EXE_*` 在运行时不可用，改用 `option_env!`（编译期）+ `CARGO_MANIFEST_DIR` 路径回退。

### 步骤 5：验证
- `cargo fmt --check` ✅、`cargo clippy --all-targets -- -D warnings` ✅、`cargo test --workspace` ✅（sts2-core 7 + sts2-mcp 2 = 9 passed）。
- 手动验证：`echo initialize | ./target/debug/sts2-mcp-mock` 正确返回 JSON-RPC 响应。
- Mock 配置切换：`config.toml` 中 `[mcp] command = "./target/debug/sts2-mcp-mock"` 即可从真实 Python server 切到 Mock。

---

## 待你确认/配合的事项

- 暂无阻塞项。后续 P4（LLM 客户端）需要真实 API key，届时请通过 `config/.env`（`STS2_OPENAI_API_KEY=...`）或 `secret/` 提供——**不要**直接贴在对话里，也勿写入会被提交的文件。
- 下一步可进入 **P5：完整编排**（会话/历史、CancellationToken 打断、多轮循环、LLM 输出解析为 Action 并执行）。是否需要把 P0–P4 commit 推送到 GitHub？

---

## P4 LLM 客户端 + LLM 决策接线（已完成）

### 步骤 1：sts2-llm LLM 客户端
- `types.rs`：`ChatMessage`（system/user/assistant 构造器）、`Usage`（prompt/completion tokens + cost 换算）、`ChatResponse`（content + reasoning + usage）、`StreamEvent`（Delta/Reasoning/Usage/Done/Error）。
- `client.rs`：`LlmClient`（OpenAI 兼容 `/chat/completions`，reqwest + rustls）。
  - `chat()`：非流式，提取 content + reasoning_content/reasoning + usage。
  - `chat_stream()`：流式 SSE，后台 tokio task 逐行解析 `data: ` 行，推送 `Delta`/`Reasoning`/`Usage`/`Done`/`Error` 到 `UnboundedReceiver`；`stream_options.include_usage` 取最终用量。
  - `from_config(&ModelConfig)` 从配置构造。
- `budget.rs`：`BudgetGuard` 累计 token/成本，`is_over_budget()` 超 token 或成本限额即返回 true（R6）。
- 单测 4 个：cost 计算、预算累计、成本限额、无限额——全绿。

### 步骤 2：决策接线（sts2-agent `decide.rs` + sts2-tui CLI）
- `decide.rs`：`run_decide(config, use_mock)` 编排：
  1. 拉起 MCP server（Mock 或真实，由 `--mock` 决定）
  2. `get_game_state(format="json")` 取状态
  3. 构造 system prompt（STS2 动作规则 + 格式要求）+ user message（状态 JSON）
  4. `LlmClient::chat_stream()` 流式输出决策到 stdout，思考到 stderr
  5. `BudgetGuard` 记录用量并打印成本
- `sts2-tui` CLI 新增 `--decide`（执行一次决策）和 `--mock`（用 Mock MCP）。
- 无 API key 时优雅报错，不 panic。

### 步骤 3：验证
- `cargo fmt --check` ✅、`cargo clippy --all-targets -- -D warnings` ✅、`cargo test --workspace` ✅（13 passed：core 7 + llm 4 + mcp 2）。
- `cargo run -p sts2-tui -- --decide --mock` 正确识别无 key 并报错（不 crash）。
- Mock 二进制 `./target/debug/sts2-mcp-mock` 就位可被 spawn。

### 第一个可运行成果
配置 API key 后即可跑通完整链路：
```bash
# 1. 在 config/.env 填写 key（不入库）
echo 'STS2_OPENAI_API_KEY=sk-...' > config/.env
# 2. 运行（Mock 状态 → LLM 流式决策 → 终端输出）
cargo run -p sts2-tui -- --decide --mock
```
输出：LLM 给出 `ACTION: ... | ...` + `REASON: ...`，并打印 token 用量与成本。

---

## 待你确认/配合的事项

- **需要你配置 API key** 才能看到 LLM 决策实跑。请通过 `config/.env`（`STS2_OPENAI_API_KEY=sk-...`）或 `secret/` 提供——**不要**直接贴在对话里。config.toml 里可改 endpoint/model/price 切换供应商。
- 下一步可进入 **P5：完整编排**（会话/历史、CancellationToken 打断、多轮循环、LLM 输出解析为 Action 并执行）。是否需要把 P0–P4 commit 推送到 GitHub？

---

## 修乱码 + 闭合回路（已完成）

### 步骤 1：修乱码
- 在 `sts2-llm/client.rs` 加 `sanitize()` 函数：过滤控制字符和 NUL，保留换行/制表符；流式和非流式路径均应用。
- `decide.rs` 加 `show_thinking` 参数：默认不打印 reasoning（之前乱码来自 reasoning 流）；加 `--thinking` CLI flag 可选开启。
- 效果：实测 `--play --mock` 6 轮，全程无乱码。

### 步骤 2：闭合回路（自动对局）
- `parse.rs`：从 LLM 输出提取 `ACTION: tool | key=value | ...`，值自动推断 int/string，工具名归一化别名（end_turn→combat_end_turn 等）。6 个单测全绿。
- `play.rs`：自动对局循环——每轮取状态→LLM 流式决策→解析 ACTION→MCP 执行→记录结果到历史→取下一状态。保留最近 5 轮历史（含执行结果）供 LLM 上下文。
- `sts2-tui` CLI 新增 `--play`、`--max-turns N`、`--thinking`。
- 预算守卫：每轮检查 `is_over_budget()`，超额自动停。
- 状态摘要：每轮标题显示 `[地图]` / `[战斗 R1 | 72/80 HP, 3 能量 | 敌人: Jaw Worm]` / `[奖励]` 等。
- 历史反馈：执行成功/失败结果都记入历史，LLM 能从失败中调整策略。

### 步骤 3：验证
- `cargo fmt --check` ✅、`cargo clippy --all-targets -- -D warnings` ✅、`cargo test --workspace` ✅（19 passed：parse 6 + core 7 + llm 4 + mcp 2）。
- `cargo run -p sts2-tui -- --play --mock --max-turns 6` 实测：LLM 第 1 轮选 Shop 失败→第 2 轮改选 Monster→战斗→2× Strike 杀 Jaw Worm→领 25 金+药水。全程无乱码，成本 $0.0024。

### 如何检验成果
```bash
# 1. 单次决策（修乱码后干净输出）
cargo run -p sts2-tui -- --decide --mock

# 2. 自动对局（闭合回路，看 LLM 自动打通一局 Mock）
cargo run -p sts2-tui -- --play --mock --max-turns 6

# 3. 带思考过程
cargo run -p sts2-tui -- --play --mock --max-turns 6 --thinking

# 4. 全套测试
cargo test --workspace
```
预期：LLM 每轮给出 ACTION + REASON，自动执行，状态从地图→战斗→杀敌→领奖→回地图。token/成本逐轮累计。Ctrl+C 可打断。

---

## 待你确认/配合的事项

- config.toml 里 `price_in`/`price_out` 仍是 OpenAI 的默认价格，GLM-5 实际价格不同。你知道清华平台定价的话可以改，不改也不影响功能（只影响 cost 显示）。
- 下一步方向：commit 推送？做 TUI 界面（P6）？加会话历史保存/加载（R5）？
