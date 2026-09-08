# 交接文档 HANDOVER

> 写给下一个接手的 agent。本文件是完整的工作交接：项目理解、架构现状、关键坑、用户约定、待办事项。
> 阅读顺序：AGENTS.md → 本文件 → PLAN.md（§14 有 T1-T10 核查表）→ DEVLOG.md（每步细节）。

---

## 1. 项目是什么

《杀戮尖塔 2》专用决策 Agent（大学大作业）。六大要求 R1-R6 均已实现：

- **R1** Rust 核心：Cargo workspace 6 crate，主控在 sts2-agent
- **R2** 界面：ratatui TUI 对话模式（sts2-tui）
- **R3** 模型配置：config.toml + .env（endpoint/key/model/价格/上下文长度/思考模式）
- **R4** 进度+打断：流式输出 + Esc/打字打断 + CancellationToken
- **R5** 历史：session JSON 存盘（对话/动作/状态/用量）+ `--list` / `--load` 回放 / `--tui --load` **恢复上下文继续对话**
- **R6** 用量统计：BudgetGuard 精确统计 input/output/cached token 与成本，预算超额自动停

## 2. 拓扑与环境（关键！）

```
WSL (Ubuntu-24.04)                          Windows
┌─────────────────────────┐                ┌──────────────────────┐
│ Rust agent (sts2-tui)   │  powershell.exe│ 杀戮尖塔2 + STS2_MCP  │
│  └ spawn → powershell   │ ─────────────▶ │  Mod (C#) HTTP :15526│
│     .exe 跑 win_server  │                │  (localhost)         │
│     .py（Windows python │                └──────────────────────┘
│     执行 WSL 路径的      │
│     server.py，stdio    │
│     JSON-RPC）          │
└─────────────────────────┘
```

- **config.toml 的 mcp command**：`powershell.exe -c "python '\\wsl.localhost\<发行版>\home\<用户名>\sts2mcp\win_server.py'"`。win_server.py 在 `/home/<用户名>/sts2mcp/win_server.py`（仓库外），内容：读 WSL UNC 路径的 STS2MCP/mcp/server.py 并 exec。**Windows python 跑在 Windows 端，其 localhost 指向 Windows**，所以能直连游戏 Mod，无需端口转发。
- **不要用 netsh portproxy**！上次排查 502 时发现残留 portproxy（注册表持久化）会劫持 15526 端口导致连接挂起。已删除。若再遇到 502/空响应，先查 `netsh interface portproxy show v4tov4`。
- **游戏 Mod 必须重新编译**（游戏版本更新，上游 Release 的 DLL 不兼容）。源码在 `/home/<用户名>/sts2mcp/STS2MCP/`，`build.ps1 -GameDir "<游戏目录>"` 编译，产物拷到 `mods/`。
- API key 在 `config/.env`（STS2_OPENAI_API_KEY）。config.toml 的 api_key 留空。**load_config 优先读 config/.env**（dotenvy::from_path），再回退 cwd/.env。
- LLM：清华平台 `https://lab.cs.tsinghua.edu.cn/ai-platform/api/v1`，模型 glm-5。**对 tools（function calling）支持良好**。

## 3. 代码结构（重构后）

```
crates/
├─ sts2-core/      # GameState/Action/Config serde 模型（宽进严出，flatten extra）
├─ sts2-mcp/       # McpClient（stdio JSON-RPC 子集）+ Mock server（bin/sts2-mcp-mock）
├─ sts2-decision/  # 空壳：DecisionEngine trait 预留（实际决策由 LLM 驱动）
├─ sts2-llm/       # LlmClient：流式 SSE + tools/tool_calls 解析 + usage/cached + 预算
├─ sts2-agent/
│  ├─ decide.rs    # build_messages（prompt 构造）+ tool_definitions() + run_decide
│  ├─ parse.rs     # ACTION 文本解析（回退用）+ parse_tool_call + normalize_tool/args
│  ├─ play.rs      # --play 裸文本自动对局循环
│  ├─ slim.rs      # 状态 JSON 瘦身（递归删 keywords/null 字段，省 ~25% token）
│  ├─ lookup.rs    # LLM 主动查询：lookup_query（本地表格）+ search_wiki_via_mcp（Wiki 兜底）
│  ├─ context.rs   # 决策前自动注入：search_game_knowledge + 未知手牌检测
│  └─ storage.rs   # Session/TurnRecord/SessionStore（data/sessions/*.json）
└─ sts2-tui/
   ├─ main.rs      # CLI 分发（--check/--list/--load/--tui/--decide/--play）
   ├─ app.rs       # AppState（auto_mode/task/lookup_context/no_action_streak...）
   ├─ ui.rs        # ratatui 绘制（手动 wrap_line 换行）
   └─ runner/
      ├─ mod.rs    # run() 主循环 + 按键 + Backend enum
      ├─ stream.rs # start_decision（拼 prompt 发 LLM）/ consume_stream / abort
      ├─ backend.rs# 消息编排：on_stream_done / on_stale_decision / on_exec_done /
      │            # on_state_ready / spawn_state_stabilize / append_session_notes
      ├─ intent.rs # parse_intent（关键词）+ handle_user_intent
      └─ actions.rs# try_reflex_action（免 LLM 机械操作）/ spawn_exec / handle_lookup
game-knowledge/    # 12 个 .md：反编译生成的结构化表格（577卡/121敌/64药水/68事件）
```

