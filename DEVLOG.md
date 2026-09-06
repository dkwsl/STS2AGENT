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

---

## TUI 交互式终端界面（已完成）

### 步骤 1：事件系统与状态管理（`app.rs`）
- `AppState`：游戏状态、决策文本、思考文本、历史记录、token/成本累计、进度、播放模式、错误等。
- `HistoryEntry`：每轮的回合号、状态摘要、ACTION、执行结果、成功/失败。
- `state_lines(gs)`：从 `GameState` 提取显示行（职业/HP/能量/手牌/敌人意图/地图选项/奖励）。
- `status_color()`：根据状态返回颜色（播放=绿/错误=红/结束=黄/待命=青）。

### 步骤 2：ratatui 布局（`ui.rs`）
- 三行垂直布局：顶栏（1行状态标题）| 主体（左 38% 状态面板 + 右 62% 决策面板）| 底栏（1行用量+按键提示）。
- 状态面板：`List` 渲染 `state_lines` 输出。
- 决策面板：进度提示 / 错误 / LLM 流式文本（`Paragraph` + `Wrap`）。
- 顶栏：模型名 + 模式 + 成本。底栏：轮数 + token + 成本 + 按键提示。

### 步骤 3：异步事件循环（`runner.rs`）
- 主循环：渲染 → 检查是否需触发决策 → `poll` 按键(50ms) → 非阻塞接收 LLM 流事件。
- 按键处理：`d`手动决策 / `p`自动播放 / `x`打断 / `t`思考切换 / `q`退出。
- LLM 流式：`chat_stream` 返回 `UnboundedReceiver`，主循环 `try_recv` 非阻塞读取 Delta/Reasoning/Usage/Done/Error。
- 决策完成 → `parse_action` → `mcp.call_tool` 执行 → 记录历史 → 下一轮。
- 预算检查每轮进行，超额自动停。
- `CancellationToken`：`x` 键取消后 `stream_rx = None`，停止当前决策。
- 终端恢复：退出时 `disable_raw_mode` + `LeaveAlternateScreen`，stderr 打印总结。

### 步骤 4：CLI 入口（`main.rs`）
- `--tui`：进入 ratatui 界面（优先于 `--play`）。
- `--tui --play`：TUI + 自动对局模式（`auto_play=true`）。
- `--tui` 不带 `--play`：手动模式，按 `d` 触发决策。
- `--play`（无 `--tui`）：裸文本自动对局（原有功能不变）。
- `--decide`：裸文本单次决策（原有功能不变）。

### 步骤 5：验证
- `cargo fmt --check` ✅、`cargo clippy --all-targets -- -D warnings` ✅、`cargo test --workspace` ✅（19 passed）。
- `--play --mock --zh --max-turns 2` 裸文本模式正常。
- `--tui --mock --zh --play` 需要 TTY 交互式终端（管道模式下报 `os error 6` 是预期行为）。

### 如何检验 TUI 成果
```bash
# 在交互式终端中运行（不能管道/重定向）：
cargo run -p sts2-tui -- --tui --mock --zh
# 看到状态面板 + 决策面板，按 d 手动决策，按 p 自动对局

# 自动对局模式：
cargo run -p sts2-tui -- --tui --mock --zh --play --max-turns 6
# LLM 自动打通一局，按 x 打断，按 q 退出

# 裸文本模式（无需 TTY）：
cargo run -p sts2-tui -- --play --mock --zh --max-turns 6
cargo run -p sts2-tui -- --decide --mock --zh

# 全套测试：
cargo test --workspace
```

---

## 待你确认/配合的事项

- TUI 需要在真正的交互式终端中运行（SSH/本地终端都行），管道或重定向会导致 crossterm 无法进入 raw mode。
- 下一步方向：commit 推送？加会话历史保存/加载（R5）？真实 Mod 联调？

---

## TUI 重写：自然语言对话模式（已完成）

### 设计变更
- 从"按键驱动自动执行"改为"自然语言对话 + 人工确认执行"。
- Agent 给出建议后**不自动执行**，显示在对话面板，等用户输入"执行"/"继续"等确认词后才调 MCP。
- 用户可随时打字与 Agent 沟通策略（作为 LLM 对话历史），或输入"打断"取消当前 LLM 流。

