//! 知识库检索已拆分为独立 crate `sts2-knowledge`（可独立作为 skill/MCP server 使用）。
//! 此处 re-export 仓库内实际使用的符号，统一调用路径。

pub use sts2_knowledge::{context, lookup, wiki};

pub use sts2_knowledge::context::search_game_knowledge;
pub use sts2_knowledge::lookup::{lookup_query_smart, perform_lookup, search_wiki};
pub use sts2_knowledge::wiki::McpWikiSearcher;
