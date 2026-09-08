# STS2Agent

面向《杀戮尖塔 2》（Slay the Spire 2）的专用决策 Agent。玩家在游戏过程中随时获取当前回合的决策建议及解释，对局结束后可进行完整的决策链复盘分析。

Agent 通过游戏 Mod 接口（[STS2MCP](https://github.com/Gennadiyev/STS2MCP/)）读取实时状态，借助决策工具和 LLM 将决策转化为自然语言解释。

## 功能

- **实时回合决策建议**：分析手牌、敌人意图、能量，给出行动方案及理由
- **自然语言对话**：与 Agent 自由交流策略——问"为什么不打这张牌""换个方案"等
- **自主模式**：说"自己打"让 Agent 连续自动操作，随时可打断；开启需在 UI 确认（y/n），右上角显示自主模式标志；游戏加载中自动重试、GameOver 正确收尾
- **原生工具调用**：LLM 经 OpenAI 兼容 `tool_calls` 结构化通道发起操作，与显示文本分离；供应商不支持时自动回退文本 ACTION 行
- **思考过程展示**：`--thinking` 开启后 LLM 思考实时流式显示（浅色），完成后固化为 `[思考]` 历史消息保留；LLM 等待期间顶栏显示实时秒数
- **知识库主动查询**：LLM 遇到不认识的卡牌/敌人/遗物/事件时自动查询（本地结构化索引 → 游戏 Wiki 两级），结果注入后续决策上下文
- **对局复盘**：加载历史会话，逐回合回放状态→决策→解释的完整链路
- **会话恢复**：`--tui --load <id>` 恢复上次会话的完整上下文继续对话（LLM 带历史记忆、用量累计接续）
- **Token 用量与成本统计**：精确统计每次 API 调用的 token 数与费用（含缓存命中），支持预算上限自动中断
- **知识库辅助**：`game-knowledge/` 结构化索引（577 张卡牌、121 种敌人、64 种药水、68 个事件，从游戏反编译数据生成），根据当前局面按 ID 精确查表，自动注入 LLM 上下文
- **跨回合策略记忆**：LLM 经 `PLAN:` 行声明作战计划，内核逐轮回显 + 回显最近已执行操作——自主循环中接着上次的操作往下想，不重复思考
- **可自定义模型配置**：支持 OpenAI / DeepSeek / 清华平台 / 本地 vLLM 等 OpenAI 兼容接口，可配置 endpoint、api_key、上下文长度、思考模式、价格等
- **实时进度渲染与打断**：流式输出 + 按键打断，长任务不卡顿
- **上下文历史管理**：每会话存为 JSON（对话/动作/状态快照/用量），支持列出/回放/恢复，非黑盒

## 系统架构

```
┌─────────────┐   MCP (JSON-RPC/stdio)   ┌──────────────────────────────────────────┐
│  STS2 游戏   │ ◀─────────────────────▶ │  Rust Agent                              │
│  + STS2MCP   │                          │                                          │
│  Mod (C#)    │                          │  ┌──────────┐  ┌──────────┐  ┌────────┐ │
│  HTTP :15526 │                          │  │ MCP 客户 │─▶│ 知识库+  │─▶│ LLM    │ │
└─────────────┘                          │  │ 端+Mock  │  │ 决策编排 │  │ 客户端 │ │
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
cargo run -p sts2-tui -- --tui --zh

# 单次决策（裸文本，无需 TTY）
cargo run -p sts2-tui -- --decide --zh

# 自动对局（裸文本）
cargo run -p sts2-tui -- --play --zh --max-turns 6
```

加 `--mock` 可使用内置的 Rust Mock MCP server（脚本化战斗），无需游戏即可端到端演示。

## 使用方式

### TUI 交互模式

```bash
cargo run -p sts2-tui -- --tui --zh
```

启动后看到状态面板（HP/能量/手牌/敌人）+ 对话面板 + 输入框。操作：

| 输入 | 效果 |
|---|---|
| `分析一下` | Agent 给出文字分析建议（**不操作游戏**） |
| `出第二张牌` / `结束回合` | Agent 执行该单次操作 |
| `执行` / `继续` | 确认执行 Agent 本轮的建议（仅执行一次） |
| `自己打` | Agent 请求开启自主模式 → 输入 `y` 同意 / `n` 拒绝后生效，之后连续自动操作直到完成或打断 |
| `自己打这层` / `自己打这局` | 同上，限定范围 |
| `停` / `打断` | 打断当前 LLM 流并退出自主模式（含取消未确认的自主请求） |
| `↑` / `↓` | 滚动对话历史 |
| `Esc` | 退出程序 |

> **自主模式**只在用户明确说"自己打"时触发。其余任何输入（包括"执行""分析一下""你来吧"）都不会让 Agent 连续自动操作。

### 裸文本模式

```bash
# 单次决策
cargo run -p sts2-tui -- --decide --zh

# 自动对局（LLM 自主决策 + 执行 + 循环）
cargo run -p sts2-tui -- --play --zh --max-turns 6

# 显示 LLM 思考过程
cargo run -p sts2-tui -- --play --zh --thinking
```

### 会话历史

```bash
# 列出历史会话
cargo run -p sts2-tui -- --list

# 只读回放（打印每轮的对话、动作、执行结果）
cargo run -p sts2-tui -- --load <session-id>

# 恢复会话上下文，在 TUI 里继续对话（LLM 带历史记忆，用量累计接续）
cargo run -p sts2-tui -- --tui --load <session-id>
```

会话以 JSON 保存在 `data/sessions/`，包含每轮的用户输入、Agent 输出、动作与执行结果、状态快照、token 用量与成本。

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

[budget]
token_limit = 0                           # 0 = 不限；超出自动中断
cost_limit_usd = 0.0                      # 0.0 = 不限

[storage]
sessions_dir = "data/sessions"
logs_dir = "data/logs"
game_knowledge_dir = "game-knowledge"       # 结构化游戏数据索引目录
```

### Windows 原生运行

Agent 与游戏都在 Windows 上时，`[mcp]` 直接用本地 Python 跑 server.py（无需 WSL 桥接）：

```toml
[mcp]
command = "python"
args = ["-X", "utf8", "C:\\path\\to\\STS2MCP\\mcp\\server.py"]
```

> `-X utf8` 规避 Windows Python 默认 GBK 编码问题。

### WSL + Windows 混合运行（Agent 在 WSL，游戏在 Windows）

游戏跑在 Windows、Agent 跑在 WSL 时，WSL 侧无法直连游戏的 `localhost:15526`（Mod 的 HTTP server 只绑定 Windows 的 localhost）。解法：**让 Python MCP server 跑在 Windows 端**，Agent 通过 `powershell.exe` 桥接它的 stdio。

1. **准备包装脚本**：参考仓库根目录的 `win_server.example.py`，在同目录复制一份为 `win_server.py`（此文件含个人路径，已 gitignore 不入库），把其中的 `mcp_dir` 改成你的实际路径。它的作用：用 Windows Python 以 UTF-8 编码读取并执行 WSL 路径下的 `server.py`（进程实际跑在 Windows 端，其 `localhost` 就是游戏所在端）。

2. **配置 `config/config.toml`**：

```toml
[mcp]
command = "powershell.exe"
args = ["-c", "python '\\\\wsl.localhost\\Ubuntu-24.04\\home\\<user>\\sts2agent\\win_server.py'"]
```

   路径说明：`\\wsl.localhost\<发行版>\...` 是 Windows 访问 WSL 文件的 UNC 路径（注意 TOML 里反斜杠要双写转义）；`<user>` 换成你的 WSL 用户名。**前提**：Windows 已安装 Python，且 `powershell.exe` 在 WSL 的 PATH 中可用（WSL 默认自带 Windows 互操作）。

3. **链路**：Agent（WSL）→ stdio → powershell.exe → Windows Python（执行 win_server.py → 加载 server.py）→ `localhost:15526` → 游戏 Mod。

**常见坑**：
- 连接挂起/返回 `Error:`：先确认游戏开着、Mod 已启用（游戏日志应出现 `[STS2 MCP] server started`）；
- 检查有没有残留的端口转发劫持 15526：`netsh interface portproxy show v4tov4`（有就 `delete` 掉）；

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
│  ├─ sts2-core/        # 领域模型 (GameState/Config) + serde
│  ├─ sts2-mcp/         # MCP 客户端 + Mock MCP server (演示用)
│  ├─ sts2-llm/         # OpenAI 兼容 LLM 客户端 (流式/usage/价格/预算)
│  ├─ sts2-knowledge/   # 知识库 skill (检索/查询/Wiki 兜底) + MCP server 二进制
│  ├─ sts2-agent/       # 编排主控 (会话/历史/取消/进度/存储)
│  └─ sts2-tui/         # ratatui TUI 界面
├─ config/
│  ├─ config.example.toml
│  └─ .env.example
├─ game-knowledge/         # 结构化游戏数据索引 (反编译生成，已入库)
│  ├─ cards.md / card-behaviors.md        # 卡牌索引 + 行为
│  ├─ monsters.md / monster-behaviors.md  # 敌人索引 + 行为
│  ├─ potions.md / potion-behaviors.md    # 药水索引 + 行为
│  ├─ events.md / characters.md           # 事件 + 角色开局
│  └─ playbook.md / agent-reference.md    # 决策流程指引
├─ data/                   # 运行时数据 (gitignore)
│  ├─ sessions/            # 会话历史 JSON
│  └─ logs/                # 日志 (含 MCP server stderr)
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
cargo test --workspace                     # 测试
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
