//! LLM 类型：消息、响应、用量、流式事件。

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
    /// assistant 消息携带的工具调用（OpenAI 格式）；其他角色为 None。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Value>,
    /// role=tool 时对应的 tool_call id。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl ChatMessage {
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: "system".into(),
            content: content.into(),
            ..Default::default()
        }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: "user".into(),
            content: content.into(),
            ..Default::default()
        }
    }
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: "assistant".into(),
            content: content.into(),
            ..Default::default()
        }
    }
    /// assistant 消息：文本 + 工具调用（id/name/arguments 三元组列表）。
    pub fn assistant_tool_calls(
        content: impl Into<String>,
        calls: &[(String, String, String)],
    ) -> Self {
        let tool_calls: Vec<Value> = calls
            .iter()
            .map(|(id, name, args)| {
                json!({
                    "id": id,
                    "type": "function",
                    "function": { "name": name, "arguments": args }
                })
            })
            .collect();
        Self {
            role: "assistant".into(),
            content: content.into(),
            tool_calls: Some(Value::Array(tool_calls)),
            tool_call_id: None,
        }
    }
    /// 工具结果消息（role=tool，对应一次 tool_call）。
    pub fn tool(call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            role: "tool".into(),
            content: content.into(),
            tool_calls: None,
            tool_call_id: Some(call_id.into()),
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
