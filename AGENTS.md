# STS2AGENT 仓库工作指令

本文件约束所有在本仓库中工作的 coding agent。开始处理任务前完整阅读；子目录若有更具体的 AGENTS.md，其规则只补充对应目录，不能放宽这里的硬约束。

## 0. 大作业要求

### 0.1. 固定功能要求

下面六个模块必须要完整实现。

R1. 核心逻辑用 Rust 实现

核心业务逻辑（数据处理、算法流程、API 调用编排）必须用 Rust 写。允许调用其他语言的库（比如 Python 的 PyTorch），但主控流程必须在 Rust 里。

R2. 用户交互界面

至少提供以下之一：Web 界面、CLI 交互式终端、桌面 App、手机 App。界面必须能触发 Agent 任务，并展示结果。

R3. 可自定义模型配置

Agent 的用户必须能自由修改大模型的 API Endpoint 和 API Key（比如在 OpenAI 和本地模型之间切换），既可以通过配置文件（.env 或 config.toml），也可以通过 UI/CLI 设置页这类用户友好的界面。此外还要能配置上下文长度、思考模式、API 价格等。

R4. 实时进度渲染和打断功能

执行时间超过 3 秒的任务，UI/CLI 必须实时渲染进度，并且允许用户打断。比如处理照片时显示“已处理 45/120 张”，数学证明时显示“正在尝试证明引理...”。Web 端可以用 SSE/WebSocket 等技术，CLI 端可以用进度条库。

R5. 上下文历史管理

Agent 必须能管理多轮对话或任务状态的历史记录。用户能查看历史任务，也能保存/加载某次会话的完整上下文（比如存成 JSON 文件）。也就是说，用户能看到 Agent 任务背后实际的工作流程（如 DeepSeek Harness 的轨迹显示），而不是把它当成一个黑盒。

R6. Token 用量与价格统计

系统必须精确统计每次 API 调用的输入 token 数和输出 token 数（这两个数字在 API 响应里就能拿到），并根据你配置的模型价格（或预设价格表）实时换算成本。统计信息必须在界面上清晰展示。允许设置 token 预算，用量到预算时自动中断，免得月底看着账单流泪。

## 1. 项目内容

### 1.1. 目标

实现一个面向《杀戮尖塔2》的专用 Agent，玩家可在游戏过程中随时获取当前回合的决策建议及解释，对局结束后可进行完整的决策链复盘分析。Agent 通过游戏 Mod 接口读取实时状态，借助决策工具计算出行动方案，再由 LLM 将决策转化为自然语言解释。

### 1.2. 技术栈

**语言**：Rust（核心 Agent 程序）

**通信**：通过 MCP 协议与游戏交互

**决策**：使用可更改的决策工具获取行动建议

**解释生成**：调用 LLM API

**存储**：本地文件系统保存对局记录

## 2. 绝对不能做的事！！！

任何敏感信息绝对不能上传 git，包括 API key/token/密码等，一律不准出现在源码、测试、文档、日志、commit message 等文件中。

## 3. 外部依赖与集成约定

### 3.1 STS2MCP（游戏接口来源）

- 本项目不自带游戏 Mod，采用社区项目 STS2MCP。本地 clone 位于 `/home/aaa12321/sts2mcp/STS2MCP/`（在主目录内、本仓库外；勿将其内容提交到本仓库）。
- 拓扑：游戏内 C# Mod 起 HTTP API `http://localhost:15526`（无鉴权、仅本地）；随附 Python MCP server（`mcp/server.py`，FastMCP + stdio）作桥；Rust Agent 作 MCP 客户端，以 stdio 拉起该 Python server，经 MCP 工具读写状态。这满足 §1.2「通过 MCP 协议与游戏交互」。
- 启动命令（写入配置 `mcp.command` / `mcp.args`）：`uv run --directory /home/aaa12321/sts2mcp/STS2MCP/mcp python server.py`。
- 完整接口契约（工具清单 + 状态 JSON schema + 动作规则）见 `PLAN.md` §9。上游权威来源：`STS2MCP/mcp/server.py`、`STS2MCP/docs/raw-full.md`、`STS2MCP/docs/raw-simplified.md`、`STS2MCP/mcp/README.md`。
- 开发/演示阶段用 Rust 写的 Mock MCP server（与真实契约同构）；真实联调才需本机运行游戏并装 `STS2_MCP` Mod（非阻塞当前开发）。

### 3.2 接口交互硬规则（实现决策引擎与编排时必须遵守）

- 战斗用 `get_game_state(format="json")` 取结构化状态；菜单/地图可用 `markdown` 概览。
- 出牌会使手牌索引左移：从右到左出牌，或每次出牌后重取状态再算索引。
- 单体牌/药水须带 `target` = 敌人 `entity_id`（UPPER_SNAKE_CASE 带 `_0` 后缀，如 `JAW_WORM_0`）。
- `end_turn` 后需重取 `get_game_state` 推进回合（可能需取两次：一次看敌方回合结果，一次看新手牌）。
- 药水 `slot` 是药水槽索引而非牌索引；不耗能量、不计出牌；buff 药水先于出牌使用。
- 事件用 `choose_event_option`（含 Proceed 选项）；`proceed` 不适用于事件。
- 奖励领取从右到左以避免索引漂移；卡牌奖励会切到 `card_reward` 子屏。

## 4. 开发约定

- 核心逻辑用 Rust（R1），Cargo workspace 多 crate；主控流程在 `sts2-agent`。
- 禁用 `unsafe` 等不安全语法（每个 crate 根以 `#![forbid(unsafe_code)]` 强制；如确需绕过须先与用户确认）。
- 配置经 `config.toml` + `.env` 加载；密钥仅放 `secret/` 或环境变量，禁入 git（呼应 §2）。
- 日志仅落 `data/logs/`，对 `api_key` 等脱敏。
- 不提交 `secret/`、`.env`、`data/`、`target/`、用户覆盖配置等（见 `.gitignore`）。
