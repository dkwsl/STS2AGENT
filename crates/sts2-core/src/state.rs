//! 游戏状态领域模型骨架（P1 将按 STS2MCP `raw-full.md` 填全完整 schema）。
//! 设计原则：宽进严出——`#[serde(flatten)] extra` 保留未识别字段，供 LLM 上下文与调试。

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// 顶层游戏状态，对齐 `get_game_state(format="json")` 响应。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GameState {
    pub state_type: StateType,
    #[serde(default)]
    pub run: Option<RunInfo>,
    #[serde(default)]
    pub player: Option<Player>,
    /// 保留未识别字段。
    #[serde(flatten)]
    pub extra: Value,
}

/// 当前屏幕类型（对齐上游 `state_type`，全小写下划线）。
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
    /// 未识别状态兜底。
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

/// 玩家状态骨架；战斗字段（energy/hand/piles/orbs/…）在 P1 补全并标 `Option`。
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
    #[serde(flatten)]
    pub extra: Value,
}
