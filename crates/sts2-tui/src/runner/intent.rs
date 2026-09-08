//! 用户意图：关键词分类 + 各意图的处理（打断/拒绝/对话）。
//!
//! 自主模式的开启/关闭不由这里判定——用户说"自己打"/"执行"等一律走 Chat，
//! 由决策 LLM 理解后输出 `ACTION: auto_start | task=...` / `ACTION: auto_stop`
//! 请求 Rust 内核切换（见 backend.rs 的拦截与门禁）。
//! 唯一例外：用户喊"停/打断"直接在 Rust 侧关闭自主模式（不经过 LLM，更可靠）。

use std::sync::Arc;

use tokio::sync::{mpsc, Mutex};

use sts2_agent::decide;
use sts2_core::{Config, GameState};
use sts2_llm::{BudgetGuard, LlmClient};
use sts2_mcp::McpClient;

use crate::app::{AppState, Mode, MsgRole};

use super::stream::{abort_current_llm, start_decision};

/// 用户意图。
#[derive(Debug, Clone)]
pub(super) enum UserIntent {
    Reject,
    Interrupt,
    Quit,
    Chat(String),
}

/// 关键词意图分类（无 LLM，省一次调用）。
/// 未命中关键词的输入归 Chat，由决策 LLM 理解并决定是否发起 auto_start。
pub(super) fn parse_intent(text: &str) -> UserIntent {
    let lower = text.to_lowercase();
    let trimmed = text.trim();

    if trimmed.contains("退出") || lower == "quit" || lower == "exit" {
        return UserIntent::Quit;
    }
    if trimmed == "打断" || trimmed == "停" || lower == "stop" || lower == "interrupt" {
        return UserIntent::Interrupt;
    }
    if trimmed == "不"
        || trimmed == "换一个"
        || trimmed == "不要"
        || trimmed == "拒绝"
        || lower == "no"
        || lower == "n"
        || lower == "reject"
    {
        return UserIntent::Reject;
    }
    UserIntent::Chat(text.to_string())
}

/// 重取最新游戏状态并发起决策。
/// Reject 语义下取状态失败需要中止（返回 Err），其余调用方忽略错误按默认状态继续。
#[allow(clippy::too_many_arguments, clippy::ptr_arg)]
async fn refresh_and_decide(
    state: &mut AppState,
    mode: &mut Mode,
    full_text: &mut String,
    config: &Config,
    llm: &LlmClient,
    mcp: &Arc<Mutex<McpClient>>,
    bt_tx: &mpsc::UnboundedSender<super::Backend>,
    history: &mut Vec<decide::ChatTurn>,
    user_msg: Option<&str>,
    zh: bool,
) -> anyhow::Result<()> {
    *mode = Mode::Streaming;
    state.streaming_text.clear();
    full_text.clear();

    let sj = mcp.lock().await.get_game_state("json").await?;
    let gs: GameState = serde_json::from_str(&sj).unwrap_or_default();
    state.last_state_json = sj.clone();
    state.game_state = gs.clone();
    start_decision(&gs, &sj, config, llm, bt_tx, history, user_msg, zh, state);
    Ok(())
}

/// 处理用户意图。
#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_user_intent(
    intent: &UserIntent,
    text: &str,
    state: &mut AppState,
    pending_actions: &mut Vec<sts2_agent::parse::ParsedAction>,
    mode: &mut Mode,
    config: &Config,
    llm: &LlmClient,
    mcp: &Arc<Mutex<McpClient>>,
    bt_tx: &mpsc::UnboundedSender<super::Backend>,
    bt_rx: &mut mpsc::UnboundedReceiver<super::Backend>,
    history: &mut Vec<decide::ChatTurn>,
    budget: &mut BudgetGuard,
    zh: bool,
    full_text: &mut String,
) {
    // 新的用户意图：重置任务状态，清空残留动作队列
    // （否则打断自主模式后，队列中剩余动作仍会在 ExecDone 到达时继续执行）
    state.lookup_context.clear();
    state.lookup_rounds = 0;
    state.queued_auto_reply = None;
    state.pending_user_input = Some(text.to_string());
    pending_actions.clear();
    if !matches!(intent, UserIntent::Chat(_)) {
        // 非对话意图（打断/拒绝）：策略记忆随任务重置
        state.plan = None;
        state.recent_actions.clear();
    }

    match intent {
        UserIntent::Quit => {}
        UserIntent::Interrupt => {
            abort_current_llm(state, bt_rx, full_text);
            state.auto_mode = false;
            state.task = None;
            // 排空所有 stale 后台消息，防止旧的 StreamDone 触发执行
            while bt_rx.try_recv().is_ok() {}
            state.push_chat(MsgRole::System, "已打断，退出自主模式。".into());
            history.clear();
            state.streaming_text.clear();
            *mode = Mode::Idle;
            state.progress = None;
            full_text.clear();
        }
        UserIntent::Reject => {
            history.push(decide::ChatTurn::User(text.to_string()));
            state.pending_action = None;
            state.task = None;
            state.progress = Some("重新决策中…".into());
            if refresh_and_decide(
                state,
                mode,
                full_text,
                config,
                llm,
                mcp,
                bt_tx,
                history,
                Some(text),
                zh,
            )
            .await
            .is_err()
            {
                state.push_chat(MsgRole::System, "取状态失败".into());
                *mode = Mode::Idle;
            }
        }
        UserIntent::Chat(msg) => {
            // 对话/操作指令/自主请求：由决策 LLM 理解。
            // LLM 需要操作游戏时输出 ACTION: auto_start（内核拦截开启自主模式），
            // 非自主模式下的裸操作 ACTION 会被内核无条件否决。
            history.push(decide::ChatTurn::User(msg.clone()));
            state.task = None;
            state.progress = Some("LLM 回复中…".into());
            let _ = refresh_and_decide(
                state,
                mode,
                full_text,
                config,
                llm,
                mcp,
                bt_tx,
                history,
                Some(msg),
                zh,
            )
            .await;
        }
    }

    // 公共：检查预算
    if budget.is_over_budget() {
        state.finished = true;
        state.progress = Some(format!("预算超限: {}", budget.summary()));
        *mode = Mode::Idle;
    }
}
