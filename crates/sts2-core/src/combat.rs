//! 战斗相关子结构（对齐 `STS2MCP/docs/raw-full.md`）。
//! Card/Power/Keyword/Enemy/Intent/Orb/Relic/Potion/Pet/Battle。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 回合归属：玩家回合 / 敌人回合；未识别值兜底为 `Unknown`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Turn {
    Player,
    Enemy,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Battle {
    #[serde(default)]
    pub round: Option<u32>,
    #[serde(default)]
    pub turn: Option<Turn>,
    #[serde(default)]
    pub is_play_phase: Option<bool>,
    #[serde(default)]
    pub enemies: Vec<Enemy>,
    /// 多人：全部玩家是否就绪。
    #[serde(default)]
    pub all_players_ready: Option<bool>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Enemy {
    pub entity_id: String,
    #[serde(default)]
    pub combat_id: Option<i64>,
    pub name: String,
    #[serde(default)]
    pub hp: i32,
    #[serde(default)]
    pub max_hp: i32,
    #[serde(default)]
    pub block: i32,
    #[serde(default)]
    pub status: Vec<Power>,
    #[serde(default)]
    pub intents: Vec<Intent>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Intent {
    /// Attack/Defend/Buff/Debuff/Sleep/…
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Card {
    #[serde(default)]
    pub index: Option<u32>,
    pub id: String,
    pub name: String,
    /// Attack/Skill/Power/Status/Curse
    #[serde(rename = "type")]
    pub kind: String,
    /// 能量费用，字符串（"X" 表示 X 费）。
    pub cost: String,
    #[serde(default)]
    pub star_cost: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub target_type: Option<String>,
    #[serde(default)]
    pub can_play: Option<bool>,
    #[serde(default)]
    pub unplayable_reason: Option<String>,
    #[serde(default)]
    pub is_upgraded: Option<bool>,
    #[serde(default)]
    pub rarity: Option<String>,
    #[serde(default)]
    pub keywords: Vec<Keyword>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PileCard {
    pub name: String,
    pub cost: String,
    #[serde(default)]
    pub star_cost: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Power {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub amount: Option<i64>,
    /// Buff / Debuff
    #[serde(default, rename = "type")]
    pub power_type: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub keywords: Vec<Keyword>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Keyword {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Orb {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub passive_val: Option<i64>,
    #[serde(default)]
    pub evoke_val: Option<i64>,
    #[serde(default)]
    pub keywords: Vec<Keyword>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Relic {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    /// null 或数字计数。
    #[serde(default)]
    pub counter: Option<i64>,
    #[serde(default)]
    pub keywords: Vec<Keyword>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Potion {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub slot: u32,
    #[serde(default)]
    pub can_use_in_combat: Option<bool>,
    #[serde(default)]
    pub target_type: Option<String>,
    #[serde(default)]
    pub keywords: Vec<Keyword>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Pet {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub alive: Option<bool>,
    #[serde(default)]
    pub hp: i32,
    #[serde(default)]
    pub max_hp: i32,
    #[serde(default)]
    pub block: i32,
    #[serde(default)]
    pub status: Vec<Power>,
    #[serde(flatten)]
    pub extra: Value,
}