## 4. 核心机制（当前设计，务必理解再改）

### 4.1 LLM 调用纪律（用户强需求）
- **无后台状态轮询**（poll_state_loop 已删除！）。原因：STS2MCP Mod 的 BuildGameState 在 shop 房间会调用 `OpenInventory()` 打开商人界面（McpMod.StateBuilder.cs ~551，上游副作用），轮询会导致用户关不掉商店界面。
- 状态读取只有两个入口：① 用户意图处理（refresh_and_decide 里 GET 一次）② 自主模式执行动作后（spawn_state_stabilize：0.8s 等待 + 双取 0.3s 间隔确认稳定 → StateReady）。
- 代价：用户手动打游戏时面板不自动刷新（用户接受）。

### 4.2 自主模式（推平重写过两轮，当前为最终设计）
- LLM 通过**内部工具请求**切换：`auto_start(task=...)` / `auto_stop`（本地拦截，不发给游戏）。
- Rust 是唯一权威：`state.auto_mode` 只被这两个请求或用户喊"停"（parse_intent → Interrupt，Rust 直关不经 LLM）改变。
- **门禁**：非自主模式下一切游戏操作被无条件否决（⛔ 提示+丢弃）。lookup 不受限。
- 单次指令 = LLM 输出 auto_start + 操作 + auto_stop 三连；持续自主 = auto_start 后逐步操作直到 auto_stop。
- 防误退：自主中 LLM 无动作 → 纠正重问，连续 3 次（no_action_streak）才退出；prompt 强调"任务未完成绝不 auto_stop"。
- max_turns 默认 0=不限（用户要求取消上限），预算守卫兜底。

### 4.3 原生工具调用（tool_calls）
- 23 个工具定义在 `decide.rs tool_definitions()`（游戏操作 + lookup/auto_start/auto_stop）。
- 流式解析 delta.tool_calls 分片（按 index 合并），StreamDone 携带完整列表。
- 动作来源：tool_calls 优先；空则**回退解析文本 ACTION 行**（供应商不支持 tools 时兜底）。parse.rs 两条路共用 normalize_tool/normalize_args。
- pending_actions 是 `Vec<ParsedAction>`（已解析，队列执行零解析）。

### 4.4 知识查询（两级）
- 自动注入：context.rs search_game_knowledge 按手牌/敌人/药水/事件 ID 查表 + playbook 段落 + **未知手牌显式标注**（"[!] 知识库未收录…必须查询或明说不确定"）。
- LLM 主动：`lookup(query)` → 本地表格（lookup_query）→ 未命中查显示名转 ID（lookup_query_smart）→ 仍未命中调 MCP `search_wiki`（游戏本体 wiki，卡牌/遗物，档案已解锁内容）。每次任务最多 3 次，结果累积进 lookup_context 注入后续 prompt。
- **ID 格式坑**：真实游戏 card.id 是 `STRIKE_R`（单字母角色后缀），知识库表格列是 `StrikeRegent`（PascalCase）。`strip_card_suffix` 剥掉长度≤2 的下划线段后再 normalize 匹配。

### 4.5 prompt 要点（decide.rs system prompt，用户反复调优过，慎改）
- LLM 角色：牌手顾问，**不是**决策者。
- 防幻觉：效果以状态 JSON description 为准；禁止用 STS1 经验推 STS2；数字只引用可见值；**状态 JSON 数值已含 buff 影响，禁止重复计算**。
- 回复格式：**禁 Markdown**（UI 无法渲染）；极简 5 句内；只输出分析/结论/打法。
- 中文输出（--zh）。
- 历史：build_messages 只保留最近 10 条 ChatTurn。

### 4.6 反射动作（省 token）
自主循环里机械操作不问 LLM：奖励从右到左领+proceed、CardSelect 可确认时直接确认、宝箱唯一遗物直接拿（actions.rs try_reflex_action）。

## 5. 踩过的坑（血泪史，别再踩）

