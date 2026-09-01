//! 顶层游戏状态，对齐 `STS2MCP` 的 `get_game_state(format="json")` 响应。
//! 宽进严出：命名字段捕获已知负载，`#[serde(flatten)] extra` 兜底未识别字段。

use crate::combat::{Battle, Card, Orb, Pet, PileCard, Potion, Power, Relic};
use crate::screens::{
    BundleSelect, CardReward, CardSelect, CrystalSphere, EventState, FakeMerchant, GameOver,
    HandSelect, MapState, Overlay, RelicSelect, RestSite, Rewards, Shop, Treasure,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GameState {
    pub state_type: StateType,
    #[serde(default)]
    pub run: Option<RunInfo>,
    #[serde(default)]
    pub player: Option<Player>,

    // 菜单/character_select/game_over 等的顶层字段：
    #[serde(default)]
    pub menu_screen: Option<String>,
    /// 异构：主菜单为字符串数组，多人 lobby 为对象数组，故用 Value。
    #[serde(default)]
    pub options: Option<Vec<Value>>,
    #[serde(default)]
    pub message: Option<String>,

    // 各 state_type 嵌套负载：
    #[serde(default)]
    pub battle: Option<Battle>,
    #[serde(default)]
    pub hand_select: Option<HandSelect>,
    #[serde(default)]
    pub rewards: Option<Rewards>,
    #[serde(default)]
    pub card_reward: Option<CardReward>,
    #[serde(default)]
    pub map: Option<MapState>,
    #[serde(default)]
    pub event: Option<EventState>,
    #[serde(default)]
    pub rest_site: Option<RestSite>,
    #[serde(default)]
    pub shop: Option<Shop>,
    #[serde(default)]
    pub fake_merchant: Option<FakeMerchant>,
    #[serde(default)]
    pub treasure: Option<Treasure>,
    #[serde(default)]
    pub card_select: Option<CardSelect>,
    #[serde(default)]
    pub bundle_select: Option<BundleSelect>,
    #[serde(default)]
    pub relic_select: Option<RelicSelect>,
    #[serde(default)]
    pub crystal_sphere: Option<CrystalSphere>,
    #[serde(default)]
    pub game_over: Option<GameOver>,
    #[serde(default)]
    pub overlay: Option<Overlay>,

    /// 未识别顶层字段兜底，供 LLM 上下文与调试。
    #[serde(flatten)]
    pub extra: Value,
}

/// 屏幕类型（对齐上游 `state_type`，全小写下划线）；未识别值归 `Unknown`。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StateType {
    #[default]
    Menu,
    Monster,
    Elite,
    Boss,
    HandSelect,
    Rewards,
    CardReward,
    Map,
    Event,
    RestSite,
    Shop,
    FakeMerchant,
    Treasure,
    CardSelect,
    BundleSelect,
    RelicSelect,
    CrystalSphere,
    GameOver,
    Overlay,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RunInfo {
    #[serde(default)]
    pub act: u32,
    #[serde(default)]
    pub floor: u32,
    #[serde(default)]
    pub ascension: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Player {
    #[serde(default)]
    pub character: String,
    #[serde(default)]
    pub hp: i32,
    #[serde(default)]
    pub max_hp: i32,
    #[serde(default)]
    pub block: i32,
    #[serde(default)]
    pub gold: i32,

    // 战斗字段（仅战斗中出现）：
    #[serde(default)]
    pub energy: Option<i32>,
    #[serde(default)]
    pub max_energy: Option<i32>,
    #[serde(default)]
    pub stars: Option<i32>,
    #[serde(default)]
    pub hand: Option<Vec<Card>>,
    #[serde(default)]
    pub draw_pile_count: Option<u32>,
    #[serde(default)]
    pub discard_pile_count: Option<u32>,
    #[serde(default)]
    pub exhaust_pile_count: Option<u32>,
    #[serde(default)]
    pub draw_pile: Option<Vec<PileCard>>,
    #[serde(default)]
    pub discard_pile: Option<Vec<PileCard>>,
    #[serde(default)]
    pub exhaust_pile: Option<Vec<PileCard>>,
    #[serde(default)]
    pub orbs: Option<Vec<Orb>>,
    #[serde(default)]
    pub orb_slots: Option<u32>,
    #[serde(default)]
    pub orb_empty_slots: Option<u32>,
    #[serde(default)]
    pub pets: Option<Vec<Pet>>,

    // 常驻字段：
    #[serde(default)]
    pub status: Vec<Power>,
    #[serde(default)]
    pub relics: Vec<Relic>,
    #[serde(default)]
    pub potions: Vec<Potion>,
    #[serde(default)]
    pub max_potion_slots: u32,

    #[serde(flatten)]
    pub extra: Value,
}
