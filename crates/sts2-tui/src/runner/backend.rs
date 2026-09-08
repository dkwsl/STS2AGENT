//! 后台消息处理：LLM 流结束、动作执行完成、游戏状态变化的编排逻辑。

use std::sync::Arc;

use tokio::sync::{mpsc, Mutex};

use sts2_agent::storage::{Session, SessionStore};
use sts2_agent::{decide, parse};
use sts2_core::{Config, GameState, StateType};
use sts2_llm::{BudgetGuard, LlmClient};
use sts2_mcp::McpClient;

use crate::app::{AppState, Mode, MsgRole};

use super::actions::{handle_lookup, spawn_exec, try_reflex_action};
use super::intent::{handle_user_intent, UserIntent};
use super::stream::start_decision;
use super::Backend;

/// 处理一条后台消息。返回 true = 应退出程序。
#[allow(clippy::too_many_arguments)]
pub(super) async fn handle_backend_msg(
    msg: Backend,
    state: &mut AppState,
    mode: &mut Mode,
    full_text: &mut String,
    config: &Config,
    llm: &LlmClient,
    mcp: &Arc<Mutex<McpClient>>,
    bt_tx: &mpsc::UnboundedSender<Backend>,
    bt_rx: &mut mpsc::UnboundedReceiver<Backend>,
    history: &mut Vec<decide::ChatTurn>,
    budget: &mut BudgetGuard,
    session: &mut Session,
    store: &SessionStore,
    pending_actions: &mut Vec<parse::ParsedAction>,
    zh: bool,
    max_turns: u32,
) -> bool {
    match msg {
        Backend::Delta(t) => {
            state.streaming_text.push_str(&t);
            full_text.push_str(&t);
        }
        Backend::Reasoning(t) => {
            if state.show_thinking {
                state.reasoning_text.push_str(&t);
            }
        }
        Backend::Usage(u) => {
            state.last_usage = u.clone();
            budget.record(&u, config.model.price_in, config.model.price_out);
            state.total_input = budget.total_input();
            state.total_output = budget.total_output();
            state.total_cost = budget.total_cost();
        }
        Backend::IntentReady { text, intent } => {
            state.progress = None;
            if matches!(intent, UserIntent::Quit) {
                return true;
            }
            handle_user_intent(
                &intent,
                &text,
                state,
                pending_actions,
                mode,
                config,
                llm,
                mcp,
                bt_tx,
                bt_rx,
                history,
                budget,
                zh,
                full_text,
            )
            .await;
        }
        Backend::StreamDone { tool_calls } => {
            on_stream_done(
                state,
                mode,
                full_text,
                config,
                llm,
                mcp,
                bt_tx,
                history,
                pending_actions,
                tool_calls,
                zh,
                budget,
                session,
                store,
            )
            .await;
        }
        Backend::StreamError(e) => {
            state.push_chat(MsgRole::System, format!("LLM 错误: {e}"));
            state.streaming_text.clear();
            *mode = Mode::Idle;
            state.progress = None;
            full_text.clear();
        }
        Backend::ExecDone { success, message } => {
            on_exec_done(
                state,
                mode,
                mcp,
                bt_tx,
                budget,
                session,
                store,
                pending_actions,
                message,
                success,
            )
            .await;
        }
        Backend::StateReady(sj) => {
            on_state_ready(
                state, mode, config, llm, mcp, bt_tx, history, full_text, zh, max_turns, sj,
            )
            .await;
        }
        Backend::Error(e) => {
            state.push_chat(MsgRole::System, e);
            *mode = Mode::Idle;
            state.progress = None;
        }
    }
    false
}

/// 用户对自主模式开启请求的答复（y/n）。
/// 同意：开启自主模式并进入稳定等待→自主循环；拒绝：驳回本次请求，回 Idle。
pub(super) fn resolve_auto_start(
    confirm: bool,
    state: &mut AppState,
    mode: &mut Mode,
    full_text: &mut String,
    mcp: &Arc<Mutex<McpClient>>,
    bt_tx: &mpsc::UnboundedSender<Backend>,
) {
    let task = state.pending_auto_start.take().unwrap_or_default();
    full_text.clear();
    if confirm {
        state.auto_mode = true;
        state.no_action_streak = 0;
        state.task = if task.is_empty() { None } else { Some(task) };
        state.push_chat(MsgRole::System, "🤖 自主模式开启。".into());
        *mode = Mode::FetchingState;
        state.progress = Some("等待状态稳定…".into());
        spawn_state_stabilize(mcp, bt_tx);
    } else {
        state.push_chat(MsgRole::System, "⛔ 已驳回本次自主模式请求。".into());
        *mode = Mode::Idle;
        state.progress = None;
    }
}

