# STS2Agent

面向《杀戮尖塔 2》（Slay the Spire 2）的专用决策 Agent。玩家在游戏过程中随时获取当前回合的决策建议及解释，对局结束后可进行完整的决策链复盘分析。

Agent 通过游戏 Mod 接口（[STS2MCP](https://github.com/Gennadiyev/STS2MCP/)）读取实时状态，借助决策工具和 LLM 将决策转化为自然语言解释。

## 功能

- **实时回合决策建议**：分析手牌、敌人意图、能量，给出最优行动方案及理由
- **自然语言对话**：与 Agent 自由交流策略——问"为什么不打这张牌""换个方案"等
- **自主模式**：说"自己打"让 Agent 连续自动操作一整局，随时可打断
- **对局复盘**：加载历史会话，逐回合回放状态→决策→解释的完整链路
- **Token 用量与成本统计**：精确统计每次 API 调用的 token 数与费用，支持预算上限自动中断
- **知识库辅助**：两层知识体系——`game-knowledge/` 结构化索引（577 张卡牌、121 种敌人、64 种药水、68 个事件，从游戏反编译数据生成，按 ID 精确查表）+ `data/knowledge/raw/` 攻略文章（按关键词模糊检索），均根据当前局面自动注入 LLM 上下文
- **经验笔记**：Agent 在对局中自主记录经验教训（NOTE），后续回合自动引用
- **可自定义模型配置**：支持 OpenAI / DeepSeek / 清华平台 / 本地 vLLM 等 OpenAI 兼容接口，可配置 endpoint、api_key、上下文长度、思考模式、价格等
- **实时进度渲染与打断**：流式输出 + 按键打断，长任务不卡顿
- **上下文历史管理**：每会话存为 JSON，支持列出/加载/导出，非黑盒

## 系统架构

```
┌─────────────┐   MCP (JSON-RPC/stdio)   ┌──────────────────────────────────────────┐
│  STS2 游戏   │ ◀─────────────────────▶ │  Rust Agent                              │
│  + STS2MCP   │                          │                                          │
│  Mod (C#)    │                          │  ┌──────────┐  ┌──────────┐  ┌────────┐ │
│  HTTP :15526 │                          │  │ MCP 客户 │─▶│ 决策引擎 │─▶│ LLM    │ │
└─────────────┘                          │  │ 端+Mock  │  │ (可插拔) │  │ 客户端 │ │
       ▲                                  │  └──────────┘  └──────────┘  └───┬────┘ │
       │ 演示用 Mock                       │  ┌──────────────────────────────▼───┐ │
       ▼                                  │  │ 编排层: 会话/历史/取消/进度/预算   │ │
┌─────────────┐                          │  └──────────────────────────────────┘ │
│ Mock MCP    │                          │  ┌──────────┐  ┌────────────────────┐  │
│ server(Rust)│                          │  │ 存储层   │  │ TUI (ratatui)      │  │
└─────────────┘                          │  │ JSON/配置│  │ 对话/进度/历史/用量 │  │
                                         │  └──────────┘  └────────────────────┘  │
                                         └──────────────────────────────────────────┘
```

核心编排与表现层解耦：`sts2-agent` 对外暴露"事件流 + 命令"接口，任何前端（TUI/Web/桌面）只消费事件、下发命令，复用同一核心。

## 快速开始

### 1. 构建

```bash
cargo build --workspace
```

### 2. 配置

```bash
cp config/config.example.toml config/config.toml
cp config/.env.example config/.env
```

编辑 `config/config.toml`：
- `[model]`：填入 endpoint、model、价格等（api_key 留空，从 .env 读）
- `[mcp]`：配置 MCP server 启动命令

编辑 `config/.env`：
```
STS2_OPENAI_API_KEY=sk-your-key-here
```

> 密钥仅存于 `config/.env`（已被 `.gitignore` 忽略），不会入库。

### 3. 运行

```bash
# 交互式 TUI 对话界面（推荐）
cargo run -p sts2-tui -- --tui --mock --zh

# 单次决策（裸文本，无需 TTY）
cargo run -p sts2-tui -- --decide --mock --zh

# 自动对局（裸文本）
cargo run -p sts2-tui -- --play --mock --zh --max-turns 6
```

`--mock` 使用内置的 Rust Mock MCP server（脚本化战斗），无需游戏即可端到端演示。去掉 `--mock` 连接真实游戏。

## 使用方式

### TUI 交互模式

```bash
cargo run -p sts2-tui -- --tui --mock --zh
```

启动后看到状态面板（HP/能量/手牌/敌人）+ 对话面板 + 输入框。操作：

| 输入 | 效果 |
|---|---|
| `分析一下` | Agent 给出文字分析建议（**不操作游戏**） |
| `出第二张牌` / `结束回合` | Agent 执行该单次操作 |
| `执行` / `继续` | 确认执行 Agent 本轮的建议（仅执行一次） |
| `自己打` | 进入自主模式，Agent 连续自动操作直到完成或打断 |
| `自己打这层` / `自己打这局` | 自主模式，限定范围 |
| `停` / `打断` | 打断当前 LLM 流并退出自主模式 |
| `↑` / `↓` | 滚动对话历史 |
| `Esc` | 退出程序 |

> **自主模式**只在用户明确说"自己打"时触发。其余任何输入（包括"执行""分析一下""你来吧"）都不会让 Agent 连续自动操作。

### 裸文本模式

```bash
# 单次决策
cargo run -p sts2-tui -- --decide --mock --zh

# 自动对局（LLM 自主决策 + 执行 + 循环）
cargo run -p sts2-tui -- --play --mock --zh --max-turns 6

# 显示 LLM 思考过程
cargo run -p sts2-tui -- --play --mock --zh --thinking
```

### 会话历史

```bash
# 列出历史会话
cargo run -p sts2-tui -- --list

# 加载回放
cargo run -p sts2-tui -- --load <session-id>
```

会话以 JSON 保存在 `data/sessions/`，包含每轮的状态快照、决策、LLM 输出、token 用量与成本。

## 配置说明

### `config/config.toml`

```toml
[model]
endpoint = "https://api.openai.com/v1"  # OpenAI 兼容 API 地址
api_key = ""                              # 畺空则从 .env 读取
model = "gpt-4o-mini"
context_length = 128000
thinking_mode = false                     # 是否请求 reasoning_content
price_in = 0.15                           # 每百万输入 token 美元
price_out = 0.60                          # 每百万输出 token 美元

[mcp]
command = "uv"                            # MCP server 启动命令
args = ["run", "--directory", "/path/to/STS2MCP/mcp", "python", "server.py"]

[decision]
engine = "rule_based"                     # rule_based（内置）；预留 mcts / remote

[budget]
token_limit = 0                           # 0 = 不限；超出自动中断
cost_limit_usd = 0.0                      # 0.0 = 不限

[storage]
sessions_dir = "data/sessions"
logs_dir = "data/logs"
knowledge_dir = "data/knowledge/raw"        # 攻略文章目录
game_knowledge_dir = "game-knowledge"       # 结构化游戏数据索引目录
```

### 切换 LLM 供应商

修改 `config/config.toml` 的 `[model]` 段即可：

| 供应商 | endpoint | model 示例 |
|---|---|---|
| OpenAI | `https://api.openai.com/v1` | `gpt-4o-mini` |
| DeepSeek | `https://api.deepseek.com/v1` | `deepseek-chat` |
| 清华平台 | `https://lab.cs.tsinghua.edu.cn/ai-platform/api/v1` | `glm-5` |
| 本地 vLLM | `http://localhost:8000/v1` | `Qwen2.5-72B-Instruct` |

## 项目结构

```
sts2agent/
├─ crates/
│  ├─ sts2-core/        # 领域模型 (GameState/Action/Config) + serde
│  ├─ sts2-mcp/         # MCP 客户端 + Mock MCP server (演示用)
│  ├─ sts2-decision/    # DecisionEngine trait (可插拔，预留扩展)
│  ├─ sts2-llm/         # OpenAI 兼容 LLM 客户端 (流式/usage/价格/预算)
│  ├─ sts2-agent/       # 编排主控 (会话/历史/取消/进度/存储/知识库)
│  └─ sts2-tui/         # ratatui TUI 界面
├─ config/
│  ├─ config.example.toml
│  └─ .env.example
├─ scripts/
│  └─ knowledge/fetch.py  # 爬取攻略到 data/knowledge/raw/
├─ game-knowledge/         # 结构化游戏数据索引 (反编译生成，已入库)
│  ├─ cards.md / card-behaviors.md        # 卡牌索引 + 行为
│  ├─ monsters.md / monster-behaviors.md  # 敌人索引 + 行为
│  ├─ potions.md / potion-behaviors.md    # 药水索引 + 行为
│  ├─ events.md / characters.md           # 事件 + 角色开局
│  └─ playbook.md / agent-reference.md    # 决策流程指引
├─ data/                   # 运行时数据 (gitignore)
│  ├─ sessions/            # 会话历史 JSON
│  ├─ logs/                # 日志 (含 MCP server stderr)
│  └─ knowledge/raw/       # 爬取的攻略文章
├─ Cargo.toml
├─ AGENTS.md               # 工作指令
├─ PLAN.md                 # 项目计划
└─ DEVLOG.md               # 开发日志
```

## 开发

```bash
cargo fmt                                  # 格式化
cargo fmt --check                          # 格式校验
cargo build --workspace                    # 构建
cargo clippy --all-targets -- -D warnings  # 严格 lint
cargo test --workspace                     # 测试 (35 项)
```

每个 crate 根以 `#![forbid(unsafe_code)]` 强制禁用 unsafe。

## 技术栈

| 领域 | 选型 |
|---|---|
| 语言 | Rust (edition 2021) |
| 异步运行时 | tokio |
| TUI | ratatui + crossterm |
| HTTP/LLM | reqwest (rustls-tls，零系统依赖) |
| MCP | 手写 stdio JSON-RPC 子集 (initialize/tools-list/tools-call) |
| 序列化 | serde / serde_json |
| 配置 | toml + dotenvy |
| CLI 参数 | clap |
