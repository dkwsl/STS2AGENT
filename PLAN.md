# STS2Agent 项目完成计划

> 面向《杀戮尖塔 2》的专用决策 Agent：游戏中实时回合决策建议 + 解释，对局后完整决策链复盘。
> 本计划遵守 `AGENTS.md` 全部硬约束（R1–R6、Rust 核心、MCP 通信、可更改决策工具、LLM 解释、本地存储、敏感信息禁入 git）。

---

## 1. 决策结论（已与用户确认）

| 议题 | 结论 |
|---|---|
| 游戏 Mod | 采用社区项目 `STS2MCP`（本地 clone 于 `/home/aaa12321/sts2mcp/STS2MCP/`，非本仓库内容、勿提交）：C# Mod 起游戏内 HTTP API（`localhost:15526`，无鉴权），随附 Python MCP server（`mcp/server.py`，stdio）作桥。Rust Agent 作 MCP 客户端拉起该 Python server，经 MCP 工具读写状态。契约详见 §9。 |
| 用户界面 | 先做 TUI（ratatui）；核心与表现分离，后续可加 Web / 桌面 / 嵌入游戏内 Mod UI。 |
| 决策工具 | 推荐：定义 `DecisionEngine` trait + 内置规则启发式实现，可插拔替换；MCTS 等高级算法留作扩展。详见 §8。 |
| 演示数据 | Rust 写的 Mock MCP server（与真实契约同构的脚本化战斗）+ 真实 MCP 客户端代码，无需游戏即可端到端演示。 |

---

## 2. 需求 → 方案映射（R1–R6）

| 需求 | 方案 | 落点 |
|---|---|---|
| **R1** Rust 核心 | 全部编排/决策/数据处理/LLM 调用均在 Rust；主控流程在 `sts2-agent`。允许调用外部库但不作为主控。 | `sts2-agent` 等 |
| **R2** 用户界面 | TUI（ratatui + crossterm）为首选前端；`Presenter` trait 抽象，便于后续 Web/桌面复用同一核心。 | `sts2-tui` |
| **R3** 可自定义模型配置 | `config.toml` + `.env` + TUI 设置页；字段含 endpoint、api_key、model、context_length、thinking_mode、price(in/out per 1M tokens)。`.gitignore` 屏蔽敏感文件；日志脱敏。 | `sts2-core`/`sts2-agent`/`sts2-tui` |
| **R4** 实时进度 + 打断 | 异步事件总线（tokio channel）发布阶段进度与 LLM token 流；`CancellationToken` 支持任意时刻打断；TUI 进度条 + 按键取消。 | `sts2-agent`/`sts2-tui` |
| **R5** 上下文历史 | 每会话存为 JSON（状态快照 + 决策 + LLM 轨迹 + 用量）；可列表/加载/导出；TUI 轨迹视图展示 Agent 内部工作流（非黑盒）。 | `sts2-agent`/`sts2-storage` |
| **R6** Token 用量与价格 | 每次 LLM 调用从 API 响应取 input/output token，按价格表换算成本；UI 实时累计；可设 token/成本预算，超额自动中断。 | `sts2-llm`/`sts2-agent`/`sts2-tui` |

---

## 3. 系统架构

```
┌─────────────┐   MCP(JSON-RPC)    ┌──────────────────────────────────────────┐
│ 游戏 Mod    │ ◀────────────────▶ │  Rust Agent (核心, 全 Rust 编排)          │
│ (已存在,    │                    │                                          │
│  暴露状态)  │                    │  ┌──────────┐  ┌────────────┐  ┌────────┐│
└─────────────┘                    │  │ MCP 客户 │─▶│ 决策引擎   │─▶│ LLM 客 ││
      ▲                            │  │ 端(+Mock)│  │ (可插拔)   │  │ 户端   ││
      │演示用 Mock                 │  └──────────┘  └────────────┘  └───┬────┘│
      ▼                            │                                  │     │
┌─────────────┐                    │  ┌────────────────────────────────▼───┐ │
│ Mock MCP    │                    │  │ 编排层: 会话/历史/预算/取消/进度事件 │ │
│ server(Rust)│                    │  └─────────────────────────────────────┘ │
└─────────────┘                    │  ┌──────────┐  ┌──────────────────────┐ │
                                   │  │ 存储层   │  │ TUI(ratatui) 首选前端 │ │
                                   │  │ JSON/配置│  │  + 扩展口子(Web/桌面) │ │
                                   │  └──────────┘  └──────────────────────┘ │
                                   └──────────────────────────────────────────┘
```