/// LLM 流结束：校验状态一致性 → 处理输出（NOTE/对话文本）→ 拦截 lookup / 执行 ACTION。
#[allow(clippy::too_many_arguments)]
async fn on_stream_done(
    state: &mut AppState,
    mode: &mut Mode,
    full_text: &mut String,
    config: &Config,
    llm: &LlmClient,
    mcp: &Arc<Mutex<McpClient>>,
    bt_tx: &mpsc::UnboundedSender<Backend>,
    history: &mut Vec<decide::ChatTurn>,
    pending_actions: &mut Vec<parse::ParsedAction>,
    tool_calls: Vec<sts2_llm::ToolCall>,
    zh: bool,
    budget: &mut BudgetGuard,
    session: &mut Session,
    store: &SessionStore,
) {
    // 校验：当前状态与 LLM 分析时是否一致（用户中间手动操作过则作废本次决策）
    let current_sj = mcp
        .lock()
        .await
        .get_game_state("json")
        .await
        .unwrap_or_default();
    let state_valid = !current_sj.is_empty() && current_sj == state.decision_state_json;

    if !state_valid {
        return on_stale_decision(state, mode, full_text, mcp, bt_tx, current_sj);
    }

    // ---- 状态一致，正常处理 LLM 输出 ----

    // 1. 提取 NOTE 行（LLM 自主经验笔记）并追加到 session 笔记文件
    let notes = parse::extract_notes(full_text);
    if !notes.is_empty() {
        append_session_notes(state, config, &notes);
    }

    // 2. 动作来源：原生 tool_calls 优先（与文本分离，不依赖格式解析）；
    //    无 tool_calls 时回退解析文本 ACTION 行（供应商不支持 tools 时兜底）
    let mut actions: Vec<parse::ParsedAction> = Vec::new();
    for tc in &tool_calls {
        match parse::parse_tool_call(&tc.name, &tc.arguments) {
            Ok(a) => actions.push(a),
            Err(e) => state.push_chat(MsgRole::System, format!("工具调用解析失败: {e:#}")),
        }
    }
    let action_lines: Vec<String> = full_text
        .lines()
        .filter(|l| parse::is_action_line(l))
        .map(|l| l.to_string())
        .collect();
    if actions.is_empty() {
        for l in &action_lines {
            if let Ok(a) = parse::parse_action(l) {
                actions.push(a);
            }
        }
    }
    let chat_text: String = full_text
        .lines()
        .filter(|l| !parse::is_action_line(l) && !parse::is_note_line(l))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    // 固化本轮思考过程（时间顺序：思考先生成，排在分析之前）
    if !state.reasoning_text.is_empty() {
        let t = std::mem::take(&mut state.reasoning_text);
        state.push_chat(MsgRole::Thinking, t);
    }
    if !chat_text.is_empty() {
        state.push_chat(MsgRole::Agent, chat_text.clone());
        history.push(decide::ChatTurn::Assistant(chat_text.clone()));
    }
    state.streaming_text.clear();

    // 记录本轮到 session（R5：TUI 会话也保存对话/动作/用量；result 由 ExecDone 回填）
    session.turns.push(sts2_agent::storage::TurnRecord {
        turn: session.turns.len() as u32 + 1,
        state_summary: crate::app::state_summary(&state.game_state),
        state_json: state.decision_state_json.clone(),
        agent_text: chat_text,
        action: actions.first().map(|a| format!("{} {}", a.tool, a.args)),
        result: None,
        success: false,
        user_input: state.pending_user_input.take(),
        input_tokens: state.last_usage.prompt_tokens,
        output_tokens: state.last_usage.completion_tokens,
    });
    session.total_input = budget.total_input();
    session.total_output = budget.total_output();
    session.total_cost = budget.total_cost();
    let _ = store.save(session);

    // 3. 处理动作：lookup / auto_start / auto_stop 拦截，其余按自主模式门禁
    if let Some(action) = actions.first().cloned() {
        // 知识库查询：拦截，不发给游戏，查完注入上下文重新决策
        if action.tool == "lookup" {
            let query = action
                .args
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let ok = handle_lookup(&query, state, config, mcp).await;
            full_text.clear();
            pending_actions.clear();
            *mode = Mode::Streaming;
            state.progress = Some("结合查询结果分析…".into());
            // lookup 不依赖具体状态：重取最新状态决策，
            // 避免 StreamDone 状态校验因快照过期而丢弃本轮输出
            let sj = match mcp.lock().await.get_game_state("json").await {
                Ok(s) if !s.is_empty() => s,
                _ => state.decision_state_json.clone(),
            };
            let gs: GameState = serde_json::from_str(&sj).unwrap_or_default();
            state.game_state = gs.clone();
            state.last_state_json = sj.clone();
            // 自主模式走自主 prompt（查询记录在上下文中）；非自主带恢复指令
            // （否则 user_msg=None + 非 auto_mode 会走"只给文字建议"分支丢任务）
            let resume_msg = if state.auto_mode {
                None
            } else if ok {
                Some("（系统）查询完成，结果已附在下方「知识库查询记录」中。请基于查询结果继续完成我之前的指令。".to_string())
            } else {
                Some(
                    "（系统）查询无效或已达上限（3 次）。请基于现有信息继续完成我之前的指令。"
                        .to_string(),
                )
            };
            start_decision(
                &gs,
                &sj,
                config,
                llm,
                bt_tx,
                history,
                resume_msg.as_deref(),
                zh,
                state,
            );
            return;
        }

        // 自主模式开启请求：LLM 理解用户指令后发起，内核记录状态。
        // 丢弃同回复的剩余 ACTION，等状态稳定后进入自主循环重新决策。
        if action.tool == "auto_start" {
            let task = action
                .args
                .get("task")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if state.auto_mode {
                // 已处于自主模式：重复请求直接忽略
                full_text.clear();
                pending_actions.clear();
                return;
            }
            // 请求用户确认：内核不自行开启自主模式
            state.pending_auto_start = Some(task.clone());
            state.push_chat(
                MsgRole::System,
                format!(
                    "🤖 Agent 请求开启自主模式（{task}）。\n输入 y 同意 / n 拒绝（其他文字将取消该请求并当作普通对话）。"
                ),
            );
            full_text.clear();
            pending_actions.clear();
            *mode = Mode::PendingConfirm;
            state.progress = Some("等待确认自主模式…".into());
            return;
        }

        // 自主模式关闭请求：仅在自主模式中生效（任务完成的唯一 LLM 途径）
        if action.tool == "auto_stop" {
            if state.auto_mode {
                state.auto_mode = false;
                state.task = None;
                state.push_chat(MsgRole::System, "🤖 自主模式结束。".into());
            }
            pending_actions.clear();
            *mode = Mode::Idle;
            state.progress = None;
            full_text.clear();
            return;
        }

        // 门禁：非自主模式下无条件否决一切游戏操作
        if !state.auto_mode {
            state.push_chat(
                MsgRole::System,
                format!(
                    "⛔ 已否决 {}：非自主模式不执行游戏操作。需要操作时请输出 ACTION: auto_start。",
                    action.tool
                ),
            );
            pending_actions.clear();
            *mode = Mode::Idle;
            state.progress = None;
            full_text.clear();
            return;
        }

        // 自主模式内：正常动作入队执行（动作有产出即重置无动作计数）
        state.no_action_streak = 0;
        actions.remove(0);
        *pending_actions = actions;
        *mode = Mode::Executing;
        state.progress = Some(format!("执行 {}…", action.tool));
        spawn_exec(mcp, bt_tx, &action.tool, action.args);
    } else {
        // 无动作（tool_calls 与文本 ACTION 均为空）
        if state.auto_mode {
            // 自主模式中无 ACTION：不直接退出——纠正后重问（LLM 偶尔只输出文字分析）。
            // 连续 3 次无 ACTION 才视为放弃，退出自主模式。
            state.no_action_streak += 1;
            if state.no_action_streak >= 3 {
                state.auto_mode = false;
                state.task = None;
                state.no_action_streak = 0;
                state.push_chat(MsgRole::System, "自主模式结束（连续 3 轮无操作）。".into());
                *mode = Mode::Idle;
                state.progress = None;
            } else {
                let nudge = "（系统）自主模式仍在进行。请直接给出下一步游戏操作 ACTION；仅当任务已全部完成时才输出 ACTION: auto_stop。".to_string();
                let gs = state.game_state.clone();
                let sj = state.last_state_json.clone();
                full_text.clear();
                pending_actions.clear();
                *mode = Mode::Streaming;
                state.progress = Some("继续分析…".into());
                start_decision(
                    &gs,
                    &sj,
                    config,
                    llm,
                    bt_tx,
                    history,
                    Some(&nudge),
                    zh,
                    state,
                );
                return;
            }
        } else {
            *mode = Mode::Idle;
            state.progress = None; // 回答完成：清除"LLM 回复中…"等进度提示
        }
        state.last_poll = std::time::Instant::now();
    }

    full_text.clear();
}

