//! TUI 异步事件循环——自然语言对话模式。
//!
//! 流程：
//! 1. 取状态 → LLM 生成建议（流式，用户打字即打断）
//! 2. 建议出来后不自动执行，等用户确认
//! 3. 用户输入解析意图：执行 / 拒绝 / 对话 / 打断
//! 4. 确认 → MCP 执行 → 取下一状态 → 循环

use std::io::stdout;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use crossterm::event::{poll, read, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use tokio::sync::{mpsc, Mutex};
use tokio_util::sync::CancellationToken;

use sts2_agent::storage::{Session, SessionStore};
use sts2_agent::{decide, parse};
use sts2_core::{Config, GameState, StateType};
use sts2_llm::{BudgetGuard, LlmClient, StreamEvent, Usage};
use sts2_mcp::McpClient;

use crate::app::{state_summary, AppState, Mode, MsgRole};
use crate::ui;

/// 发起一轮决策：构造 prompt → LLM 流式。
/// 调用前必须确保旧流已 cancel + bt_rx 已排空。
#[allow(clippy::too_many_arguments)]
fn start_decision(
    gs: &GameState,
    state_json: &str,
    config: &Config,
    llm: &LlmClient,
    bt_tx: &mpsc::UnboundedSender<Backend>,
    history: &[decide::ChatTurn],
    user_msg: Option<&str>,
    zh: bool,
    state: &mut AppState,
) {
    // 创建新的 cancel token 并存到 state
    let cancel = CancellationToken::new();
    state.current_cancel = cancel.clone();
    state.decision_state_json = state_json.to_string();
    let summary = state_summary(gs);

    // game-knowledge 结构化索引检索
    let game_knowledge =
        sts2_agent::knowledge::search_game_knowledge(gs, &config.storage.game_knowledge_dir);

    // 读取 session 笔记
    let notes_path = format!(
        "{}/{}_notes.md",
        config.storage.sessions_dir, state.session_id
    );
    let session_notes = std::fs::read_to_string(&notes_path).ok();

    let messages = decide::build_messages(
        state_json,
        &config.model.model,
        history,
        &summary,
        user_msg,
        state.auto_mode,
        state.task.as_deref(),
        if game_knowledge.is_empty() {
            None
        } else {
            Some(&game_knowledge)
        },
        session_notes.as_deref(),
        zh,
    );
    match llm.chat_stream(&messages) {
        Ok(rx) => {
            let bt_tx2 = bt_tx.clone();
            tokio::spawn(async move {
                consume_stream(rx, bt_tx2, cancel).await;
            });
        }
        Err(e) => {
            let _ = bt_tx.send(Backend::Error(format!("LLM 失败: {e:#}")));
        }
    }
}

/// 打断当前 LLM 流：cancel + 排空 bt_rx + 清空输出。
fn abort_current_llm(
    state: &mut AppState,
    bt_rx: &mut mpsc::UnboundedReceiver<Backend>,
    full_text: &mut String,
) {
    state.current_cancel.cancel();
    // 排空所有 stale 消息
    while bt_rx.try_recv().is_ok() {}
    state.streaming_text.clear();
    state.reasoning_text.clear();
    full_text.clear();
}

/// 自主模式下的反射动作：无需 LLM 判断的机械操作（省 token）。
/// 只处理确定无歧义的操作，其余返回 None 交给 LLM。
fn try_reflex_action(gs: &GameState) -> Option<(String, serde_json::Value)> {
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
fn spawn_exec(
    mcp: &Arc<Mutex<McpClient>>,
    bt_tx: &mpsc::UnboundedSender<Backend>,
    tool: &str,
    args: serde_json::Value,
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

/// 后台状态轮询：每 1 秒检查游戏状态是否变化。
async fn poll_state_loop(
    mcp: Arc<Mutex<McpClient>>,
    bt_tx: mpsc::UnboundedSender<Backend>,
    initial_state: String,
) {
    let mut last = initial_state;
    loop {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let sj = mcp
            .lock()
            .await
            .get_game_state("json")
            .await
            .unwrap_or_default();
        if !sj.is_empty() && sj != last {
            last = sj.clone();
            let _ = bt_tx.send(Backend::StateChange(sj));
        }
    }
}

enum Backend {
    StateReady(String),
    StateChange(String),
    Delta(String),
    Reasoning(String),
    Usage(Usage),
    StreamDone,
    StreamError(String),
    ExecDone { success: bool, message: String },
    IntentReady { text: String, intent: UserIntent },
    Error(String),
}

/// 用户意图。
#[derive(Debug, Clone)]
enum UserIntent {
    Confirm,
    Reject,
    Interrupt,
    Quit,
    /// 进入自主模式：用户明确说了"自己打"等。
    AutoPlay,
    Chat(String),
}

pub async fn run(
    config: &Config,
    use_mock: bool,
    show_thinking: bool,
    zh: bool,
    _auto_play: bool,
    max_turns: u32,
) -> Result<()> {
    enable_raw_mode()?;
    execute!(stdout(), EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout());
    let mut terminal = Terminal::new(backend)?;

    let mut state = AppState::new(zh);
    state.show_thinking = show_thinking;

    let (command, args) = if use_mock {
        ("./target/debug/sts2-mcp-mock".to_string(), Vec::new())
    } else {
        (config.mcp.command.clone(), config.mcp.args.clone())
    };
    let mut mcp = McpClient::spawn(&command, &args)?;
    mcp.initialize().await?;
    let mcp = Arc::new(Mutex::new(mcp));

    // 取首帧
    let sj = mcp.lock().await.get_game_state("json").await?;
    let gs: GameState = serde_json::from_str(&sj).unwrap_or_default();
    state.game_state = gs.clone();
    state.last_state_json = sj.clone();

    let llm = LlmClient::from_config(&config.model);
    let mut budget = BudgetGuard::new(config.budget.token_limit, config.budget.cost_limit_usd);
    let mut history: Vec<decide::ChatTurn> = Vec::new();

    let (bt_tx, mut bt_rx) = mpsc::unbounded_channel::<Backend>();

    // 会话存储（R5）
    let store = SessionStore::from_dir(&config.storage.sessions_dir);
    let mut session = Session::new(&config.model.model);

    // 后台状态轮询：每 1 秒检查游戏状态是否变化，变化则发 StateChange
    {
        let mcp_poll = mcp.clone();
        let bt_tx_poll = bt_tx.clone();
        let last_known = state.last_state_json.clone();
        tokio::spawn(poll_state_loop(mcp_poll, bt_tx_poll, last_known));
    }

    // 启动后自动发起首轮决策（仅建议，不自动执行）
    let mut mode = Mode::Idle;
    let mut full_text = String::new();
    let mut pending_actions: Vec<String> = Vec::new();
    state.execute_actions = false;

    // 首次只更新状态，不自动发起 LLM 分析——等用户指令
    state.push_chat(
        MsgRole::System,
        "已连接。输入消息开始对话（如\"分析一下\"或\"自己打\"）。".into(),
    );
    state.session_id = session.id.clone();

    let mut should_quit = false;
    let mut last_draw = std::time::Instant::now();

    loop {
        // 按键优先：先非阻塞检查按键，再决定是否重绘
        if poll(Duration::from_millis(0))? {
            if let Event::Key(key) = read()? {
                if key.kind == KeyEventKind::Press {
                    match key.code {
                        KeyCode::Enter => {
                            let text = state.submit();
                            if text.is_empty() {
                                continue;
                            }
                            // 打断当前流
                            if !matches!(mode, Mode::Idle) && !matches!(mode, Mode::PendingConfirm)
                            {
                                abort_current_llm(&mut state, &mut bt_rx, &mut full_text);
                                mode = Mode::Idle;
                            }
                            state.push_chat(MsgRole::User, text.clone());
                            state.progress = Some("理解中…".into());
                            // 关键词意图分类（省一次 LLM 调用）；
                            // 未命中关键词的输入归 Chat，由决策 LLM 判断是否为操作指令
                            let intent = parse_intent(&text);
                            let _ = bt_tx.send(Backend::IntentReady { text, intent });
                        }
                        KeyCode::Backspace => {
                            state.backspace();
                        }
                        KeyCode::Esc => {
                            should_quit = true;
                        }
                        KeyCode::Char(c) => {
                            state.input_char(c);
                        }
                        KeyCode::Up => {
                            if state.chat_scroll < 1000 {
                                state.chat_scroll += 3;
                            }
                        }
                        KeyCode::Down => {
                            state.chat_scroll = state.chat_scroll.saturating_sub(3);
                        }
                        _ => {}
                    }
                }
            }
        }

        if should_quit {
            session.finished = true;
            session.total_input = budget.total_input();
            session.total_output = budget.total_output();
            session.total_cost = budget.total_cost();
            let _ = store.save(&session);
            break;
        }

        // 非阻塞排空后台消息
        while let Ok(msg) = bt_rx.try_recv() {
            let quit = handle_backend_msg(
                msg,
                &mut state,
                &mut mode,
                &mut full_text,
                config,
                &llm,
                &mcp,
                &bt_tx,
                &mut bt_rx,
                &mut history,
                &mut budget,
                &mut session,
                &store,
                &mut pending_actions,
                zh,
                max_turns,
            )
            .await;
            if quit {
                should_quit = true;
                break;
            }
        }

        // 限帧重绘：最多 15fps（66ms），减少 wrap_line 计算频率
        if last_draw.elapsed() >= Duration::from_millis(66) {
            terminal.draw(|f| ui::draw(f, &state))?;
            last_draw = std::time::Instant::now();
        }

        // 短暂让出 CPU，不 busy-loop
        tokio::task::yield_now().await;
    }

    // 终端恢复
    disable_raw_mode()?;
    execute!(stdout(), LeaveAlternateScreen)?;
    mcp.lock().await.shutdown().await.ok();
    eprintln!("\n对局结束。总用量: {}", budget.summary());
    Ok(())
}

/// 后台消费 LLM 流，推回主循环。cancel 用于打断。
async fn consume_stream(
    mut rx: mpsc::UnboundedReceiver<StreamEvent>,
    bt_tx: mpsc::UnboundedSender<Backend>,
    cancel: CancellationToken,
) {
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                return;
            }
            ev = rx.recv() => {
                match ev {
                    Some(StreamEvent::Delta(t)) => {
                        let _ = bt_tx.send(Backend::Delta(t));
                    }
                    Some(StreamEvent::Reasoning(t)) => {
                        let _ = bt_tx.send(Backend::Reasoning(t));
                    }
                    Some(StreamEvent::Usage(u)) => {
                        let _ = bt_tx.send(Backend::Usage(u));
                    }
                    Some(StreamEvent::Done) => {
                        let _ = bt_tx.send(Backend::StreamDone);
                        return;
                    }
                    Some(StreamEvent::Error(e)) => {
                        let _ = bt_tx.send(Backend::StreamError(e));
                        return;
                    }
                    None => {
                        let _ = bt_tx.send(Backend::StreamDone);
                        return;
                    }
                }
            }
        }
    }
}