1. **portproxy 残留**：502/空响应的元凶之一，见 §2。
2. **Mod 主线程挂起**：游戏不在跑/Mod 没初始化时，GET 会 TCP 连上但永不响应（Mod 的请求走 RunOnMainThread 等主循环）。游戏日志无 `[STS2 MCP]` = Mod 没起来。
3. **stderr 曾被 Stdio::null**：Python server 报错全不可见。现在 stderr 落 `data/logs/mcp.log`（**/data/logs 已 gitignore，含子目录**）。
4. **StreamDone 状态校验**：决策期间游戏状态变了 → 丢弃输出（会提示 ⚠️）。lookup 后要**重取状态**再 start_decision，否则校验必挂。
5. **shop 轮询副作用**：见 §4.1，无轮询已根治。
6. **menu_select 的 option 必须是字符串**（pydantic）——normalize_args 已强制转换 option/tool 为 string。
7. **意图分类不要用 LLM**：曾每句话额外调一次 LLM 分类，删掉改纯关键词（parse_intent），省一次调用。
8. **UI progress 残留**：回答完不清 progress 会一直显示"回复中"，各收尾分支都要 `progress = None`。
9. **流挂起**：供应商不发 [DONE] 会永远挂——chat_stream 有 90s IDLE_TIMEOUT 兜底。
10. **中文引号**：format! 字符串里用中文双引号 "" 会切断 Rust 字符串，用「」。

## 6. 用户约定（必须遵守）

- **不主动 git push**！用户说"推送"才推。当前 origin/main..HEAD 有 **20 个未推送提交**（从 "Reduce hallucination and token usage" 到 "README: document new features..."）。
- 用户消息即需求，改完自查 fmt/clippy/test（`cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test --workspace`，当前 37+ 测试全绿）。
- 用户反感屎山：大函数要拆（runner.rs 曾 1067 行被拆成 5 模块）；重复逻辑要收敛。
- LLM 输出要短、纯文本、无 Markdown。
- 每个 crate `#![forbid(unsafe_code)]`。
- commit message 英文、说清 root cause。

## 7. 待办 / 遗留

- [ ] **真实游戏实测**：tool_calls 在清华平台的实战稳定性、自主模式新架构（auto_start/auto_stop）的实际表现——mock 已验证，真实环境未充分测试。
- [ ] T3 滚动方向：当前 ↑=看上方历史（TUI 惯例），PLAN.md §14 标注"待用户定夺"。
- [ ] decide.rs run_decide 里还有 `[诊断] get_game_state 返回 X 字节` 打印（排查期遗留，可清理）。
- [ ] Mod 上游修复：把 OpenInventory 移出状态读取路径（StateBuilder.cs ~551），改后可恢复后台轮询（如果想要面板自动刷新）。源码可本地改+重编译。
- [ ] sts2-decision crate 是空壳，README 已如实描述为 trait 预留。
- [ ] PLAN.md §13 风险 7（--zh 思考偶尔英文）未修。
- [ ] 会话 turns 里 user_input 只在 TUI 记录；--play 模式 user_input 恒 None。

## 8. 常用命令

```bash
cargo fmt && cargo clippy --all-targets -- -D warnings && cargo test --workspace
cargo run -p sts2-tui -- --check                          # 校验配置
cargo run -p sts2-tui -- --tui --zh                       # TUI（真实 MCP）
cargo run -p sts2-tui -- --tui --mock --zh                # TUI（Mock）
cargo run -p sts2-tui -- --play --mock --zh               # 裸文本自动对局
cargo run -p sts2-tui -- --decide --mock --zh             # 单次决策
cargo run -p sts2-tui -- --list / --load <id>             # 会话查看
cargo run -p sts2-tui -- --tui --load <id>                # 恢复会话继续聊
echo '<json-rpc>' | ./target/debug/sts2-mcp-mock          # 手测 Mock
rm -f data/logs/mcp.log                                   # 清 MCP 日志再复现问题
```

## 9. 本轮（agent2）完成的大事记

按 DEVLOG 顺序：menu_select/知识库验证提交 → T1-T10 核查 → dotenvy 路径修复+key 迁移 .env → 真实 MCP 联调（portproxy 根因）→ 自主模式严格化（"自己打"）→ README → game-knowledge 结构化库 → 删旧攻略库 → LLM 简洁化禁 Markdown → 降幻觉省 token 六件套 → lookup 主动查询+ID 格式修复+wiki 兜底 → 修 lookup 丢上下文/静默丢弃 → 架构重构（runner/knowledge 拆模块）→ 删后台轮询 → 修 Interrupt 队列残留 → 修 UI 卡回复中 → 自主模式两轮重构（显式请求+门禁+防误退+提速）→ 原生 tool_calls → 取消轮数上限 → R5 补全（会话记录+恢复）→ README 更新。

细节全部在 DEVLOG.md，按章节可查。

---

*交接完毕。祝顺利。*
