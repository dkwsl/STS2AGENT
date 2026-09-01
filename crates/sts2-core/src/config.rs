//! 配置类型（对齐 `config.example.toml`）。
//! 密钥仅从环境变量或 `secret/` 读取，禁入配置文件与 git。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub model: ModelConfig,
    pub mcp: McpConfig,
    pub decision: DecisionConfig,
    pub budget: BudgetConfig,
    pub storage: StorageConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    pub endpoint: String,
    #[serde(default)]
    pub api_key: String,
    pub model: String,
    #[serde(default)]
    pub context_length: u32,
    #[serde(default)]
    pub thinking_mode: bool,
    #[serde(default)]
    pub price_in: f64,
    #[serde(default)]
    pub price_out: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpConfig {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionConfig {
    #[serde(default = "default_engine")]
    pub engine: String,
}

fn default_engine() -> String {
    "rule_based".to_string()
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BudgetConfig {
    #[serde(default)]
    pub token_limit: u64,
    #[serde(default)]
    pub cost_limit_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    #[serde(default = "default_sessions_dir")]
    pub sessions_dir: String,
    #[serde(default = "default_logs_dir")]
    pub logs_dir: String,
}

fn default_sessions_dir() -> String {
    "data/sessions".to_string()
}

fn default_logs_dir() -> String {
    "data/logs".to_string()
}
