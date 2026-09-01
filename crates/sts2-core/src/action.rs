//! 行动类型，对齐 STS2MCP 动作工具入参（见 PLAN.md §9.3）。
//! `Action` 以 `#[serde(tag = "action")]` 序列化，可直接作为 MCP/HTTP 动作请求体。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum Action {
    PlayCard {
        card_index: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        target: Option<String>,
    },
    EndTurn,
    UsePotion {
        slot: u32,
        #[serde(skip_serializing_if = "Option::is_none")]
        target: Option<String>,
    },
    DiscardPotion {
        slot: u32,
    },
    ChooseMapNode {
        index: u32,
    },
    ChooseEventOption {
        index: u32,
    },
    AdvanceDialogue,
    ChooseRestOption {
        index: u32,
    },
    ShopPurchase {
        index: u32,
    },
    ClaimReward {
        index: u32,
    },
    SelectCardReward {
        card_index: u32,
    },
    SkipCardReward,
    Proceed,
    DeckSelectCard {
        index: u32,
    },
    DeckConfirmSelection,
    DeckCancelSelection,
    MenuSelect {
        option: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        seed: Option<String>,
    },
}

/// 决策引擎产出的行动建议（带优先级与理由，供 LLM 解释）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionRecommendation {
    pub action: Action,
    /// 优先级，越大越优先。
    pub priority: i32,
    /// 规则理由 hint，喂给 LLM 生成自然语言解释。
    pub reason: String,
}