**分层原则**：核心编排与表现层解耦——`sts2-agent` 对外暴露“事件流 + 命令”接口，任何前端（TUI/Web/嵌入式）只消费事件、下发命令，复用同一核心。

---

## 4. Cargo Workspace 结构

```
sts2agent/
├─ Cargo.toml                  # workspace
├─ crates/
│  ├─ sts2-core/               # 领域类型(GameState/Action/...) + 配置类型 + 错误
│  ├─ sts2-mcp/                # MCP 客户端 + Mock MCP server(演示)
│  ├─ sts2-decision/           # DecisionEngine trait + 规则启发式实现 + 测试
│  ├─ sts2-llm/                # OpenAI 兼容 chat 客户端: 流式/用量/价格/预算/思考模式
│  ├─ sts2-agent/              # 编排主控: 会话/历史/取消/进度事件/存储(JSON)
│  └─ sts2-tui/                # ratatui 二进制: 视图/进度/打断/历史/设置/用量面板
├─ config/
│  ├─ config.example.toml      # 配置模板(无密钥)
│  └─ .env.example             # 环境变量模板(无密钥)
├─ data/                       # 运行时数据(已 gitignore): sessions/, logs/
├─ PLAN.md  AGENTS.md  .gitignore
└─ secret/                     # 本地密钥(已 gitignore, 永不入库)
```

> 可在实现阶段按需合并以减负，但 trait 边界保持不变。

---

## 5. 关键模块设计

### 5.1 sts2-core：领域模型与配置
- `GameState`（顶层，对齐 `STS2MCP/docs/raw-full.md` 的真实 JSON schema）：`state_type`（`menu`/`monster`|`elite`|`boss`/`hand_select`/`rewards`/`card_reward`/`map`/`event`/`rest_site`/`shop`/`fake_merchant`/`treasure`/`card_select`/`bundle_select`/`relic_select`/`crystal_sphere`/`game_over`/`overlay`/`unknown`）、`run{act,floor,ascension}`（`menu` 时缺失）、`player`，以及随 `state_type` 变化的负载块（`battle`/`map`/`event`/`rewards`/`shop`/`treasure`/…）。
- 子结构（Rust serde 模型，字段对齐上游命名）：
  - `Player{character,hp,max_hp,block,gold, energy?,max_energy?,stars?, hand?,draw_pile_count?,discard_pile_count?,exhaust_pile_count?, draw_pile?,discard_pile?,exhaust_pile?, orbs?,orb_slots?,orb_empty_slots?,pets?, status,relics,potions,max_potion_slots}`（战斗字段仅在战斗出现）。
  - `Card{index,id,name,type,cost,star_cost,description,target_type,can_play,unplayable_reason,is_upgraded,keywords}`；`PileCard{name,cost,star_cost,description}`；`Power{id,name,amount,type,description,keywords}`；`Keyword{name,description}`；`Enemy{entity_id,combat_id,name,hp,max_hp,block,status,intents}`；`Intent{type,label,title,description}`；`Orb{id,name,description,passive_val,evoke_val,keywords}`；`Relic{id,name,description,counter,keywords}`；`Potion{id,name,description,slot,can_use_in_combat,target_type,keywords}`。
  - `Battle{round,turn("player"|"enemy"),is_play_phase,enemies}`；各 `state_type` 负载（`Map`/`Event`/`Rewards`/`Shop`/`Treasure`/`RestSite`/`CardSelect`/`BundleSelect`/`RelicSelect`/`CrystalSphere`/`HandSelect`/…）。
- 反序列化策略：宽进严出——`#[serde(default)]` + `Option` 兼容字段缺失；保留原始 JSON 串供 LLM 上下文与调试。
- `Action` / `ActionRecommendation`：入参对齐 MCP 工具，如 `PlayCard{card_index,target?}`、`EndTurn`、`UsePotion{slot,target?}`、`DiscardPotion{slot}`、`ChooseMapNode{index}`、`ChooseEventOption{index}`、`ShopPurchase{index}`、`RestChooseOption{index}` 等，附优先级与理由 hint。
- `Config`：`ModelConfig{ endpoint, api_key, model, context_length, thinking_mode, price_in, price_out }`、`McpConfig`（见 §5.2 启动命令）、`DecisionConfig{ engine, params }`、`BudgetConfig{ token_limit, cost_limit_usd }`。
- 从 `config.toml` + `.env` 加载，`secret/` 与环境变量提供密钥；日志对密钥脱敏。

