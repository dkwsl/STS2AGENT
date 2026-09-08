//! 知识库检索已拆分为独立 crate `sts2-knowledge`（可独立作为 skill/MCP server 使用）。
//! 此处 re-export 保持既有调用路径兼容。

pub use sts2_knowledge::{context, lookup, wiki};

// 以下 re-export 保持 `sts2_agent::lookup::xxx` 等旧路径可用
pub use sts2_knowledge::context::{find_unknown_hand_cards, search_game_knowledge};
pub use sts2_knowledge::lookup::{
    lookup_query, lookup_query_smart, perform_lookup, search_wiki, LookupOutcome,
};
pub use sts2_knowledge::wiki::{McpWikiSearcher, WikiSearcher};
