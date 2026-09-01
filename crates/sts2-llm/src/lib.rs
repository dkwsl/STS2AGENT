//! sts2-llm: OpenAI 兼容 chat 客户端（见 PLAN.md §5.4）。
//!
//! 流式响应（SSE）、从 usage 取 input/output token、按价格换算成本（R6）、
//! 预算守卫、思考模式（DeepSeek `reasoning_content` / OpenAI 推理模型）。

#![forbid(unsafe_code)]

pub mod budget;
pub mod client;
pub mod types;

pub use budget::BudgetGuard;
pub use client::LlmClient;
pub use types::{ChatMessage, ChatResponse, StreamEvent, Usage};