### 5.2 sts2-mcp：MCP 客户端 + Mock
- **真实链路**：Rust Agent 作 MCP 客户端，以 stdio 拉起 `STS2MCP` 的 Python MCP server（`FastMCP("sts2")`，stdio 传输），后者再转发游戏 Mod 的 HTTP API（`localhost:15526`）。配置示例：
  ```toml
  [mcp]
  command = "uv"
  args = ["run","--directory","/home/aaa12321/sts2mcp/STS2MCP/mcp","python","server.py"]
  # server.py 可选参数：--host/--port 指向游戏 HTTP、--no-trust-env 忽略代理
  ```
  客户端实现 MCP 必要子集（`initialize` / `tools/list` / `tools/call`，stdio JSON-RPC）。先评估现有 Rust MCP SDK；不合用则手写该子集（约 3 个方法，成本低）。核心只需：`get_game_state(format="json")` 取状态，再按 `state_type` 调对应动作工具（见 §9）。
- **Mock**：Rust 实现的同构 MCP server（同样暴露 `get_game_state` 等，返回脚本化战斗序列），使整条链路无游戏也能端到端演示；配置切换 `command` 即可真实/Mock 互换，零代码改动。
- **真实联调前置**：游戏需装 `STS2_MCP` Mod 并启用（开发/演示阶段用 Mock，非必需）。

### 5.3 sts2-decision：决策引擎（可插拔）
- trait `DecisionEngine { fn decide(&GameState) -> Vec<ActionRecommendation> }`。
- 内置 `RuleBasedEngine`：基于杀戮尖塔通用策略的启发式规则（攻击/防御权衡、能量利用、关键牌优先、斩杀线、遗物联动等），输出带理由的结构化建议。
- 种子知识（取自 `STS2MCP/AGENTS.md` 策略要点，纳入规则库）：HP 是资源而非分数；前压伤害；读意图决策（Sleep/Buff→全力输出，Attack→攻防平衡，Debuff→通常免伤输出）；出牌顺序 0 费→技能→大招最后；能斩杀则完全不防御；地图选路（健康>70% 打精英、Boss 前<80% HP 休息、100+ 金进店、卡组质量>数量）；Boss 战优先杀首领、积极用药；不囤药。
- 用 `dyn DecisionEngine` + 配置选择实现“可更改”；设计上预留“远程 MCP 决策工具”实现位（决策工具本身可作为外部 MCP server 由 Agent 调用）。
- 单测重点覆盖：规则分支、边界状态、可替换性。

### 5.4 sts2-llm：LLM 客户端
- OpenAI 兼容 `/v1/chat/completions`（支持 OpenAI / DeepSeek / 本地 vLLM 等，仅靠 endpoint+key 切换）。
- 流式响应（SSE）；`stream_options.include_usage` 取最终 usage。
- 思考模式：兼容 DeepSeek `reasoning_content` / OpenAI 推理模型，UI 可展示思考轨迹（呼应 R5）。
- 用量与价格：从响应 usage 取 `prompt_tokens`/`completion_tokens`，按配置价格换算成本（R6 明确不依赖本地分词器）。
- 预算守卫：调用前检查累计用量/成本预算，超额自动中断并提示。

### 5.5 sts2-agent：编排主控
- 主循环：`取状态(MCP) → 决策(DecisionEngine) → 拼 prompt → LLM(流式) → 产出解释`，全程 Rust 主控（R1）。
- 进度事件总线：向订阅者发布阶段进度与 token 流（供 R4 渲染）。
- `CancellationToken`：用户打断传播到 MCP/LLM 子任务并安全收尾（R4）。
- 会话/历史：每会话为 JSON（状态快照、决策、LLM 轨迹、用量、成本），支持列出/加载/导出；轨迹视图揭示内部工作流（R5）。
- 预算累计与自动中断（R6）；存储落盘到 `data/sessions/`（R5）。

