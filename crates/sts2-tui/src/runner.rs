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

use sts2_agent::{decide, parse};
use sts2_core::{Config, GameState, StateType};
use sts2_llm::{BudgetGuard, LlmClient, StreamEvent, Usage};
use sts2_mcp::McpClient;

use crate::app::{state_summary, AppState, Mode, MsgRole};
use crate::ui;

/// 发起一轮决策：构造 prompt → LLM 流式。`user_msg` = 用户本轮输入。
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
) {
    let summary = state_summary(gs);
    let messages = decide::build_messages(
        state_json,
        &config.model.model,
        history,
        &summary,
        user_msg,
        zh,
    );
    match llm.chat_stream(&messages) {
        Ok(rx) => {
            let bt_tx2 = bt_tx.clone();
            tokio::spawn(async move {
                consume_stream(rx, bt_tx2).await;
            });
        }
        Err(e) => {
            let _ = bt_tx.send(Backend::Error(format!("LLM 失败: {e:#}")));
        }
    }
}

/// 后台 → 主循环消息。
enum Backend {
    StateReady(String),
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
    /// 确认执行当前建议。
    Confirm,
    /// 拒绝/换一个。
    Reject,
    /// 打断当前 LLM 流。
    Interrupt,
    /// 退出程序。
    Quit,
    /// 与 Agent 对话（附带文本）。
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

    let llm = LlmClient::from_config(&config.model);
    let mut budget = BudgetGuard::new(config.budget.token_limit, config.budget.cost_limit_usd);
    let mut history: Vec<decide::ChatTurn> = Vec::new();
    let cancel = CancellationToken::new();

    let (bt_tx, mut bt_rx) = mpsc::unbounded_channel::<Backend>();

    // 启动后自动发起首轮决策
    let mut mode = Mode::Idle;
    let mut full_text = String::new();
    start_decision(&gs, &sj, config, &llm, &bt_tx, &history, None, zh);

    state.push_chat(
        MsgRole::System,
        "已连接 Mock。正在分析初始状态…（打字可打断决策）".into(),
    );

    let should_quit = false;

