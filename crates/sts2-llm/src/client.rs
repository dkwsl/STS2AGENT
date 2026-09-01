//! OpenAI 兼容 chat 客户端（见 PLAN.md §5.4）。
//!
//! 支持：流式 SSE + 非流式；从 usage 取 input/output token；思考模式
//! （DeepSeek reasoning_content / OpenAI reasoning）；预算守卫（R6）。

use anyhow::{Context, Result};
use futures_util::StreamExt;
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tracing::debug;

use crate::types::{ChatMessage, ChatResponse, StreamEvent, Usage};

pub struct LlmClient {
    http: reqwest::Client,
    endpoint: String,
    api_key: String,
    model: String,
    context_length: u32,
    thinking_mode: bool,
    price_in: f64,
    price_out: f64,
}

impl LlmClient {
    pub fn new(
        endpoint: &str,
        api_key: &str,
        model: &str,
        context_length: u32,
        thinking_mode: bool,
        price_in: f64,
        price_out: f64,
    ) -> Self {
        Self {
            http: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(120))
                .build()
                .expect("failed to build HTTP client"),
            endpoint: endpoint.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            model: model.to_string(),
            context_length,
            thinking_mode,
            price_in,
            price_out,
        }
    }

    pub fn from_config(config: &sts2_core::ModelConfig) -> Self {
        Self::new(
            &config.endpoint,
            &config.api_key,
            &config.model,
            config.context_length,
            config.thinking_mode,
            config.price_in,
            config.price_out,
        )
    }

    fn chat_url(&self) -> String {
        format!("{}/chat/completions", self.endpoint)
    }

    /// 非流式 chat。返回完整内容 + reasoning + usage。
    pub async fn chat(&self, messages: &[ChatMessage]) -> Result<ChatResponse> {
        let body = json!({
            "model": self.model,
            "messages": messages,
            "stream": false,
        });
        debug!("chat request to {} model={}", self.chat_url(), self.model);

        let resp = self
            .http
            .post(self.chat_url())
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await
            .context("failed to send chat request")?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            anyhow::bail!("API error {status}: {text}");
        }

        let v: Value = resp.json().await.context("failed to parse response")?;
        let content = v["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let reasoning = v["choices"][0]["message"]["reasoning_content"]
            .as_str()
            .or_else(|| v["choices"][0]["message"]["reasoning"].as_str())
            .map(|s| s.to_string());
        let usage = Usage {
            prompt_tokens: v["usage"]["prompt_tokens"].as_u64().unwrap_or(0),
            completion_tokens: v["usage"]["completion_tokens"].as_u64().unwrap_or(0),
        };

        Ok(ChatResponse {
            content,
            reasoning,
            usage,
        })
    }

    /// 流式 chat。返回 `UnboundedReceiver<StreamEvent>`，后台任务推送 Delta/Reasoning/Usage/Done。
    pub fn chat_stream(
        &self,
        messages: &[ChatMessage],
    ) -> Result<mpsc::UnboundedReceiver<StreamEvent>> {
        let body = json!({
            "model": self.model,
            "messages": messages,
            "stream": true,
            "stream_options": {"include_usage": true},
        });
        let url = self.chat_url();
        let key = self.api_key.clone();

        let (tx, rx) = mpsc::unbounded_channel();

        let http = self.http.clone();
        tokio::spawn(async move {
            let send = |ev: StreamEvent| {
                let _ = tx.send(ev);
            };

            let resp = match http.post(&url).bearer_auth(&key).json(&body).send().await {
                Ok(r) => r,
                Err(e) => {
                    send(StreamEvent::Error(format!("request failed: {e:#}")));
                    send(StreamEvent::Done);
                    return;
                }
            };

            if !resp.status().is_success() {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                send(StreamEvent::Error(format!("API error {status}: {text}")));
                send(StreamEvent::Done);
                return;
            }

            let mut stream = resp.bytes_stream();
            let mut buf = String::new();
            let mut usage = Usage::default();

            while let Some(chunk) = stream.next().await {
                let chunk = match chunk {
                    Ok(c) => c,
                    Err(e) => {
                        send(StreamEvent::Error(format!("stream error: {e}")));
                        break;
                    }
                };
                buf.push_str(&String::from_utf8_lossy(&chunk));

                while let Some(pos) = buf.find('\n') {
                    let line = buf[..pos].trim().to_string();
                    buf = buf[pos + 1..].to_string();

                    if !line.starts_with("data: ") {
                        continue;
                    }
                    let data = &line[6..];
                    if data == "[DONE]" {
                        send(StreamEvent::Usage(usage.clone()));
                        send(StreamEvent::Done);
                        return;
                    }
                    if let Ok(parsed) = serde_json::from_str::<Value>(data) {
                        // content delta
                        if let Some(content) = parsed["choices"][0]["delta"]["content"].as_str() {
                            send(StreamEvent::Delta(content.to_string()));
                        }
                        // reasoning delta (DeepSeek)
                        if let Some(r) = parsed["choices"][0]["delta"]["reasoning_content"]
                            .as_str()
                            .or_else(|| parsed["choices"][0]["delta"]["reasoning"].as_str())
                        {
                            send(StreamEvent::Reasoning(r.to_string()));
                        }
                        // usage (final chunk, may have empty choices)
                        if let Some(u) = parsed.get("usage") {
                            if !u.is_null() {
                                usage = Usage {
                                    prompt_tokens: u["prompt_tokens"].as_u64().unwrap_or(0),
                                    completion_tokens: u["completion_tokens"].as_u64().unwrap_or(0),
                                };
                            }
                        }
                    }
                }
            }
            // 流自然结束（无 [DONE]）
            send(StreamEvent::Usage(usage));
            send(StreamEvent::Done);
        });

        Ok(rx)
    }

    pub fn price_in(&self) -> f64 {
        self.price_in
    }
    pub fn price_out(&self) -> f64 {
        self.price_out
    }
    pub fn model(&self) -> &str {
        &self.model
    }
    pub fn context_length(&self) -> u32 {
        self.context_length
    }
    pub fn thinking_mode(&self) -> bool {
        self.thinking_mode
    }
}