### 5.6 sts2-tui：终端界面
- 视图：实时建议面板（决策 + 解释 + 思考轨迹）、回合状态摘要、进度条/阶段提示、**按键打断**、历史会话列表与轨迹回放、模型设置页、Token/成本累计面板与预算条。
- 经 `Presenter` trait 与核心解耦：核心只发事件、收命令；后续 Web/桌面/嵌入式前端复用同一核心。

---

## 6. 核心数据流

### 6.1 游戏中实时建议
1. 用户触发“获取建议”（或状态变化自动触发）。
2. Agent 经 MCP 客户端取当前 `GameState`（带超时）。
3. `DecisionEngine` 产出候选行动 + 理由 hint。
4. 拼装 prompt（状态 + 决策依据）→ LLM 流式生成自然语言解释。
5. 全程进度事件 → UI 实时渲染（“取状态中…/计算决策…/生成解释…/token 流”）；用户随时按键打断（R4）。
6. 每次调用记录 input/output token 与成本，累计展示；预算超额自动中断（R6）。
7. 本回合轨迹写入会话历史（R5）。

### 6.2 对局后复盘
1. 加载已保存会话 JSON（逐回合状态/决策/解释/用量）。
2. TUI 时间线回放每回合：状态快照 → 决策 → 解释；可选 LLM 生成整局总结。
3. 展示完整决策链与成本汇总（R5/R6）。

---

## 7. 关键技术选型

| 领域 | 选型 | 备注 |
|---|---|---|
| 异步运行时 | tokio（CancellationToken、mpsc/broadcast channel） | 进度事件总线、打断 |
| 序列化 | serde / serde_json | 状态、会话、配置 |
| TUI | ratatui + crossterm | 进度条、按键打断 |
| HTTP/LLM | reqwest（薄客户端） | 自控流式/usage/思考模式/provider 差异；优先于现成 SDK 以兼容多供应商 |
| 配置 | toml + dotenvy | `config.toml` + `.env` |
| MCP | 优先评估现有 Rust MCP SDK；不合用则手写 stdio JSON-RPC 子集（initialize/tools-list/tools-call） | 已知只需 3 方法，成本可控；契约见 §9 |
| 错误/日志 | anyhow + thiserror + tracing | 日志仅落 `data/logs/`，密钥脱敏 |
| CLI 参数 | clap | 即便 TUI 也接受启动参数 |

---

## 8. 决策工具方案分析（用户“不确定”→ 推荐结论）

**需求拆解**：决策工具把游戏状态算成行动建议，LLM 只做自然语言解释；“可更改”指用户能替换工具。

| 方案 | 优点 | 缺点 | 结论 |
|---|---|---|---|
| 规则启发式 + 可插拔 trait | 可控、可解释、可演示、工作量适中、理由可直接喂 LLM | 上限不及搜索算法 | **推荐：本次实现** |
| MCTS/搜索式 | 理论更优 | 实现复杂、耗时长、演示易超时、风险高 | 留作扩展（trait 已预留） |
| 外部现成工具 | 省力 | 目前无现成可用 | 暂不依赖 |

**推荐**：实现 `RuleBasedEngine` 作为参考实现，所有决策经 `dyn DecisionEngine` 派发，配置可选引擎；架构预留“远程 MCP 决策工具”实现位，未来可挂接 MCTS 或外部工具而不改核心。

---

## 9. MCP 接口契约（已从 `STS2MCP` 取得）

> 权威来源：`STS2MCP/mcp/server.py`、`STS2MCP/docs/raw-full.md`、`raw-simplified.md`、`mcp/README.md`。

### 9.1 拓扑
- 游戏 Mod（C#）在游戏内起 HTTP server：`http://localhost:15526`，无鉴权，仅本地。端点：`GET/POST /api/v1/singleplayer`、`GET/POST /api/v1/multiplayer`、`GET /api/v1/profile`、`GET /api/v1/compendium`、`GET /api/v1/wiki`、`GET/POST /api/v1/profiles`。SP/MP 互斥（错调返回 409）。
- Python MCP server（`mcp/server.py`，`FastMCP` + stdio）把上述 REST 包成 MCP 工具。Rust Agent = MCP 客户端，stdio 拉起它（启动命令见 §5.2）。也可后备直连 REST，但默认走 MCP 以符合「通过 MCP 协议与游戏交互」。