/// 决策时状态已失效：丢弃本次分析；自主模式等状态稳定后继续。
#[allow(clippy::too_many_arguments, clippy::ptr_arg)]
fn on_stale_decision(
    state: &mut AppState,
    mode: &mut Mode,
    full_text: &mut String,
    mcp: &Arc<Mutex<McpClient>>,
    bt_tx: &mpsc::UnboundedSender<Backend>,
    current_sj: String,
) {
    state.streaming_text.clear();
    if !state.reasoning_text.is_empty() {
        let t = std::mem::take(&mut state.reasoning_text);
        state.push_chat(MsgRole::Thinking, t);
    }
    full_text.clear();
    // 明确告知用户：输出已显示但被作废（否则看起来像"打印了却没执行"）
    state.push_chat(
        MsgRole::System,
        "⚠️ 游戏状态已变化（决策期间局面变动），本次分析作废。".into(),
    );
    if !current_sj.is_empty() {
        let gs: GameState = serde_json::from_str(&current_sj).unwrap_or_default();
        state.game_state = gs;
        state.last_state_json = current_sj.clone();
    }
    // 只有自主模式才继续：等状态稳定后由 StateReady 统一路径处理
    if state.auto_mode && !current_sj.is_empty() {
        let gs: GameState = serde_json::from_str(&current_sj).unwrap_or_default();
        if !matches!(
            gs.state_type,
            StateType::Unknown | StateType::GameOver | StateType::Overlay
        ) {
            *mode = Mode::FetchingState;
            state.progress = Some("等待状态稳定…".into());
            spawn_state_stabilize(mcp, bt_tx);
            return;
        }
    }
    *mode = Mode::Idle;
    state.progress = None;
}

