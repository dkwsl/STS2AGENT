//! TUI 异步事件循环——自然语言对话模式。
//!
//! 流程：
//! 1. 取状态 → LLM 生成建议（流式，用户打字即打断）
//! 2. 建议出来后不自动执行，等用户确认
//! 3. 用户输入解析意图：执行 / 拒绝 / 对话 / 打断
//! 4. 确认 → MCP 执行 → 取下一状态 → 循环
//!
//! 模块划分：
//! - `stream`：LLM 决策的发起/消费/打断 + 状态轮询
//! - `backend`：后台消息（流结束/执行完成/状态变化）的编排
//! - `intent`：用户意图分类与处理
//! - `actions`：反射动作 / 后台 MCP 执行 / 知识库查询拦截

mod actions;
mod backend;
mod intent;
mod stream;

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

use sts2_agent::decide;
use sts2_agent::storage::{Session, SessionStore};
use sts2_core::{Config, GameState};
use sts2_llm::{BudgetGuard, LlmClient, Usage};
use sts2_mcp::McpClient;

use crate::app::{AppState, Mode, MsgRole};

use intent::parse_intent;
use stream::abort_current_llm;

/// 后台任务 → 主循环的消息。
enum Backend {
    StateReady(String),
    Delta(String),
    Reasoning(String),
    Usage(Usage),
    StreamDone {
        tool_calls: Vec<sts2_llm::ToolCall>,
    },
    StreamError(String),
    ExecDone {
        success: bool,
        message: String,
    },
    IntentReady {
        text: String,
        intent: intent::UserIntent,
    },
    Error(String),
}

pub async fn run(
    config: &Config,
    use_mock: bool,
    show_thinking: bool,
    zh: bool,
    _auto_play: bool,
    max_turns: u32,
    resume_id: Option<String>,
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

    // 会话存储（R5）；--load <id> 时恢复历史上下文继续
    let store = SessionStore::from_dir(&config.storage.sessions_dir);
    let mut session = match &resume_id {
        Some(id) => {
            let mut s = store
                .load(id)
                .map_err(|e| anyhow::anyhow!("加载会话 {id} 失败: {e:#}"))?;
            s.finished = false;
            // 恢复预算累计（预算守卫跨会话继续）
            budget.restore(s.total_input, s.total_output, s.total_cost);
            state.total_input = s.total_input;
            state.total_output = s.total_output;
            state.total_cost = s.total_cost;
            // 恢复对话历史（LLM 上下文）与 UI 面板
            for t in &s.turns {
                if let Some(u) = &t.user_input {
                    history.push(decide::ChatTurn::User(u.clone()));
                    state.push_chat(crate::app::MsgRole::User, u.clone());
                }
                if !t.agent_text.is_empty() {
                    history.push(decide::ChatTurn::Assistant(t.agent_text.clone()));
                    state.push_chat(crate::app::MsgRole::Agent, t.agent_text.clone());
                }
            }
            let n = s.turns.len();
            state.current_turn = n as u32;
            s
        }
        None => Session::new(&config.model.model),
    };

    // 无后台状态轮询：状态读取只由两个入口触发——
    // 1) 用户让 agent 分析时（意图处理里 GET 一次）
    // 2) 自主模式执行完动作后（等状态稳定再 GET，喂给 LLM 循环决策）

    let mut mode = Mode::Idle;
    let mut full_text = String::new();
    let mut pending_actions: Vec<sts2_agent::parse::ParsedAction> = Vec::new();

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
                            // 自主模式开启请求待确认：y/n 直接判定，其他文字取消询问转普通对话
                            if state.pending_auto_start.is_some() {
                                let lower = text.trim().to_lowercase();
                                let confirm = matches!(
                                    lower.as_str(),
                                    "y" | "yes" | "好" | "同意" | "可以" | "确定" | "开" | "开吧"
                                );
                                let reject = matches!(
                                    lower.as_str(),
                                    "n" | "no" | "不" | "不要" | "拒绝" | "取消" | "不行"
                                );
                                if confirm || reject {
                                    state.push_chat(MsgRole::User, text.clone());
                                    backend::resolve_auto_start(
                                        confirm,
                                        &mut state,
                                        &mut mode,
                                        &mut full_text,
                                        &mcp,
                                        &bt_tx,
                                    );
                                    continue;
                                }
                                if lower == "停" || lower == "stop" || lower == "interrupt" {
                                    // 打断：取消询问即可
                                    state.pending_auto_start = None;
                                    state.push_chat(MsgRole::User, text.clone());
                                    state.push_chat(MsgRole::System, "已取消自主模式确认。".into());
                                    mode = Mode::Idle;
                                    state.progress = None;
                                    continue;
                                }
                                // 其他文字：取消询问，落入普通对话流程
                                state.pending_auto_start = None;
                                state.push_chat(
                                    MsgRole::System,
                                    "已取消自主模式确认，按普通对话处理。".into(),
                                );
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
                        KeyCode::Char('t') => {
                            state.toggle_thinking();
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
            let quit = backend::handle_backend_msg(
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
            terminal.draw(|f| crate::ui::draw(f, &state))?;
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