### 9.2 状态读取
- 工具 `get_game_state(format="markdown"|"json")`（默认 markdown；战斗用 `json` 取结构化，菜单/地图用 `markdown` 概览）。MP 版 `mp_get_game_state`。
- 响应顶层：`state_type` + `run{act,floor,ascension}` + `player`（`menu` 时无 `run`/`player`，只有 `menu_screen`/`options`/`blocked_options?`）。其余字段随 `state_type` 变（`battle`/`map`/`event`/`rewards`/…，schema 见 §5.1 与 `raw-full.md`）。
- 辅助只读工具：`get_profile()`、`get_compendium()`（含 `current_run.run_id`）、`search_wiki(query, item_type="all"|"card"|"relic", limit=10)`、`list_profiles()`。

### 9.3 动作工具（按 state_type 分组，SP；MP 同名加 `mp_` 前缀且 end_turn 为投票）
| state_type | 工具 / 入参 |
|---|---|
| `monster`/`elite`/`boss` | `combat_play_card(card_index, target?)`、`use_potion(slot, target?)`、`discard_potion(slot)`、`combat_end_turn()` |
| `hand_select` | `combat_select_card(card_index)`、`combat_confirm_selection()` |
| `rewards` | `rewards_claim(reward_index)`、`proceed_to_map()` |
| `card_reward` | `rewards_pick_card(card_index)`、`rewards_skip_card()` |
| `map` | `map_choose_node(node_index)`（MP 为 `mp_map_vote`） |
| `event` | `event_choose_option(option_index)`（含 Proceed）、`event_advance_dialogue()` |
| `rest_site` | `rest_choose_option(option_index)`、`proceed_to_map()` |
| `shop`/`fake_merchant` | `shop_purchase(item_index)`、`proceed_to_map()` |
| `treasure` | `treasure_claim_relic(relic_index)`、`proceed_to_map()` |
| `card_select` | `deck_select_card(card_index)`、`deck_confirm_selection()`、`deck_cancel_selection()` |
| `bundle_select` | `bundle_select(bundle_index)`、`bundle_confirm_selection()`、`bundle_cancel_selection()` |
| `relic_select` | `relic_select(relic_index)`、`relic_skip()` |
| `crystal_sphere` | `crystal_sphere_set_tool("big"|"small")`、`crystal_sphere_click_cell(x,y)`、`crystal_sphere_proceed()` |
| `menu`/`game_over` | `menu_select(option, seed?)`（`game_over` 仅 `main_menu`） |

> 全部动作经 `POST /api/v1/singleplayer`（body 含 `action` 字段）；响应 `{status:"ok"|"error", message|error}`。`target` 为敌人 `entity_id`（如 `JAW_WORM_0`）。

### 9.4 关键交互规则（决策引擎与编排须遵守）
- **手牌索引左移**：出牌后剩余牌索引变化；从右到左出牌或每次出牌后重取状态。
- **单体牌/药水**：须带 `target` = 敌人 `entity_id`（UPPER_SNAKE_CASE 带 `_0` 后缀）。
- **回合推进**：`end_turn` 后状态可能仍为 `is_play_phase:false` / `turn:enemy`，需重取 `get_game_state`（有时需取两次：一次看敌方回合结果，一次看新手牌）。
- **药水**：`slot` 是药水槽索引（非牌索引）；不耗能量、不计出牌；buff 药水在出牌前用。
- **奖励/选牌**：领取从右到左以免索引漂移；卡牌奖励会切到 `card_reward` 子屏。
- **事件**：选完常有 index 0 的 Proceed；Ancient 事件先 `advance_dialogue` 直到 `in_dialogue:false`。
- **proceed 不适用事件**：事件用 `choose_event_option` 选 Proceed。

### 9.5 Mock 对齐
- Rust Mock MCP server 按上述同构实现：暴露同名工具，返回脚本化 `state_type` 序列（一段完整战斗：`map`→`monster` 多回合→`rewards`→…），让 Agent 端到端跑通决策+解释+历史+用量，无需游戏。

---

## 10. 安全与配置
- `.gitignore`：`/secret`、`.env`、`/data`、`/target`、`/config/local.toml` 等用户覆盖件。
- 仅提交 `config.example.toml` / `.env.example`，不含任何密钥。
- 配置加载与日志输出对 `api_key` 等脱敏；tracing 输出仅入 `data/logs/`。
- 建议 CI 加敏感信息扫描（如 gitleaks 或对常见 key 模式的 ripgrep 检查）。

---