### 布局（三段式）
- **顶栏**：轮数 + token + 成本 + 状态
- **主体左**：状态面板（HP/能量/手牌/敌人/路径/奖励）
- **主体右**：对话面板（历史对话 + 流式输出 + 思考 + pending 确认提示）
- **底部**：输入框（打字 + 回车发送，光标可见）

### 交互流程
1. 启动 → 自动发起首轮决策（LLM 流式，用户可打字打断）
2. LLM 完成 → ACTION 显示在对话面板 → 等待用户确认
3. 用户输入"执行"/"继续"/"好" → 确认 → 后台 MCP 执行 → 取下一状态 → 循环
4. 用户输入"不"/"换一个" → 拒绝，重新决策
5. 用户输入其他文字 → 作为对话发给 LLM（带上下文）
6. LLM 流式中用户打字 → 打断当前决策

### 意图解析（`parse_intent`）
- 确认：执行/继续/好/确认/可以/ok/go/yes/y
- 拒绝：不/换一个/不要/拒绝/no/n/reject
- 打断：打断/停/stop/interrupt
- 其他：对话

### 架构
- 后台 task（`tokio::spawn`）：取状态 / LLM 流式 / MCP 执行，结果经 `mpsc::UnboundedChannel` 推回主循环。
- 主循环 `tokio::select!`：33ms 超时 + 按键 poll + 后台消息，**绝不直接 .await 耗时操作**。
- `McpClient` 包在 `Arc<Mutex>` 中供后台 task 共享。
- `CancellationToken` 在 `consume_stream` 中生效。
- 状态机 `Mode`：Idle/FetchingState/Streaming/Executing/PendingConfirm。

### 如何检验
```bash
# 交互式终端中运行（需 TTY）：
cargo run -p sts2-tui -- --tui --mock --zh --max-turns 6
# 看到对话面板 + 输入框，Agent 给建议后输入"执行"确认

# 裸文本模式（无需 TTY）：
cargo run -p sts2-tui -- --play --mock --zh --max-turns 4

# 全套测试：
cargo test --workspace
```

---

## 待你确认/配合的事项

- TUI 对话模式需在交互式终端中运行验证（管道模式无法进入 raw mode）。
- 下一步方向：commit 推送？加会话历史保存/加载（R5）？真实 Mod 联调？

---

## 知识库检索 + menu_select 类型修复 + --play auto_mode 修复（已完成）

> 接替上一个 agent（对话超限）。上一个 agent 留下未提交改动：menu_select 类型修复 + 知识库功能。本次验证、补全并提交。

### 步骤 1：验证并补全 menu_select 类型修复
- **问题**：真实 MCP server（Python pydantic）要求 `menu_select` 的 `option` 参数为字符串，但 LLM 输出整数 0 导致 `string_type` 校验报错。
- **修复**（上一个 agent 已做）：`parse.rs` 的 `normalize_args` 把 `option`/`tool` 参数强制转字符串。
- **补全**：新增单测 `menu_select_option_coerced_to_string`，覆盖 int→string、原生字符串、`crystal_sphere_set_tool` 的 tool 参数。
- 验证三处 `call_tool`（runner.rs:526/557/626、play.rs:129）的 args 均经 `parse_action`→`normalize_args`，修复路径完整。

### 步骤 2：修复 --play 模式 auto_mode bug（本次新发现）
- **现象**：`--play --mock` 端到端验证时，LLM 给出完美分析但**不输出 ACTION 行**，2 轮均"解析失败跳过"，对局无法推进。
- **根因**：`play.rs:73` 调 `build_messages` 时 `auto_mode=false`，触发 system prompt 的"不要输出 ACTION 行，只给文字建议"分支——这是对话模式的正确行为，但 `--play` 是自动对局模式，应 `auto_mode=true`。
- **修复**：`play.rs` 的 `auto_mode` 改为 `true`，并同步注入知识库检索（与 runner 一致）。
- **验证**：重跑 `--play --mock --max-turns 3`：第 1 轮 LLM 选休息点(node 1)→Mock 报错只支持 node 0→第 2 轮 LLM 从失败中学习改选 node 0→成功进入战斗（历史反馈机制生效）。第 3 轮因 LLM 服务端 503 跳过（非代码问题）。

