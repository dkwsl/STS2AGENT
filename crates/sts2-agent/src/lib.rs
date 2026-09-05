//! sts2-agent: 编排主控（见 PLAN.md §5.5 / §6）。
//!
//! 主循环：取状态(MCP) → 决策(DecisionEngine/LLM) → 拼 prompt → LLM(流式) → 产出解释。
//! 负责：会话/历史、预算累计与自动中断、CancellationToken 打断、进度事件总线。

#![forbid(unsafe_code)]

pub mod decide;
pub mod parse;
pub mod play;
pub mod storage;

use sts2_core::Config;

/// 加载配置：`config.toml` + `.env`（密钥优先环境变量）。
pub fn load_config() -> anyhow::Result<Config> {
    let _ = dotenvy::dotenv();
    let text = std::fs::read_to_string("config/config.toml")
        .or_else(|_| std::fs::read_to_string("config.toml"))?;
    let mut cfg: Config = toml::from_str(&text)?;
    if cfg.model.api_key.is_empty() {
        if let Ok(k) = std::env::var("STS2_OPENAI_API_KEY") {
            cfg.model.api_key = k;
        }
    }
    Ok(cfg)
}