## 11. 测试策略
- **单元**：`sts2-decision` 规则分支/边界/可替换性；`sts2-core` 序列化往返；`sts2-llm` 用量/价格/预算计算。
- **集成**：Mock MCP server → Agent → Mock LLM（HTTP mock）→ 断言轨迹/预算/打断/历史落盘。
- **演示场景**：脚本化战斗走完整回合链，覆盖 R1–R6 全部可观测行为。

---

## 12. 实施阶段与里程碑

| 阶段 | 内容 | 交付物 | 覆盖 |
|---|---|---|---|
| P0 仓库脚手架 | workspace、`.gitignore`、配置模板、fmt/clippy/test 基线、AGENTS 遵从 | 可编译空壳 | — |
| P1 领域模型+配置 | `sts2-core`：`GameState`/`Action`/`Config` 类型、serde、加载保存 | 类型 + 单测 | R1/R3 |
| P2 MCP 客户端+Mock | `sts2-mcp`：真实客户端 + Mock server，端到端取状态 | 取状态 demo | R1 |
| P3 决策引擎 | `sts2-decision`：trait + `RuleBasedEngine` + 测试 | 建议产出 | R1 |
| P4 LLM 客户端 | `sts2-llm`：流式、usage、价格、预算、思考模式 | LLM 调用 demo | R3/R6 |
| P5 Agent 编排 | `sts2-agent`：主循环、会话/历史、取消、进度事件、存储 | 端到端核心 | R1/R4/R5/R6 |
| P6 TUI | `sts2-tui`：建议/进度/打断/历史/设置/用量面板 | 完整界面 | R2/R4/R5/R6 |
| P7 集成与演示 | 脚本化战斗贯通、文档、演示脚本、R1–R6 验收 | 可演示交付 | 全部 |
| P8 扩展(可选) | MCTS 引擎 / Web 前端 / 真实 Mod 联调 / 嵌入式 UI | 增强 | — |

---

## 13. 先决条件与风险

1. ~~**真实 Mod 接口契约**~~：已解决。从本地 clone 的 `STS2MCP` 取得完整契约（§9），`sts2-core` 数据模型与 Mock 据此对齐。
2. **MCP Rust SDK 可用性**：实现时评估现有 SDK；若不契合则手写 stdio JSON-RPC 子集（仅 initialize/tools-list/tools-call），成本可控（已计入计划）。
3. **决策质量**：规则启发式为基线，足够演示与解释；如需更强可平滑升级为搜索算法（trait 已留口）。
4. **LLM 供应商差异**：薄 reqwest 客户端兼容 OpenAI/DeepSeek/本地，思考模式按供应商分支处理。
5. **演示鲁棒性**：Mock + 真实接口双轨，避免演示依赖真实游戏环境。
6. **真实联调依赖游戏+Mod**：演示与开发阶段用 Mock；真实联调需本机运行游戏并装 `STS2_MCP` Mod（非阻塞当前开发）。
7. **中文输出不稳定**：`--zh` 时 LLM 思考过程偶尔仍用英文。待修复：可在 system prompt 中强化指令、或在 streaming 层对 reasoning_content 追加语言约束、或改用结构化 JSON 输出（`response_format`）强制字段语言。

---

## 14. TUI 对话模式待修复问题（逐项推进）

> 以下十个问题按优先级逐项修复，不做批量改动。

### 核查结论（2026-09-06，接替 agent 复核 "Major overhaul" commit 6bdb711 后状态）