### 步骤 3：知识库检索功能（上一个 agent 已做，本次验证）
- `knowledge.rs`：从 `data/knowledge/raw/*.md` 按关键词检索（含中英同义词扩展），返回 top 5 段落（≤1500 字）。`extract_keywords` 从 GameState 提取角色/敌人/手牌/遗物/药水关键词。
- `scripts/knowledge/fetch.py`：爬取 slaythespire-2.com 攻略站 18 篇攻略到 `data/knowledge/raw/`（已运行，data/ 已 gitignore）。
- `decide.rs`：`build_messages` 新增 `knowledge`/`session_notes` 参数，注入知识库参考与往期经验。
- LLM 可输出 `NOTE:` 行记录经验，`parse.rs` 的 `extract_notes`/`is_note_line` 提取并追加到 `{session_id}_notes.md`，后续轮次注入 prompt。
- `config.rs`：新增 `knowledge_dir` 配置（默认 `data/knowledge/raw`）。

### 验证
- `cargo fmt --check` ✅、`cargo clippy --all-targets -- -D warnings` ✅、`cargo test --workspace` ✅（32 passed）。
- `--play --mock --zh --max-turns 3` 端到端：对局从地图推进到战斗，知识库注入生效（LLM 引用 Burning Blood 回血机制），会话存盘正常。

### 注意事项
- ~~`config/config.toml` 含明文 API key~~ → 已迁移到 `config/.env`（见下节）。
- T1–T10 TUI 待修问题（PLAN.md §14）已核查，结论见下节。

---

## T1–T10 核查 + dotenvy 路径修复 + API key 迁移（已完成）

### 步骤 1：逐项核查 T1–T10（对照 "Major overhaul" commit 6bdb711）
逐一审查 runner.rs / app.rs / ui.rs / decide.rs 代码，结论（详见 PLAN.md §14 核查表）：
- **已修（9 项）**：T1（Esc+文字"退出"，IntentReady 拦截 Quit 返回 quit）/ T2（system prompt 重写为对话伙伴）/ T4（streaming_text 分离，流式不重置 scroll）/ T5（限帧 66ms+批量排空）/ T6（退出存 session）/ T7（abort_current_llm+排空 bt_rx）/ T8（Enter 只 push chat，intent 只 push history）/ T9（wrap_line 手动换行）/ T10（pending_actions 多步队列）。
- **待定（1 项）**：T3 滚动方向。当前 `↑=看上方历史`（runner.rs:258），实为 TUI 标准约定（vim/less/man 一致），PLAN 原"对调"要求疑基于误判，建议保持现状待用户实测确认。

### 步骤 2：修复 dotenvy 路径 bug（API key 无法从 config/.env 加载）
- **问题**：`load_config` 用 `dotenvy::dotenv()`（从 cwd 找 `./.env`），但项目 .env 在 `config/.env`，导致清空 config.toml 的 api_key 后 key 无法加载（`--decide` 报"未配置 API key"）。
- **修复**（lib.rs:18-21）：优先 `dotenvy::from_path("config/.env")`，失败回退 `dotenvy::dotenv()`。
- **验证**：修复后 `--decide --mock` 不再报"未配置"，正常进入 MCP 连接阶段。

### 步骤 3：迁移 config.toml 明文 key 到 config/.env（AGENTS.md §4 合规）
- `config/config.toml` 的 `api_key` 字段原含明文 key（已 gitignore 未入库，但违反 §4"密钥仅放 secret/ 或环境变量"）。
- 迁移：`config.toml` 的 `api_key = ""`（留空），key 仅存 `config/.env` 的 `STS2_OPENAI_API_KEY`（已确认 .env 与原 config.toml 的 key 一致）。
- `load_config` 在 api_key 畺空时从环境变量读取，行为不变。

### 验证
- `cargo fmt --check` ✅、`cargo clippy --all-targets -- -D warnings` ✅、`cargo test --workspace` ✅。
- `--decide --mock` api_key 从 .env 正确加载。

---

## 真实 MCP 连接修复（已完成）

### 现象
真实 MCP 模式（非 `--mock`）下 agent 读不到游戏状态：`get_game_state` 返回 7 字节 `"Error:"`，状态面板空，Agent 说看不到状态。

### 根因
上一个 agent 在调试 WSL→Windows 网络时配置了 `netsh interface portproxy` 规则（`0.0.0.0:15526 → 127.0.0.1:15526`），该规则持久化在 Windows 注册表，**重启后仍然存在**。这条规则导致：

