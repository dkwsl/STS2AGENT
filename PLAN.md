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

*本计划为活文档，随实现进展与接口契约明确后迭代更新。*
