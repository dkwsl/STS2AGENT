//! 用户意图：关键词分类 + 各意图的处理（确认/拒绝/打断/自主/对话）。

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
    Confirm,
    Reject,
    Interrupt,
    Quit,
    /// 进入自主模式：用户明确说了"自己打"等。
    AutoPlay,
    Chat(String),
}

/// 关键词意图分类（无 LLM，省一次调用）。
/// 未命中关键词的输入归 Chat，由决策 LLM 判断是否为操作指令（单次执行语义）。
pub(super) fn parse_intent(text: &str) -> UserIntent {
    let lower = text.to_lowercase();
    let trimmed = text.trim();

    if trimmed.contains("退出") || lower == "quit" || lower == "exit" {
        return UserIntent::Quit;
    }
    // 自主模式：只认「自己打」类，其余一律当对话
    if trimmed.contains("自己打") {
        return UserIntent::AutoPlay;
    }
    if trimmed == "打断" || trimmed == "停" || lower == "stop" || lower == "interrupt" {
        return UserIntent::Interrupt;
    }
    if trimmed == "执行"
        || trimmed == "继续"
        || trimmed == "好"
        || trimmed == "确认"
        || trimmed == "可以"
        || lower == "ok"
        || lower == "go"
        || lower == "yes"
        || trimmed.contains("这样做")
        || trimmed.contains("就这么做")
        || trimmed.contains("就这么打")
    {
        return UserIntent::Confirm;
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
    pending_actions: &mut Vec<String>,
    mode: &mut Mode,
    config: &Config,
    llm: &LlmClient,
    mcp: &Arc<Mutex<McpClient>>,
    bt_tx: &mpsc::UnboundedSender<super::Backend>,
    bt_rx: &mut mpsc::UnboundedReceiver<super::Backend>,
    history: &mut Vec<decide::ChatTurn>,
    budget: &mut BudgetGuard,
    zh: bool,
    max_turns: u32,
    full_text: &mut String,
) {
    // 新的用户意图：重置任务状态，清空残留动作队列
    // （否则打断自主模式后，队列中剩余动作仍会在 ExecDone 到达时继续执行）
    state.lookup_context.clear();
    state.lookup_rounds = 0;
    pending_actions.clear();

    match intent {
        UserIntent::Quit => {}
        UserIntent::Interrupt => {
            abort_current_llm(state, bt_rx, full_text);
            state.auto_mode = false;
            state.execute_actions = false;
            state.task = None;
            // 排空所有 stale 后台消息，防止旧的 StreamDone 触发执行
            while bt_rx.try_recv().is_ok() {}
            state.push_chat(MsgRole::System, "已打断，退出自主模式。".into());
            history.clear();
            state.streaming_text.clear();
            *mode = Mode::Idle;
            full_text.clear();
        }
        UserIntent::AutoPlay => {
            state.auto_mode = true;
            state.execute_actions = true;
            state.task = Some(text.to_string());
            history.push(decide::ChatTurn::User(text.to_string()));
            state.progress = Some("自主模式启动…".into());
            let _ = refresh_and_decide(
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
            .await;
        }
        UserIntent::Confirm => {
            // 确认执行：只执行本轮 ACTION，不进入持续自主模式
            history.push(decide::ChatTurn::User(text.to_string()));
            state.execute_actions = true; // 临时开启，执行完一轮后自动关闭
            state.task = Some(text.to_string());
            state.pending_action = None;
            state.progress = Some("LLM 处理中…".into());
            let _ = refresh_and_decide(
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
            .await;
        }
        UserIntent::Reject => {
            history.push(decide::ChatTurn::User(text.to_string()));
            state.pending_action = None;
            state.execute_actions = false;
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
            // 对话/操作指令：由决策 LLM 判断是否输出 ACTION。
            // execute_actions=true + 非 auto_mode = 单次执行语义（执行一轮即停），
            // 纯对话（无 ACTION）则不操作游戏。
            history.push(decide::ChatTurn::User(msg.clone()));
            state.execute_actions = true;
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

    // 公共：检查预算/轮数
    if budget.is_over_budget() {
        state.finished = true;
        state.progress = Some(format!("预算超限: {}", budget.summary()));
        *mode = Mode::Idle;
    }
    if state.current_turn >= max_turns && matches!(intent, UserIntent::Confirm) {
        state.finished = true;
        state.progress = Some(format!("已达最大轮数 {max_turns}"));
        *mode = Mode::Idle;
    }
}
