//! STS2 知识库 skill crate：结构化游戏知识检索。
//!
//! 两个能力：
//! - [`lookup`]：按名称/内部 ID 主动查询（本地表格 → 显示名回退 → Wiki 兜底）
//! - [`context`]：按当前 GameState 自动注入相关表格行 + playbook + 未知手牌检测
//!
//! 数据目录（默认 `game-knowledge/`）由调用方注入——目录即知识包，换目录即换领域。
//! [`wiki::WikiSearcher`] 抽象了联网兜底通道（默认实现走 MCP `search_wiki`），
//! 本 crate 不直接依赖具体传输。

pub mod context;
pub mod lookup;
pub mod wiki;

pub use context::{find_unknown_hand_cards, search_game_knowledge};
pub use lookup::{lookup_query, lookup_query_smart, perform_lookup, search_wiki, LookupOutcome};
pub use wiki::{HttpWikiSearcher, McpWikiSearcher, WikiSearcher};
