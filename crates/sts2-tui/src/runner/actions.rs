//! 动作执行层：反射动作（免 LLM）、后台 MCP 执行、知识库查询拦截。

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::sync::{mpsc, Mutex};

use sts2_core::{Config, GameState, StateType};
use sts2_mcp::McpClient;

use crate::app::{AppState, MsgRole};

use super::Backend;

/// 自主模式下的反射动作：无需 LLM 判断的机械操作（省 token）。
/// 只处理确定无歧义的操作，其余返回 None 交给 LLM。
pub(super) fn try_reflex_action(gs: &GameState) -> Option<(String, Value)> {
    match gs.state_type {
        // 奖励屏：从右到左逐个领取（避免索引漂移），领完推进
        StateType::Rewards => {
            if let Some(r) = &gs.rewards {
                if !r.items.is_empty() {
                    let idx = r.items.iter().filter_map(|i| i.index).max().unwrap_or(0);
                    return Some((
                        "rewards_claim".into(),
                        serde_json::json!({ "reward_index": idx }),
                    ));
                }
                if r.can_proceed.unwrap_or(false) {
                    return Some(("proceed_to_map".into(), serde_json::json!({})));
                }
            }
            None
        }
        // 卡牌选择：已选完进入预览/可确认状态 → 直接确认（否则会卡住）
        StateType::CardSelect => {
            if let Some(cs) = &gs.card_select {
                let confirmed_ready = cs.can_confirm.unwrap_or(false)
                    && (cs.preview_showing.unwrap_or(false) || cs.cards.is_empty());
                if confirmed_ready {
                    return Some(("deck_confirm_selection".into(), serde_json::json!({})));
                }
            }
            None
        }
        // 宝箱：唯一遗物直接拾取（非竞标阶段）
        StateType::Treasure => {
            if let Some(t) = &gs.treasure {
                if t.relics.len() == 1 && !t.is_bidding_phase.unwrap_or(false) {
                    let idx = t.relics[0].index.unwrap_or(0);
                    return Some((
                        "treasure_claim_relic".into(),
                        serde_json::json!({ "relic_index": idx }),
                    ));
                }
            }
            None
        }
        _ => None,
    }
}

/// 后台执行一个 MCP 动作（等 2 秒让游戏状态更新），结果经 ExecDone 回主循环。
pub(super) fn spawn_exec(
    mcp: &Arc<Mutex<McpClient>>,
    bt_tx: &mpsc::UnboundedSender<Backend>,
    tool: &str,
    args: Value,
) {
    let mcp2 = mcp.clone();
    let bt_tx2 = bt_tx.clone();
    let tool = tool.to_string();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let mut m = mcp2.lock().await;
        let result = m.call_tool(&tool, args).await;
        let (success, message) = match result {
            Ok(r) => (true, r.chars().take(120).collect()),
            Err(e) => (false, format!("{e:#}")),
        };
        let _ = bt_tx2.send(Backend::ExecDone { success, message });
    });
}

/// 拦截 LLM 的 lookup 动作：查知识库 → 结果累积进 lookup_context → 复用原状态重新决策。
/// 返回 true 表示已拦截并重新发起决策；false 表示查询无效/超限（调用方按无动作处理）。
pub(super) fn handle_lookup(query: &str, state: &mut AppState, config: &Config) -> bool {
    if query.is_empty() || state.lookup_rounds >= 3 {
        state.push_chat(
            MsgRole::System,
            "查询无效或已达本次任务上限（3 次），基于现有信息决策。".into(),
        );
        return false;
    }
    state.lookup_rounds += 1;
    state.push_chat(MsgRole::System, format!("📖 查询知识库: {query}…"));
    let outcome = sts2_agent::lookup::perform_lookup(
        query,
        &state.game_state,
        &config.storage.game_knowledge_dir,
    );
    if outcome.result.is_empty() {
        state
            .lookup_context
            .push_str(&format!("[查询 {query}]: 知识库无记录\n"));
        state.push_chat(MsgRole::System, format!("未找到 {query}"));
    } else {
        if outcome.used != query {
            state.push_chat(
                MsgRole::System,
                format!("（显示名转内部 ID: {}）", outcome.used),
            );
        }
        state.lookup_context.push_str(&format!(
            "[查询 {query} → {}]:\n{}\n",
            outcome.used, outcome.result
        ));
        state.push_chat(
            MsgRole::System,
            format!(
                "✅ 已查询 {}（{} 字），继续分析…",
                outcome.used,
                outcome.result.chars().count()
            ),
        );
    }
    true
}
