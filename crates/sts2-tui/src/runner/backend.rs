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
use super::stream::{abort_current_llm, start_decision};
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
    pending_actions: &mut Vec<String>,
    zh: bool,
    max_turns: u32,
) -> bool {
    match msg {
        Backend::Delta(t) => {
            state.streaming_text.push_str(&t);
            full_text.push_str(&t);
        }
        Backend::Reasoning(t) => {
            state.reasoning_text.push_str(&t);
        }
        Backend::Usage(u) => {
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
                &intent, &text, state, mode, config, llm, mcp, bt_tx, bt_rx, history, budget, zh,
                max_turns, full_text,
            )
            .await;
        }
        Backend::StreamDone => {
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
                zh,
            )
            .await;
        }
        Backend::StreamError(e) => {
            state.push_chat(MsgRole::System, format!("LLM 错误: {e}"));
            state.streaming_text.clear();
            *mode = Mode::Idle;
            full_text.clear();
        }
        Backend::ExecDone { success, message } => {
            on_exec_done(
                state,
                mode,
                config,
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
                state, mode, config, llm, mcp, bt_tx, history, full_text, zh, sj,
            )
            .await;
        }
        Backend::StateChange(sj) => {
            on_state_change(
                state, mode, full_text, config, llm, mcp, bt_tx, bt_rx, history, zh, sj,
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
    pending_actions: &mut Vec<String>,
    zh: bool,
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
        return on_stale_decision(
            state, mode, full_text, config, llm, bt_tx, history, zh, current_sj,
        );
    }

    // ---- 状态一致，正常处理 LLM 输出 ----

    // 1. 提取 NOTE 行（LLM 自主经验笔记）并追加到 session 笔记文件
    let notes = parse::extract_notes(full_text);
    if !notes.is_empty() {
        append_session_notes(state, config, &notes);
    }

    // 2. 分离对话文本与 ACTION 行
    let action_lines: Vec<String> = full_text
        .lines()
        .filter(|l| parse::is_action_line(l))
        .map(|l| l.to_string())
        .collect();
    let chat_text: String = full_text
        .lines()
        .filter(|l| !parse::is_action_line(l) && !parse::is_note_line(l))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string();
    if !chat_text.is_empty() {
        state.push_chat(MsgRole::Agent, chat_text.clone());
        history.push(decide::ChatTurn::Assistant(chat_text));
    }
    state.streaming_text.clear();
    state.reasoning_text.clear();

    // 3. 执行 ACTION（或 lookup 拦截）
    if !action_lines.is_empty() && state.execute_actions {
        let first_line = action_lines[0].clone();
        match parse::parse_action(&first_line) {
            Ok(action) => {
                // 知识库查询：拦截，不发给游戏，查完注入上下文重新决策
                if action.tool == "lookup" {
                    let query = action
                        .args
                        .get("query")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    // 查询结果已写入"知识库查询记录"。必须带指令重新决策：
                    // 否则 user_msg=None + 非 auto_mode 会走"只给文字建议、
                    // 不要 ACTION"分支，丢失原任务且禁执行。
                    let resume = if handle_lookup(&query, state, config, mcp).await {
                        "（系统）查询完成，结果已附在下方「知识库查询记录」中。请基于查询结果继续完成我之前的指令。".to_string()
                    } else {
                        "（系统）查询无效或已达上限（3 次）。请基于现有信息继续完成我之前的指令。"
                            .to_string()
                    };
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
                    start_decision(
                        &gs,
                        &sj,
                        config,
                        llm,
                        bt_tx,
                        history,
                        Some(&resume),
                        zh,
                        state,
                    );
                    return;
                }

                // 正常动作：入队执行
                *pending_actions = action_lines[1..].to_vec();
                *mode = Mode::Executing;
                state.progress = Some(format!("执行 {}…", action.tool));
                spawn_exec(mcp, bt_tx, &action.tool, action.args);
            }
            Err(e) => {
                state.push_chat(MsgRole::System, format!("解析失败: {e:#}"));
                *mode = Mode::Idle;
            }
        }
    } else {
        // 没有 ACTION 行，或 execute_actions 为 false
        // auto_mode 下 LLM 没给 ACTION = 它认为该停了 → 退出自主模式
        if state.auto_mode && action_lines.is_empty() {
            state.auto_mode = false;
            state.task = None;
            state.push_chat(MsgRole::System, "自主模式结束。".into());
        }
        // 非 auto_mode 时执行权限只持续一轮，用完即关
        state.execute_actions = state.auto_mode;
        *mode = Mode::Idle;
        state.last_poll = std::time::Instant::now();
    }

    full_text.clear();
}

