//! LLM 类型：消息、响应、用量、流式事件。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: content.into(),
        }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
        }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".into(),
            content: content.into(),
        }
    }
}

/// 一次 API 调用的 token 用量（从 API 响应 usage 字段取得）。
#[derive(Debug, Clone, Default)]
pub struct Usage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    /// prompt 命中缓存的部分（DeepSeek prompt_cache_hit_tokens /
    /// OpenAI prompt_tokens_details.cached_tokens）。仅统计展示，不计价。
    pub cached_tokens: u64,
}

/// LLM 发起的一次原生工具调用（OpenAI 兼容 tool_calls）。
#[derive(Debug, Clone, Default)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    /// 参数 JSON 字符串（可能为空对象）。
    pub arguments: String,
}

impl Usage {
    /// 按价格（每百万 token 美元）换算成本。
    pub fn cost(&self, price_in: f64, price_out: f64) -> f64 {
        self.prompt_tokens as f64 * price_in / 1_000_000.0
            + self.completion_tokens as f64 * price_out / 1_000_000.0
    }

    pub fn total(&self) -> u64 {
        self.prompt_tokens + self.completion_tokens
    }
}

/// 非流式 chat 响应。
#[derive(Debug, Clone)]
pub struct ChatResponse {
    pub content: String,
    /// 思考模式输出（DeepSeek reasoning_content / OpenAI reasoning）。
    pub reasoning: Option<String>,
    pub usage: Usage,
}

/// 流式 chat 推送的事件。
#[derive(Debug, Clone)]
pub enum StreamEvent {
    /// 内容增量。
    Delta(String),
    /// 思考增量（reasoning_content / reasoning）。
    Reasoning(String),
    /// 一次完整的工具调用（流结束时统一交付，已按 index 合并分片）。
    ToolCall(ToolCall),
    /// 最终用量（流结束时发送一次）。
    Usage(Usage),
    /// 流结束。
    Done,
    /// 流中错误。
    Error(String),
}
