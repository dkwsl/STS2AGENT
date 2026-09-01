//! sts2-decision: 可插拔决策引擎（见 PLAN.md §5.3 / §8）。
//!
//! `DecisionEngine` trait + 内置 `RuleBasedEngine`（规则启发式）。
//! 决策只产出结构化 `ActionRecommendation`；自然语言解释由 LLM 完成。

#![forbid(unsafe_code)]

use sts2_core::{ActionRecommendation, GameState};

/// 决策引擎接口。通过 `dyn DecisionEngine` + 配置实现“可更改决策工具”。
pub trait DecisionEngine: Send + Sync {
    /// 依据当前游戏状态产出按优先级排序的行动建议。
    fn decide(&self, state: &GameState) -> Vec<ActionRecommendation>;
}

/// 规则启发式参考实现（P3 填全规则库）。
pub struct RuleBasedEngine;

impl DecisionEngine for RuleBasedEngine {
    fn decide(&self, _state: &GameState) -> Vec<ActionRecommendation> {
        Vec::new()
    }
}