1. portproxy 在 `0.0.0.0:15526` 监听（由 `svchost.exe` 占用），拦截所有到 15526 的连接。
2. portproxy 转发到 `127.0.0.1:15526`，但游戏 Mod 的 `Initialize()` 未执行（游戏日志无 `[STS2 MCP]`），`127.0.0.1:15526` 无人监听。
3. 连接挂起 → server.py 的 httpx 10 秒超时 → `ReadTimeout`（字符串表示为空）→ `_handle_error` 返回 `"Error:"`。
4. agent 把 `"Error:"` 当作状态 JSON 反序列化失败 → 空状态。

> 注：当前架构下 `win_server.py`（WSL 文件）由 `powershell.exe` 调用 **Windows 端 python** 执行，server.py 在 Windows 端用 `localhost:15526` 直连游戏 Mod，**不需要 portproxy**。portproxy 是上一个 agent 早期尝试 WSL 直连 Mod 时加的，后来改用了 powershell.exe 方案但没清理残留规则。

### 修复
1. **删除残留 portproxy 规则**（Windows PowerShell 管理员）：
   ```powershell
   netsh interface portproxy delete v4tov4 listenport=15526 listenaddress=0.0.0.0
   ```
2. **完全退出并重启游戏**，让 Mod 的 `Initialize()` 重新执行、HttpListener 绑定 `localhost:15526`。
3. 确认游戏日志出现 `[STS2 MCP] v... server started on http://localhost:15526/`，即 Mod 正常启动。

### agent 端改动
- `client.rs` spawn：MCP server 的 stderr 从 `Stdio::null()` 改为重定向到 `data/logs/mcp.log`，便于排查（之前 Python server 的错误/日志完全不可见）。
- `decide.rs`：`--decide` 模式在调 LLM 前打印 `get_game_state` 返回的字节数和前 800 字符（诊断用，定位完成后保留）。
- 曾加过 get_game_state 重试 3 次的逻辑，后回退（该场景重试只会让每次失败等 30 秒，反而碍事）。

### 验证
- 删除 portproxy + 重启游戏后，`cargo run -p sts2-tui -- --decide` 能读到真实游戏状态 JSON。
- `data/logs/mcp.log` 可见 server.py 的请求日志。

### 教训
- `netsh portproxy` 规则持久化在注册表，重启不丢；调试结束后必须显式 `delete` 清理。
- WSL 调 `powershell.exe` 跑 Windows python 时，python 进程在 Windows 端，其 `localhost` 指向 Windows，不需要额外端口转发。

---

## 自主模式判定严格化（已完成）

### 背景
原自主模式（auto_mode）判定过宽：`UserIntent::Confirm`（"执行"/"继续"）和 `UserIntent::Chat`（任意对话如"分析一下"）都会设 `auto_mode=true`+`execute_actions=true`，导致 agent 在用户只说了一句话后持续自动操作游戏，不符合用户"只有明确说'自己打'才操作"的要求。

### 改动
1. **意图分类**（`llm_parse_intent`/`fallback_parse_intent`）：AUTOPLAY 触发词从"你来""交给你""自动打"等宽泛词收紧为**只认"自己打"**（含"自己打这层""自己打这局""自己打这场"）。
2. **handle_user_intent**：
   - `Confirm`（"执行"/"继续"）：只执行本轮一次（`execute_actions=true` 但不设 `auto_mode`），执行完自动关闭。
   - `Chat`（纯对话）：不给执行权限（`execute_actions=false`），agent 只做分析/回答。
   - `AutoPlay`（"自己打"）：保持 `auto_mode=true`+`execute_actions=true`，持续操作直到完成或用户打断。
3. **StreamDone**：执行完一轮后，非 auto_mode 时 `execute_actions=false`；auto_mode 下 LLM 没给 ACTION 则退出自主模式。
4. **StateChange/StateReady**：只有 `auto_mode` 才触发自动重新分析（移除 `pending_actions` 作为独立触发条件——多步队列只在 auto_mode 期间才有）。
5. **system prompt**（decide.rs）：强化自主模式触发说明——只有"自己打"类指令触发自主模式；非自主模式每次只执行一次操作。

### 验证
- `cargo fmt --check` ✅、`cargo clippy --all-targets -- -D warnings` ✅、`cargo test --workspace` ✅。