| 编号 | 状态 | 说明 / 修复位置 |
|---|---|---|
| T1 | ✅ 已修 | Esc 退出 + 文字"退出"（`llm_parse_intent` 识别 QUIT，`IntentReady` 分支 runner.rs:403 拦截返回 quit=true；主循环 272 保存 session 后退出） |
| T2 | ✅ 已修 | system prompt 重写为"牌手顾问+对话伙伴"（decide.rs:104-167），明确区分"明确指令→ACTION"/"对话→纯文字"，给出正反例 |
| T3 | ⚠️ 待定 | 当前 `↑=chat_scroll+3`→看上方历史（runner.rs:258-264），实为 TUI 标准约定（vim/less/man 一致）。PLAN 原"对调"要求疑基于误判，建议保持现状，待用户实测确认 |
| T4 | ✅ 已修 | 流式 Delta 写入 `streaming_text`（独立渲染），不触发 `push_chat`；仅新消息 `push_chat` 时 `chat_scroll=0`（app.rs:143）。流式期间用户可自由浏览 |
| T5 | ✅ 已修 | 限帧 66ms（runner.rs:308）+ `while try_recv` 批量排空后台消息（282）+ `yield_now` 让出 CPU |
| T6 | ✅ 已修 | 退出路径 `session.finished=true; store.save(&session)`（runner.rs:272-278）；每轮 ExecDone 也存盘（600） |
| T7 | ✅ 已修 | Enter 时若非 Idle 调 `abort_current_llm`（cancel + 排空 bt_rx + 清空 streaming_text，runner.rs:95/231）；consume_stream 有 `cancel.cancelled()` 分支（333） |
| T8 | ✅ 已修 | Enter 处理器 push 一次 `state.chat`（237）；`handle_user_intent` 只 push `history`（LLM 上下文，非 UI），UI 不重复 |
| T9 | ✅ 已修 | `wrap_line` 按显示宽度手动换行（CJK=2列，ui.rs:105-150），不截断内容；scroll offset 按 wrap 后行数计算 |
| T10 | ✅ 已修 | `action_lines` 多行 → `pending_actions` 队列连续执行（runner.rs:511/608-634）；每步执行后重取状态；auto_mode 默认自动执行，用户可打断 |

### T1. 无法退出程序

- 现象：`--tui` 模式启动后无法退出（`q` 键已移除，输入框只接受文字输入）。
- 修复：在输入框中识别退出命令（如「退出」「exit」「quit」），或增加特殊指令前缀（如 `/quit`）；退出时恢复终端（`disable_raw_mode` + `LeaveAlternateScreen`）。
- 难点：当前 `runner.rs` 的主循环 `select!` 中没有退出分支；需在 `handle_user_intent` 中增加 `Quit` 意图。

### T2. LLM 角色定位不对——应是对话伙伴而非决策者

- 现象：当前 LLM 直接输出 `ACTION: ...` 并自行决策，不听用户指令。用户期望 LLM 是**沟通桥梁**：理解用户要求并翻译为程序动作、随时解释出牌原因、分析不这样出的理由等。
- 修复方向：
  - 重写 system prompt：LLM 角色 = 「牌手顾问 + 对话伙伴」，不是「自动决策者」。
  - 用户消息作为对话上下文喂给 LLM；LLM 回复中包含 ACTION 建议（供用户确认），但也要回答用户的策略问题。
  - 用户可问「为什么不打这张牌」「换个方案」等，LLM 用自然语言回答；回答末尾再附 ACTION（如有）。
  - 解析 LLM 输出时分离对话文本与 ACTION 行，对话文本显示在对话面板，ACTION 行触发 pending 确认。
- 难点：LLM 输出格式需兼顾自由对话与结构化 ACTION；历史上下文管理（避免无限增长）。

### T3. 滚动方向反了

- 现象：`↑` 向上滚但视觉内容向下移，与鼠标滚轮习惯相反。
- 修复：对调 `↑`/`↓` 的 `chat_scroll` 增减方向（`↑` = scroll 值减小 = 看更上方内容 → 实际应增大 scroll；当前逻辑 `↑` = scroll+3 = 内容上移，与直觉相反）。需对调。
- 一行改动。

### T4. 文本框更新时强制拉回顶部

- 现象：每次对话内容更新（流式 Delta、新消息），滚动位置被重置为 0（最底部），用户正在向上浏览历史时被拉回。
- 修复：
  - 流式 Delta 期间**不重置** scroll（让用户自由浏览）。
  - 仅在用户主动提交消息时才自动滚到底部（`push_chat` 时 reset）。
  - 需区分「流式增量更新」和「新消息到达」两种场景。

### T5. LLM 运行时程序卡顿

- 现象：LLM 流式输出期间 TUI 卡顿、按键无响应。
- 原因分析：
  - 主循环 `select!` 的 `sleep(33ms)` + `poll(0ms)` 可能不够：`poll` 返回 false 后立即进入下一轮 `select!`，CPU 空转。
  - `terminal.draw()` 每轮都全量重绘，流式高频 Delta 导致频繁重绘。
  - 后台 `consume_stream` 用 `UnboundedReceiver`，Delta 事件高频推送，主循环 `try_recv` 一次只取一条 → 积压。
