//! 各 `state_type` 负载结构（对齐 `STS2MCP/docs/raw-full.md`）。
//! 未识别字段一律 `#[serde(flatten)] extra` 兜底。

use crate::combat::{Card, Keyword};
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---- hand_select ----
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HandSelect {
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub cards: Vec<Card>,
    #[serde(default)]
    pub selected_cards: Vec<Value>,
    #[serde(default)]
    pub can_confirm: Option<bool>,
    #[serde(flatten)]
    pub extra: Value,
}

// ---- rewards ----
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Rewards {
    #[serde(default)]
    pub items: Vec<RewardItem>,
    #[serde(default)]
    pub can_proceed: Option<bool>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RewardItem {
    #[serde(default)]
    pub index: Option<u32>,
    /// gold/potion/relic/card/special_card/card_removal
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub gold_amount: Option<i64>,
    #[serde(default)]
    pub potion_id: Option<String>,
    #[serde(default)]
    pub potion_name: Option<String>,
    #[serde(flatten)]
    pub extra: Value,
}

// ---- card_reward ----
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CardReward {
    #[serde(default)]
    pub cards: Vec<Card>,
    #[serde(default)]
    pub can_skip: Option<bool>,
    #[serde(flatten)]
    pub extra: Value,
}

// ---- map ----
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MapState {
    #[serde(default)]
    pub current_position: Option<MapCoord>,
    #[serde(default)]
    pub visited: Vec<MapCoord>,
    #[serde(default)]
    pub next_options: Vec<MapNextOption>,
    #[serde(default)]
    pub nodes: Vec<MapDagNode>,
    #[serde(default)]
    pub boss: Option<BossRef>,
    #[serde(default)]
    pub bosses: Vec<BossRef>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MapCoord {
    #[serde(default)]
    pub col: Option<i32>,
    #[serde(default)]
    pub row: Option<i32>,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MapNextOption {
    #[serde(default)]
    pub index: Option<u32>,
    #[serde(default)]
    pub col: Option<i32>,
    #[serde(default)]
    pub row: Option<i32>,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub leads_to: Vec<MapCoord>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MapDagNode {
    #[serde(default)]
    pub col: Option<i32>,
    #[serde(default)]
    pub row: Option<i32>,
    #[serde(default, rename = "type")]
    pub kind: Option<String>,
    #[serde(default)]
    pub children: Vec<Vec<i32>>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BossRef {
    #[serde(default)]
    pub col: Option<i32>,
    #[serde(default)]
    pub row: Option<i32>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(flatten)]
    pub extra: Value,
}

// ---- event ----
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EventState {
    #[serde(default)]
    pub event_id: Option<String>,
    #[serde(default)]
    pub event_name: Option<String>,
    #[serde(default)]
    pub is_ancient: Option<bool>,
    #[serde(default)]
    pub in_dialogue: Option<bool>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub options: Vec<EventOption>,
    /// 多人：是否共享事件。
    #[serde(default)]
    pub is_shared: Option<bool>,
    #[serde(default)]
    pub votes: Vec<Value>,
    #[serde(default)]
    pub all_voted: Option<bool>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EventOption {
    #[serde(default)]
    pub index: Option<u32>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub is_locked: Option<bool>,
    #[serde(default)]
    pub is_proceed: Option<bool>,
    #[serde(default)]
    pub was_chosen: Option<bool>,
    #[serde(default)]
    pub relic_name: Option<String>,
    #[serde(default)]
    pub relic_description: Option<String>,
    #[serde(default)]
    pub keywords: Vec<Keyword>,
    #[serde(flatten)]
    pub extra: Value,
}

// ---- rest_site ----
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RestSite {
    #[serde(default)]
    pub options: Vec<RestOption>,
    #[serde(default)]
    pub can_proceed: Option<bool>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RestOption {
    #[serde(default)]
    pub index: Option<u32>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub is_enabled: Option<bool>,
    #[serde(flatten)]
    pub extra: Value,
}

// ---- shop / fake_merchant ----
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Shop {
    #[serde(default)]
    pub items: Vec<ShopItem>,
    #[serde(default)]
    pub can_proceed: Option<bool>,
    #[serde(default)]
    pub error: Option<String>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShopItem {
    #[serde(default)]
    pub index: Option<u32>,
    #[serde(default)]
    pub category: Option<String>,
    #[serde(default)]
    pub price: Option<i64>,
    /// fake_merchant 用 cost 而非 price。
    #[serde(default)]
    pub cost: Option<i64>,
    #[serde(default)]
    pub is_stocked: Option<bool>,
    #[serde(default)]
    pub can_afford: Option<bool>,
    #[serde(default)]
    pub on_sale: Option<bool>,
    #[serde(default)]
    pub card_id: Option<String>,
    #[serde(default)]
    pub card_name: Option<String>,
    #[serde(default)]
    pub card_type: Option<String>,
    #[serde(default)]
    pub card_cost: Option<String>,
    #[serde(default)]
    pub card_star_cost: Option<String>,
    #[serde(default)]
    pub card_rarity: Option<String>,
    #[serde(default)]
    pub card_description: Option<String>,
    #[serde(default)]
    pub relic_id: Option<String>,
    #[serde(default)]
    pub relic_name: Option<String>,
    #[serde(default)]
    pub relic_description: Option<String>,
    #[serde(default)]
    pub potion_id: Option<String>,
    #[serde(default)]
    pub potion_name: Option<String>,
    #[serde(default)]
    pub potion_description: Option<String>,
    #[serde(default)]
    pub keywords: Vec<Keyword>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FakeMerchant {
    #[serde(default)]
    pub event_id: Option<String>,
    #[serde(default)]
    pub event_name: Option<String>,
    #[serde(default)]
    pub started_fight: Option<bool>,
    #[serde(default)]
    pub shop: Option<Shop>,
    #[serde(flatten)]
    pub extra: Value,
}

// ---- treasure ----
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Treasure {
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub relics: Vec<TreasureRelic>,
    #[serde(default)]
    pub can_proceed: Option<bool>,
    /// 多人：竞标阶段。
    #[serde(default)]
    pub is_bidding_phase: Option<bool>,
    #[serde(default)]
    pub bids: Vec<Value>,
    #[serde(default)]
    pub all_bid: Option<bool>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TreasureRelic {
    #[serde(default)]
    pub index: Option<u32>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub rarity: Option<String>,
    #[serde(default)]
    pub keywords: Vec<Keyword>,
    #[serde(flatten)]
    pub extra: Value,
}

// ---- card_select ----
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CardSelect {
    #[serde(default)]
    pub screen_type: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub cards: Vec<Card>,
    #[serde(default)]
    pub preview_showing: Option<bool>,
    #[serde(default)]
    pub can_confirm: Option<bool>,
    #[serde(default)]
    pub can_cancel: Option<bool>,
    #[serde(default)]
    pub can_skip: Option<bool>,
    #[serde(flatten)]
    pub extra: Value,
}

// ---- bundle_select ----
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BundleSelect {
    #[serde(default)]
    pub screen_type: Option<String>,
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub bundles: Vec<Bundle>,
    #[serde(default)]
    pub preview_showing: Option<bool>,
    #[serde(default)]
    pub preview_cards: Vec<Card>,
    #[serde(default)]
    pub can_cancel: Option<bool>,
    #[serde(default)]
    pub can_confirm: Option<bool>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Bundle {
    #[serde(default)]
    pub index: Option<u32>,
    #[serde(default)]
    pub card_count: Option<u32>,
    #[serde(default)]
    pub cards: Vec<Card>,
    #[serde(flatten)]
    pub extra: Value,
}

// ---- relic_select ----
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RelicSelect {
    #[serde(default)]
    pub prompt: Option<String>,
    #[serde(default)]
    pub relics: Vec<RelicChoice>,
    #[serde(default)]
    pub can_skip: Option<bool>,
    #[serde(flatten)]
    pub extra: Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RelicChoice {
    #[serde(default)]
    pub index: Option<u32>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub rarity: Option<String>,
    #[serde(default)]
    pub keywords: Vec<Keyword>,
    #[serde(flatten)]
    pub extra: Value,
}

// ---- crystal_sphere ----
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CrystalSphere {
    #[serde(default)]
    pub instructions_title: Option<String>,
    #[serde(default)]
    pub instructions_description: Option<String>,
    #[serde(default)]
    pub grid_width: Option<i32>,
    #[serde(default)]
    pub grid_height: Option<i32>,
    #[serde(default)]
    pub cells: Vec<Value>,
    #[serde(default)]
    pub clickable_cells: Vec<Value>,
    #[serde(default)]
    pub revealed_items: Vec<Value>,
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub can_use_big_tool: Option<bool>,
    #[serde(default)]
    pub can_use_small_tool: Option<bool>,
    #[serde(default)]
    pub divinations_left_text: Option<String>,
    #[serde(default)]
    pub can_proceed: Option<bool>,
    #[serde(flatten)]
    pub extra: Value,
}

// ---- game_over ----
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GameOver {
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub options: Vec<Value>,
    #[serde(flatten)]
    pub extra: Value,
}

// ---- overlay ----
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Overlay {
    #[serde(default)]
    pub screen_type: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(flatten)]
    pub extra: Value,
}