/// 处理后台消息。
#[allow(clippy::too_many_arguments, clippy::ptr_arg)]
async fn handle_backend_msg(
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
    // 返回 true = 应退出
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
            // 问题: 检查当前状态与 LLM 分析时的状态是否一致
            // 如果不一致（用户中间操作了），作废本次决策
            let current_sj = mcp
                .lock()
                .await
                .get_game_state("json")
                .await
                .unwrap_or_default();
            let state_valid = !current_sj.is_empty() && current_sj == state.decision_state_json;

            if !state_valid {
                // 状态已变，丢弃本次分析（不显示，不执行）
                state.streaming_text.clear();
                state.reasoning_text.clear();
                full_text.clear();
                // 更新当前状态
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
                        return false;
                    }
                }
                *mode = Mode::Idle;
                state.progress = None;
                return false;
            }

            // 状态一致，正常处理 LLM 输出
            // 提取 NOTE 行（LLM 自主经验笔记）
            let notes = parse::extract_notes(full_text);
            if !notes.is_empty() {
                let notes_path = format!(
                    "{}/{}_notes.md",
                    config.storage.sessions_dir, state.session_id
                );
                let notes_content = notes
                    .iter()
                    .map(|n| format!("- {n}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                // 追加到 session 笔记文件
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

            if !action_lines.is_empty() && state.execute_actions {
                // 用户指令触发 → 执行 ACTION 行
                let first_line = action_lines[0].clone();
                match parse::parse_action(&first_line) {
                    Ok(action) => {
                        *pending_actions = action_lines[1..].to_vec();
                        *mode = Mode::Executing;
                        state.progress = Some(format!("执行 {}…", action.tool));
                        let mcp2 = mcp.clone();
                        let bt_tx2 = bt_tx.clone();
                        let tool = action.tool;
                        let args = action.args;
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
        Backend::StreamError(e) => {
            state.push_chat(MsgRole::System, format!("LLM 错误: {e}"));
            state.streaming_text.clear();
            *mode = Mode::Idle;
            full_text.clear();
        }
        Backend::ExecDone { success, message } => {
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

            // 如果还有待执行动作，等 1 秒后继续执行下一条（游戏状态更新有延迟）
            if success && !pending_actions.is_empty() {
                let next_line = pending_actions.remove(0);
                let mcp2 = mcp.clone();
                let bt_tx2 = bt_tx.clone();
                *mode = Mode::Executing;
                state.progress = Some("等待状态更新…".into());
                tokio::spawn(async move {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                    let action = match parse::parse_action(&next_line) {
                        Ok(a) => a,
                        Err(e) => {
                            let _ = bt_tx2.send(Backend::Error(format!("解析失败: {e:#}")));
                            return;
                        }
                    };
                    let tool = action.tool;
                    let args = action.args;
                    let mut m = mcp2.lock().await;
                    let result = m.call_tool(&tool, args).await;
                    let (success, message) = match result {
                        Ok(r) => (true, r.chars().take(120).collect()),
                        Err(e) => (false, format!("{e:#}")),
                    };
                    let _ = bt_tx2.send(Backend::ExecDone { success, message });
                });
                return false;
            }

            // 队列空了 → 检查预算/轮数 → 等 1 秒后取下一帧状态
            if budget.is_over_budget() {
                state.finished = true;
                state.progress = Some(format!("预算超限: {}", budget.summary()));
                *mode = Mode::Idle;
                return false;
            }

            *mode = Mode::FetchingState;
            state.progress = Some("等待状态稳定…".into());
            let mcp2 = mcp.clone();
            let bt_tx2 = bt_tx.clone();
            tokio::spawn(async move {
                tokio::time::sleep(Duration::from_secs(2)).await;
                let mut m = mcp2.lock().await;
                // 双取确认状态稳定：第一次取，等 0.5 秒再取，相同才认为稳定
                let s1 = m.get_game_state("json").await;
                tokio::time::sleep(Duration::from_millis(500)).await;
                let s2 = m.get_game_state("json").await;
                match (s1, s2) {
                    (Ok(a), Ok(b)) if a == b => {
                        let _ = bt_tx2.send(Backend::StateReady(a));
                    }
                    (Ok(_a), Ok(_b)) => {
                        // 不稳定，再等一次
                        tokio::time::sleep(Duration::from_millis(500)).await;
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
        Backend::StateReady(sj) => {
            let gs: GameState = serde_json::from_str(&sj).unwrap_or_default();
            state.game_state = gs.clone();
            state.last_state_json = sj.clone();

            if gs.state_type == StateType::GameOver {
                state.finished = true;
                state.progress = Some("游戏结束".into());
                *mode = Mode::Idle;
                return false;
            }
            if gs.state_type == StateType::Unknown {
                // 加载中等，不分析，等下次状态变化再触发
                *mode = Mode::Idle;
                state.progress = Some("等待游戏加载…".into());
                return false;
            }

            // 只有自主模式才继续自动分析（执行完一步后取新状态继续）
            if !state.auto_mode {
                *mode = Mode::Idle;
                state.progress = None;
                return false;
            }
            // 反射动作：机械操作不问 LLM（省 token）
            if let Some((tool, args)) = try_reflex_action(&gs) {
                spawn_exec(mcp, bt_tx, &tool, args);
                *mode = Mode::Executing;
                state.progress = Some(format!("执行 {tool}…"));
                return false;
            }
            state.execute_actions = true;
            *mode = Mode::Streaming;
            state.progress = Some("分析中…".into());
            full_text.clear();
            start_decision(&gs, &sj, config, llm, bt_tx, history, None, zh, state);
        }
        Backend::StateChange(sj) => {
            if sj == state.last_state_json {
                return false;
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
                return false;
            }

            // 正在执行动作或取状态时不打断（等 ExecDone→StateReady 正常流程）
            if matches!(*mode, Mode::Executing | Mode::FetchingState) {
                return false;
            }

            // 核心：状态变化必须打断当前 LLM 流，丢弃输出，用新状态重新分析
            abort_current_llm(state, bt_rx, full_text);

            // 判断是否需要自动分析：只有自主模式（用户明确说了"自己打"）才持续自动操作
            if !state.auto_mode {
                *mode = Mode::Idle;
                state.progress = None;
                return false;
            }

            // 反射动作：机械操作不问 LLM（省 token）
            if let Some((tool, args)) = try_reflex_action(&gs) {
                spawn_exec(mcp, bt_tx, &tool, args);
                *mode = Mode::Executing;
                state.progress = Some(format!("执行 {tool}…"));
                return false;
            }

            // 用新状态重新分析
            state.execute_actions = true;
            *mode = Mode::Streaming;
            state.progress = Some("状态更新，重新分析…".into());
            start_decision(&gs, &sj, config, llm, bt_tx, history, None, zh, state);
        }
        Backend::Error(e) => {
            state.push_chat(MsgRole::System, e);
            *mode = Mode::Idle;
            state.progress = None;
        }
    }
    false
}

/// 处理用户意图。
#[allow(clippy::too_many_arguments, clippy::ptr_arg)]
async fn handle_user_intent(
    intent: &UserIntent,
    text: &str,
    state: &mut AppState,
    mode: &mut Mode,
    config: &Config,
    llm: &LlmClient,
    mcp: &Arc<Mutex<McpClient>>,
    bt_tx: &mpsc::UnboundedSender<Backend>,
    bt_rx: &mut mpsc::UnboundedReceiver<Backend>,
    history: &mut Vec<decide::ChatTurn>,
    budget: &mut BudgetGuard,
    zh: bool,
    max_turns: u32,
    full_text: &mut String,
) {
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
            *mode = Mode::Streaming;
            state.progress = Some("自主模式启动…".into());
            state.streaming_text.clear();
            full_text.clear();
            let sj = mcp
                .lock()
                .await
                .get_game_state("json")
                .await
                .unwrap_or_default();
            state.last_state_json = sj.clone();
            let gs: GameState = serde_json::from_str(&sj).unwrap_or_default();
            state.game_state = gs.clone();
            start_decision(&gs, &sj, config, llm, bt_tx, history, Some(text), zh, state);
        }
        UserIntent::Confirm => {
            // 确认执行：只执行本轮 ACTION，不进入持续自主模式
            history.push(decide::ChatTurn::User(text.to_string()));
            state.execute_actions = true; // 临时开启，执行完一轮后自动关闭
            state.task = Some(text.to_string());
            state.pending_action = None;
            *mode = Mode::Streaming;
            state.progress = Some("LLM 处理中…".into());
            state.streaming_text.clear();
            full_text.clear();
            let sj = mcp
                .lock()
                .await
                .get_game_state("json")
                .await
                .unwrap_or_default();
            let gs: GameState = serde_json::from_str(&sj).unwrap_or_default();
            state.game_state = gs.clone();
            start_decision(&gs, &sj, config, llm, bt_tx, history, Some(text), zh, state);
        }
        UserIntent::Reject => {
            history.push(decide::ChatTurn::User(text.to_string()));
            state.pending_action = None;
            state.execute_actions = false;
            state.task = None;
            *mode = Mode::FetchingState;
            state.progress = Some("重新决策中…".into());
            let sj = {
                let mut m = mcp.lock().await;
                match m.get_game_state("json").await {
                    Ok(s) => s,
                    Err(e) => {
                        state.push_chat(MsgRole::System, format!("取状态失败: {e:#}"));
                        *mode = Mode::Idle;
                        return;
                    }
                }
            };
            state.last_state_json = sj.clone();
            let gs: GameState = serde_json::from_str(&sj).unwrap_or_default();
            state.game_state = gs.clone();
            *mode = Mode::Streaming;
            full_text.clear();
            start_decision(&gs, &sj, config, llm, bt_tx, history, Some(text), zh, state);
        }
        UserIntent::Chat(msg) => {
            // 对话/操作指令：由决策 LLM 判断是否输出 ACTION。
            // execute_actions=true + 非 auto_mode = 单次执行语义（执行一轮即停），
            // 纯对话（无 ACTION）则不操作游戏。
            history.push(decide::ChatTurn::User(msg.clone()));
            state.execute_actions = true;
            state.task = None;
            *mode = Mode::Streaming;
            state.progress = Some("LLM 回复中…".into());
            state.streaming_text.clear();
            full_text.clear();

            let sj = mcp
                .lock()
                .await
                .get_game_state("json")
                .await
                .unwrap_or_default();
            state.last_state_json = sj.clone();
            let gs: GameState = serde_json::from_str(&sj).unwrap_or_default();
            state.game_state = gs.clone();
            start_decision(&gs, &sj, config, llm, bt_tx, history, Some(msg), zh, state);
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

/// 关键词意图分类（无 LLM，省一次调用）。
/// 未命中关键词的输入归 Chat，由决策 LLM 判断是否为操作指令（单次执行语义）。
fn parse_intent(text: &str) -> UserIntent {
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