- 修复方向：
  - 主循环 `select!` 的 timeout 提高到 50-100ms（降渲染帧率）。
  - 流式 Delta 批量处理：主循环中一次 `try_recv` 取尽所有待处理 Delta，合并后只重绘一次。
  - 限制重绘频率：仅当状态变化时才 `draw`，或做脏标记。
  - 考虑用 `tokio::select!` 直接等 `bt_rx.recv()` 而非 sleep + try_recv。

### T6. 退出时未保存 session

- 现象：TUI 模式输入"退出"后程序关闭，但 `data/sessions/` 无新文件。
- 原因：TUI `runner.rs` 的退出路径（`should_quit=true` → `break`）直接跳到终端恢复，未调用 `store.save(&session)`。TUI runner 中根本没有 `Session`/`SessionStore`——只有 `--play` 裸文本模式有存盘。
- 修复：TUI runner 中引入 `Session` + `SessionStore`，每轮 `ExecDone` 和 `StreamDone` 时存盘，退出时 `session.finished = true; store.save()`。

### T7. LLM 流中无法打断

- 现象：LLM 思考/输出时用户发消息，Agent 不打断，而是分两段甚至顺序错乱。
- 原因：用户 Enter 提交后走 `llm_parse_intent`（一次额外 LLM 调用），结果通过 `Backend::IntentReady` 异步回主循环。如果当前正在 Streaming 模式，`IntentReady` 到达后 `handle_user_intent` 中的 `Interrupt` 分支只 `cancel.cancel()` + 清空 `streaming_text`，但后台 `consume_stream` task 可能仍在跑（`cancel` 信号需要传到 `consume_stream` 的 `select!`）。另外 `Chat` 分支会发起新的 `start_decision`，与旧流叠加。
- 修复方向：
  - 用户提交时如果当前在 Streaming/Executing 模式，先 `cancel.cancel()` + 等待旧流结束（`stream_rx` 清空），再处理新意图。
  - `consume_stream` 已经有 `cancel.cancelled()` 分支，但 `cancel` 是 `CancellationToken` clone——需确认 clone 传入的是同一个 token。
  - 简化：用户在任何时候提交都先取消当前流，把用户消息加入历史，然后按意图处理。

### T8. 输入文本在 UI 中显示两次

- 现象：用户输入一条消息，对话面板出现两条 `[你]` 记录。
- 原因：`handle_user_intent` 的每个分支（Confirm/Reject/Chat/Interrupt）都 `state.push_chat(MsgRole::User, text)`，但 `IntentReady` 到达前用户消息已经被 `push_chat` 了一次（在 Enter 处理器里）。两个路径重复 push。
- 修复：Enter 处理器中不 push（交给 `handle_user_intent` 统一 push），或 `handle_user_intent` 中不 push（因为 Enter 处理器已 push）。选一个入口。

### T9. UI 对话面板不自动换行

- 现象：长行被截断而非换行，对话内容显示不全。
- 原因：之前去掉 `Paragraph::Wrap` 改用手动截断（解决 scroll offset 不精确问题），导致超长行直接截断丢失内容。
- 修复方向：恢复 `Wrap`，但用 `Paragraph::scroll` 按渲染后行数（而非原始行数）计算 offset。或用 `textwrap` crate 预先 wrap 文本再计算精确行数。关键：scroll offset 需与 Wrap 后实际行数一致。

### T10. Agent 每次只能做一步操作且需用户确认

- 现象：Agent 给出建议 → 用户"执行"确认 → 执行一步 → Agent 再给建议 → 用户再"执行"……非常机械。
- 用户期望：
  (1) Agent 可以直接执行操作而不必等用户确认。
  (2) Agent 可以自动判断能否做多步操作并自己连续执行（如出牌→出牌→结束回合）。
- 修复方向：
  - system prompt 改为允许输出**多条 ACTION 行**（ACTION: ... / ACTION: ...），LLM 判断是否连续操作。
  - `parse_action` 改为 `parse_actions`：解析多条 ACTION。
  - 执行循环：逐条执行，每条执行后重取状态检查是否合法，连续操作直到无 ACTION 或用户打断。
  - 用户确认模式改为可选（默认自动执行，用户可随时打断或说"等一下"暂停）。
  - 安全阀：连续操作最多 N 步（可配置），防失控。

---

*本计划为活文档，随实现进展与接口契约明确后迭代更新。*
