//! sts2-core: 领域模型与配置类型。
//!
//! 对齐 STS2MCP 的真实游戏状态 JSON schema（见 PLAN.md §5.1 / §9）。
//! 提供 `GameState` 及其子结构、`Config`。

#![forbid(unsafe_code)]

pub mod combat;
pub mod config;
pub mod screens;
pub mod state;

pub use combat::*;
pub use config::*;
pub use screens::*;
pub use state::*;