/// 决策时状态已失效：丢弃本次分析；自主模式用新状态重新分析。
#[allow(clippy::too_many_arguments, clippy::ptr_arg)]
fn on_stale_decision(
    state: &mut AppState,
    mode: &mut Mode,
    full_text: &mut String,
    config: &Config,
    llm: &LlmClient,
    bt_tx: &mpsc::UnboundedSender<Backend>,
    history: &mut [decide::ChatTurn],
    zh: bool,
    current_sj: String,
) {
    state.streaming_text.clear();
    state.reasoning_text.clear();
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
    // 只有自主模式才用新状态重新分析
    if state.auto_mode && !current_sj.is_empty() {
        let gs: GameState = serde_json::from_str(&current_sj).unwrap_or_default();
        if !matches!(
            gs.state_type,
            StateType::Unknown | StateType::GameOver | StateType::Overlay
        ) {
            state.execute_actions = true;
            *mode = Mode::Streaming;
            state.progress = Some("状态变化，重新分析…".into());
            start_decision(
                &gs,
                &current_sj,
                config,
                llm,
                bt_tx,
                history,
                None,
                zh,
                state,
            );
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
    config: &Config,
    mcp: &Arc<Mutex<McpClient>>,
    bt_tx: &mpsc::UnboundedSender<Backend>,
    budget: &mut BudgetGuard,
    session: &mut Session,
    store: &SessionStore,
    pending_actions: &mut Vec<String>,
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

    // 如果还有待执行动作，等 2 秒后继续执行下一条（游戏状态更新有延迟）
    if success && !pending_actions.is_empty() {
        let next_line = pending_actions.remove(0);
        *mode = Mode::Executing;
        state.progress = Some("等待状态更新…".into());
        // 解析下一条；解析失败则报错结束本轮队列
        match parse::parse_action(&next_line) {
            Ok(action) => {
                spawn_exec(mcp, bt_tx, &action.tool, action.args);
            }
            Err(e) => {
                let _ = bt_tx.send(Backend::Error(format!("解析失败: {e:#}")));
            }
        }
        return;
    }

    // 队列空了 → 检查预算 → 等 2 秒后取下一帧状态
    if budget.is_over_budget() {
        state.finished = true;
        state.progress = Some(format!("预算超限: {}", budget.summary()));
        *mode = Mode::Idle;
        return;
    }

    *mode = Mode::FetchingState;
    state.progress = Some("等待状态稳定…".into());
    let mcp2 = mcp.clone();
    let bt_tx2 = bt_tx.clone();
    let _ = config; // 后续如需可传配置
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        let mut m = mcp2.lock().await;
        // 双取确认状态稳定：第一次取，等 0.5 秒再取，相同才认为稳定
        let s1 = m.get_game_state("json").await;
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        let s2 = m.get_game_state("json").await;
        match (s1, s2) {
            (Ok(a), Ok(b)) if a == b => {
                let _ = bt_tx2.send(Backend::StateReady(a));
            }
            (Ok(_a), Ok(_b)) => {
                // 不稳定，再等一次
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
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
    // 反射动作：机械操作不问 LLM（省 token）
    if let Some((tool, args)) = try_reflex_action(&gs) {
        spawn_exec(mcp, bt_tx, &tool, args);
        *mode = Mode::Executing;
        state.progress = Some(format!("执行 {tool}…"));
        return;
    }
    state.execute_actions = true;
    *mode = Mode::Streaming;
    state.progress = Some("分析中…".into());
    full_text.clear();
    start_decision(&gs, &sj, config, llm, bt_tx, history, None, zh, state);
}

/// 后台轮询检测到游戏状态变化（通常是用户手动操作）。
#[allow(clippy::too_many_arguments, clippy::ptr_arg)]
async fn on_state_change(
    state: &mut AppState,
    mode: &mut Mode,
    full_text: &mut String,
    config: &Config,
    llm: &LlmClient,
    mcp: &Arc<Mutex<McpClient>>,
    bt_tx: &mpsc::UnboundedSender<Backend>,
    bt_rx: &mut mpsc::UnboundedReceiver<Backend>,
    history: &mut [decide::ChatTurn],
    zh: bool,
    sj: String,
) {
    if sj == state.last_state_json {
        return;
    }
    state.last_state_json = sj.clone();
    let gs: GameState = serde_json::from_str(&sj).unwrap_or_default();
    state.game_state = gs.clone();

    // Unknown/GameOver/Overlay: 更新状态但不分析
    if matches!(
        gs.state_type,
        StateType::Unknown | StateType::GameOver | StateType::Overlay
    ) {
        if matches!(gs.state_type, StateType::GameOver) {
            state.finished = true;
            state.progress = Some("游戏结束".into());
        }
        // 打断当前 LLM 流（如有）
        abort_current_llm(state, bt_rx, full_text);
        *mode = Mode::Idle;
        return;
    }

    // 正在执行动作或取状态时不打断（等 ExecDone→StateReady 正常流程）
    if matches!(*mode, Mode::Executing | Mode::FetchingState) {
        return;
    }

    // 核心：状态变化必须打断当前 LLM 流，丢弃输出，用新状态重新分析
    abort_current_llm(state, bt_rx, full_text);
    if !state.auto_mode {
        state.push_chat(
            MsgRole::System,
            "⚠️ 游戏状态变化，已取消当前分析（输出未执行）。".into(),
        );
    }

    // 判断是否需要自动分析：只有自主模式（用户明确说了"自己打"）才持续自动操作
    if !state.auto_mode {
        *mode = Mode::Idle;
        state.progress = None;
        return;
    }

    // 反射动作：机械操作不问 LLM（省 token）
    if let Some((tool, args)) = try_reflex_action(&gs) {
        spawn_exec(mcp, bt_tx, &tool, args);
        *mode = Mode::Executing;
        state.progress = Some(format!("执行 {tool}…"));
        return;
    }

    // 用新状态重新分析
    state.execute_actions = true;
    *mode = Mode::Streaming;
    state.progress = Some("状态更新，重新分析…".into());
    start_decision(&gs, &sj, config, llm, bt_tx, history, None, zh, state);
}