    loop {
        terminal.draw(|f| ui::draw(f, &state))?;

        if should_quit {
            break;
        }

        tokio::select! {
            // 空闲时检查按键（不阻塞消息通道）
            _ = tokio::time::sleep(Duration::from_millis(33)) => {
                if poll(Duration::from_millis(0))? {
                    if let Event::Key(key) = read()? {
                        if key.kind == KeyEventKind::Press {
                            match key.code {
                                KeyCode::Enter => {
                                    let text = state.submit();
                                    if text.is_empty() { continue; }
                                    // 后台用 LLM 解析用户意图
                                    state.push_chat(MsgRole::User, text.clone());
                                    state.progress = Some("理解中…".into());
                                    let llm2 = LlmClient::from_config(&config.model);
                                    let bt_tx2 = bt_tx.clone();
                                    let pending = state.pending_action.clone();
                                    let zh2 = zh;
                                    tokio::spawn(async move {
                                        let intent = llm_parse_intent(&llm2, &text, pending.as_deref(), zh2).await;
                                        let _ = bt_tx2.send(Backend::IntentReady { text, intent });
                                    });
                                }
                                KeyCode::Backspace => { state.backspace(); }
                                KeyCode::Char(c) => { state.input_char(c); }
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
            }
            // 逐条处理 LLM 流式消息——每条 Delta 立即更新 + 下轮重绘 = 逐字流式
            Some(msg) = bt_rx.recv() => {
                let should_quit = handle_backend_msg(
                    msg, &mut state, &mut mode, &mut full_text, config, &llm,
                    &mcp, &bt_tx, &mut history, &mut budget, &cancel, zh, max_turns,
                ).await;
                if should_quit {
                    break;
                }
            }
        }

        let _ = state.finished;
    }

    // 终端恢复
    disable_raw_mode()?;
    execute!(stdout(), LeaveAlternateScreen)?;
    mcp.lock().await.shutdown().await.ok();
    eprintln!("\n对局结束。总用量: {}", budget.summary());
    Ok(())
}

/// 后台消费 LLM 流。
async fn consume_stream(
    mut rx: mpsc::UnboundedReceiver<StreamEvent>,
    bt_tx: mpsc::UnboundedSender<Backend>,
) {
    loop {
        match rx.recv().await {
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
    history: &mut Vec<decide::ChatTurn>,
    budget: &mut BudgetGuard,
    cancel: &CancellationToken,
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
        }
        Backend::IntentReady { text, intent } => {
            state.progress = None;
            if matches!(intent, UserIntent::Quit) {
                return true;
            }
            handle_user_intent(
                &intent, &text, state, mode, cancel, config, llm, mcp, bt_tx, history, budget, zh,
                max_turns, full_text,
            )
            .await;
        }
        Backend::StreamDone => {
            // 分离对话文本与 ACTION 行
            let action_line = full_text
                .lines()
                .find(|l| l.trim_start().to_uppercase().starts_with("ACTION:"))
                .unwrap_or("")
                .to_string();

            // 对话文本 = 去掉 ACTION 行的剩余内容
            let chat_text: String = full_text
                .lines()
                .filter(|l| !l.trim_start().to_uppercase().starts_with("ACTION:"))
                .collect::<Vec<_>>()
                .join("\n")
                .trim()
                .to_string();

            // 对话文本加入历史和面板（非空时）
            if !chat_text.is_empty() {
                state.push_chat(MsgRole::Agent, chat_text.clone());
                history.push(decide::ChatTurn::Assistant(chat_text));
            }

            state.streaming_text.clear();
            state.reasoning_text.clear();

            if !action_line.is_empty() {
                state.pending_action = Some(action_line.clone());
                state.progress = None;
                *mode = Mode::PendingConfirm;
            } else {
                // 纯对话回复，无 ACTION → 回到空闲等待用户
                *mode = Mode::Idle;
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

            if !success {
                state.push_chat(MsgRole::System, "执行失败。".into());
            }

            if budget.is_over_budget() {
                state.finished = true;
                state.progress = Some(format!("预算超限: {}", budget.summary()));
                *mode = Mode::Idle;
                return false;
            }
            if state.current_turn >= max_turns {
                state.finished = true;
                state.progress = Some(format!("已达最大轮数 {max_turns}"));
                *mode = Mode::Idle;
                return false;
            }

            // 执行成功 → 后台取下一帧状态
            *mode = Mode::FetchingState;
            state.progress = Some("取状态中…".into());
            let mcp2 = mcp.clone();
            let bt_tx2 = bt_tx.clone();
            tokio::spawn(async move {
                let mut m = mcp2.lock().await;
                match m.get_game_state("json").await {
                    Ok(s) => {
                        let _ = bt_tx2.send(Backend::StateReady(s));
                    }
                    Err(e) => {
                        let _ = bt_tx2.send(Backend::Error(format!("取状态失败: {e:#}")));
                    }
                }
            });
        }
        Backend::StateReady(sj) => {
            let gs: GameState = serde_json::from_str(&sj).unwrap_or_default();
            state.game_state = gs.clone();

            if gs.state_type == StateType::GameOver || gs.state_type == StateType::Unknown {
                state.finished = true;
                state.progress = Some("游戏结束".into());
                *mode = Mode::Idle;
                return false;
            }

            // 自动发起下一轮决策
            *mode = Mode::Streaming;
            state.progress = Some("LLM 决策中…".into());
            full_text.clear();
            start_decision(&gs, &sj, config, llm, bt_tx, history, None, zh);
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
    cancel: &CancellationToken,
    config: &Config,
    llm: &LlmClient,
    mcp: &Arc<Mutex<McpClient>>,
    bt_tx: &mpsc::UnboundedSender<Backend>,
    history: &mut Vec<decide::ChatTurn>,
    budget: &mut BudgetGuard,
    zh: bool,
    max_turns: u32,
    full_text: &mut String,
) {
    match intent {
        UserIntent::Quit => {
            // Quit 在 handle_backend_msg 已处理，这里不该到达
        }
        UserIntent::Interrupt => {
            cancel.cancel();
            state.push_chat(MsgRole::User, text.to_string());
            state.push_chat(MsgRole::System, "已打断当前决策。".into());
            state.streaming_text.clear();
            *mode = Mode::Idle;
            full_text.clear();
        }
        UserIntent::Confirm => {
            state.push_chat(MsgRole::User, text.to_string());

            // 从 pending_action 或 streaming_text 解析 ACTION
            let action_text = state.pending_action.take().or_else(|| {
                let ft = full_text.clone();
                ft.lines()
                    .find(|l| l.trim_start().to_uppercase().starts_with("ACTION:"))
                    .map(|s| s.to_string())
            });

            let action_line = match action_text {
                Some(a) => a,
                None => {
                    state.push_chat(MsgRole::System, "没有待确认的动作。".into());
                    return;
                }
            };

            let action = match parse::parse_action(&action_line) {
                Ok(a) => a,
                Err(e) => {
                    state.push_chat(MsgRole::System, format!("解析失败: {e:#}"));
                    return;
                }
            };

            // 后台执行
            *mode = Mode::Executing;
            state.progress = Some(format!("执行 {}…", action.tool));
            let mcp2 = mcp.clone();
            let bt_tx2 = bt_tx.clone();
            let tool = action.tool;
            let args = action.args;
            tokio::spawn(async move {
                let mut m = mcp2.lock().await;
                let result = m.call_tool(&tool, args).await;
                let (success, message) = match result {
                    Ok(r) => (true, r.chars().take(120).collect()),
                    Err(e) => (false, format!("{e:#}")),
                };
                let _ = bt_tx2.send(Backend::ExecDone { success, message });
            });
        }
        UserIntent::Reject => {
            state.push_chat(MsgRole::User, text.to_string());
            history.push(decide::ChatTurn::User(text.to_string()));
            state.pending_action = None;
            // 重新发起决策，带上用户的拒绝理由
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
            let gs: GameState = serde_json::from_str(&sj).unwrap_or_default();
            state.game_state = gs.clone();
            *mode = Mode::Streaming;
            full_text.clear();
            start_decision(&gs, &sj, config, llm, bt_tx, history, Some(text), zh);
        }
        UserIntent::Chat(msg) => {
            state.push_chat(MsgRole::User, msg.clone());
            history.push(decide::ChatTurn::User(msg.clone()));
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
            let gs: GameState = serde_json::from_str(&sj).unwrap_or_default();
            state.game_state = gs.clone();
            start_decision(&gs, &sj, config, llm, bt_tx, history, Some(msg), zh);
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

/// 用 LLM 解析用户输入的意图（替代关键词匹配）。
async fn llm_parse_intent(
    llm: &LlmClient,
    text: &str,
    pending_action: Option<&str>,
    zh: bool,
) -> UserIntent {
    let lang = if zh { "请用中文回答。" } else { "" };
    let pending_desc = pending_action.unwrap_or("无");
    let system = sts2_llm::ChatMessage::system(format!(
        "你是一个意图分类器。根据玩家的自然语言输入，判断玩家意图。{lang}\n\n分类规则：\n\
         - CONFIRM: 玩家同意执行当前建议（如\"执行\"\"那就这么做吧\"\"可以\"\"好\"\"继续\"）\n\
         - REJECT: 玩家否决当前建议或要求换一个（如\"不\"\"换一个\"\"不要这样\"\"不好\"）\n\
         - QUIT: 玩家想退出程序（如\"退出\"\"退出吧\"\"退出喵\"\"quit\"\"结束\"）\n\
         - INTERRUPT: 玩家想打断当前正在进行的 LLM 回复（如\"打断\"\"停\"\"stop\"）\n\
         - CHAT: 其他一切情况——玩家在与 Agent 对话、问问题、给策略指令\n\n\
         当前待确认动作: {pending_desc}\n\n\
         只回复分类名称（CONFIRM/REJECT/QUIT/INTERRUPT/CHAT），不要任何其他文字。"
    ));
    let user = sts2_llm::ChatMessage::user(format!("玩家输入: {text}"));

    match llm.chat(&[system, user]).await {
        Ok(resp) => {
            let tag = resp.content.trim().to_uppercase();
            match tag.as_str() {
                "CONFIRM" => UserIntent::Confirm,
                "REJECT" => UserIntent::Reject,
                "QUIT" => UserIntent::Quit,
                "INTERRUPT" => UserIntent::Interrupt,
                _ => UserIntent::Chat(text.to_string()),
            }
        }
        Err(_) => fallback_parse_intent(text),
    }
}

/// 关键词回退（LLM 不可用时）。
fn fallback_parse_intent(text: &str) -> UserIntent {
    let lower = text.to_lowercase();
    let trimmed = text.trim();

    if lower.contains("退出") || lower == "quit" || lower == "exit" {
        return UserIntent::Quit;
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