/// NOTE 行追加到 session 笔记文件。
fn append_session_notes(state: &AppState, config: &Config, notes: &[String]) {
    let notes_path = format!(
        "{}/{}_notes.md",
        config.storage.sessions_dir, state.session_id
    );
    let notes_content = notes
        .iter()
        .map(|n| format!("- {n}"))
        .collect::<Vec<_>>()
        .join("\n");
    if let Some(parent) = std::path::Path::new(&notes_path).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut existing = std::fs::read_to_string(&notes_path).unwrap_or_default();
    if !existing.is_empty() && !existing.ends_with('\n') {
        existing.push('\n');
    }
    existing.push_str(&notes_content);
    existing.push('\n');
    let _ = std::fs::write(&notes_path, &existing);
}

/// 一个动作执行完成：记录结果 → 继续执行队列/取下一帧状态。
#[allow(clippy::too_many_arguments)]
async fn on_exec_done(
    state: &mut AppState,
    mode: &mut Mode,
    mcp: &Arc<Mutex<McpClient>>,
    bt_tx: &mpsc::UnboundedSender<Backend>,
    budget: &mut BudgetGuard,
    session: &mut Session,
    store: &SessionStore,
    pending_actions: &mut Vec<parse::ParsedAction>,
    message: String,
    success: bool,
) {
    state.current_turn += 1;
    state.push_chat(MsgRole::System, format!("执行结果: {message}"));

    // 更新 session
    if let Some(t) = session.turns.last_mut() {
        t.result = Some(message.clone());
        t.success = success;
    }
    session.total_input = budget.total_input();
    session.total_output = budget.total_output();
    session.total_cost = budget.total_cost();
    let _ = store.save(session);

    if !success {
        state.push_chat(MsgRole::System, "执行失败，停止多步操作。".into());
        pending_actions.clear();
    }

    // 如果还有待执行动作且仍在自主模式，直接执行下一条（已解析，无需再 parse）
    if success && !pending_actions.is_empty() && state.auto_mode {
        let next = pending_actions.remove(0);
        *mode = Mode::Executing;
        state.progress = Some(format!("执行 {}…", next.tool));
        spawn_exec(mcp, bt_tx, &next.tool, next.args);
        return;
    }

    // 队列空了 → 检查预算 → 等状态稳定
    if budget.is_over_budget() {
        state.finished = true;
        state.progress = Some(format!("预算超限: {}", budget.summary()));
        *mode = Mode::Idle;
        return;
    }

    *mode = Mode::FetchingState;
    state.progress = Some("等待状态稳定…".into());
    spawn_state_stabilize(mcp, bt_tx);
}

