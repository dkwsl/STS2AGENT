//! sts2-mcp: MCP 客户端 + Mock MCP server（见 PLAN.md §5.2 / §9）。
//!
//! 真实链路：Rust Agent 作 MCP 客户端，stdio 拉起 STS2MCP 的 Python server，
//! 调用 `get_game_state(format="json")` 取状态、按 `state_type` 调对应动作工具。
//! Mock：同构 Rust MCP server，提供脚本化战斗序列用于无游戏演示。

#![forbid(unsafe_code)]

pub mod client;
pub mod mock;

pub use client::McpClient;
pub use mock::MockGame;