/// 等待游戏状态稳定后再发 StateReady（供 ExecDone/StateChange 共用）。
/// 双取确认：取一次 → 等 0.5s → 再取，相同才认为稳定；不同则再等一次。
pub(super) fn spawn_state_stabilize(
    mcp: &Arc<Mutex<McpClient>>,
    bt_tx: &mpsc::UnboundedSender<Backend>,
) {
    let mcp2 = mcp.clone();
    let bt_tx2 = bt_tx.clone();
    tokio::spawn(async move {
        // 短等待 + 双取确认：动作已生效，只需等动画/结算落定
        tokio::time::sleep(std::time::Duration::from_millis(800)).await;
        let mut m = mcp2.lock().await;
        let s1 = m.get_game_state("json").await;
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        let s2 = m.get_game_state("json").await;
        match (s1, s2) {
            (Ok(a), Ok(b)) if a == b => {
                let _ = bt_tx2.send(Backend::StateReady(a));
            }
            (Ok(_a), Ok(_b)) => {
                // 不稳定，再等一次
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                match m.get_game_state("json").await {
                    Ok(c) => {
                        let _ = bt_tx2.send(Backend::StateReady(c));
                    }
                    Err(e) => {
                        let _ = bt_tx2.send(Backend::Error(format!("取状态失败: {e:#}")));
                    }
                }
            }
            (Ok(a), Err(_)) => {
                let _ = bt_tx2.send(Backend::StateReady(a));
            }
            (Err(e), _) => {
                let _ = bt_tx2.send(Backend::Error(format!("取状态失败: {e:#}")));
            }
        }
    });
}

/// ExecDone 后取到稳定状态：自主模式 → 反射动作或继续 LLM 分析。
#[allow(clippy::too_many_arguments, clippy::ptr_arg)]
async fn on_state_ready(
    state: &mut AppState,
    mode: &mut Mode,
    config: &Config,
    llm: &LlmClient,
    mcp: &Arc<Mutex<McpClient>>,
    bt_tx: &mpsc::UnboundedSender<Backend>,
    history: &mut Vec<decide::ChatTurn>,
    full_text: &mut String,
    zh: bool,
    max_turns: u32,
    sj: String,
) {
    let gs: GameState = serde_json::from_str(&sj).unwrap_or_default();
    state.game_state = gs.clone();
    state.last_state_json = sj.clone();

    if gs.state_type == StateType::GameOver {
        state.finished = true;
        state.progress = Some("游戏结束".into());
        *mode = Mode::Idle;
        return;
    }
    if gs.state_type == StateType::Unknown {
        // 加载中等，不分析，等下次状态变化再触发
        *mode = Mode::Idle;
        state.progress = Some("等待游戏加载…".into());
        return;
    }

    // 只有自主模式才继续自动分析（执行完一步后取新状态继续）
    if !state.auto_mode {
        *mode = Mode::Idle;
        state.progress = None;
        return;
    }
    // 自主循环轮数上限（max_turns=0 表示不限；预算守卫仍会兜底）
    if max_turns > 0 && state.current_turn >= max_turns {
        state.auto_mode = false;
        state.task = None;
        state.finished = true;
        state.progress = Some(format!("已达最大轮数 {max_turns}"));
        state.push_chat(
            MsgRole::System,
            format!("已达最大轮数 {max_turns}，自主模式结束。"),
        );
        *mode = Mode::Idle;
        return;
    }
    // 反射动作：机械操作不问 LLM（省 token）
    if let Some((tool, args)) = try_reflex_action(&gs) {
        spawn_exec(mcp, bt_tx, &tool, args);
        *mode = Mode::Executing;
        state.progress = Some(format!("执行 {tool}…"));
        return;
    }
    *mode = Mode::Streaming;
    state.progress = Some("分析中…".into());
    full_text.clear();
    start_decision(&gs, &sj, config, llm, bt_tx, history, None, zh, state);
}
