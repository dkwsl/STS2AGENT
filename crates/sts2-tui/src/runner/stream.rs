//! LLM 流生命周期：发起决策、消费流、打断、后台状态轮询。

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use sts2_agent::decide;
use sts2_core::{Config, GameState};
use sts2_llm::{LlmClient, StreamEvent};

use crate::app::{AppState, MsgRole};

use super::Backend;

/// 发起一次 LLM 决策（流式）：拼 prompt（含知识库注入 + 查询记录 + 笔记）并后台消费。
#[allow(clippy::too_many_arguments)]
pub(super) fn start_decision(
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
    let summary = crate::app::state_summary(gs);

    // game-knowledge 结构化索引检索 + 本次任务的主动查询结果
    let mut game_knowledge =
        sts2_agent::knowledge::search_game_knowledge(gs, &config.storage.game_knowledge_dir);
    if !state.lookup_context.is_empty() {
        game_knowledge.push_str("\n=== 知识库查询记录 ===\n");
        game_knowledge.push_str(&state.lookup_context);
    }

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
    match llm.chat_stream(&messages, Some(decide::tool_definitions())) {
        Ok(rx) => {
            let bt_tx2 = bt_tx.clone();
            tokio::spawn(async move {
                consume_stream(rx, bt_tx2, cancel).await;
            });
            if state.progress.is_none() {
                state.progress = Some("分析中…".into());
            }
        }
        Err(e) => {
            state.push_chat(MsgRole::System, format!("LLM 启动失败: {e:#}"));
        }
    }
}

/// 打断当前 LLM 流：cancel + 排空 stale 消息 + 固化思考（浅色保留）+ 清空流式文本。
pub(super) fn abort_current_llm(
    state: &mut AppState,
    bt_rx: &mut mpsc::UnboundedReceiver<Backend>,
    full_text: &mut String,
) {
    state.current_cancel.cancel();
    // 排空所有 stale 消息
    while bt_rx.try_recv().is_ok() {}
    if !state.reasoning_text.is_empty() {
        let t = std::mem::take(&mut state.reasoning_text);
        state.push_chat(crate::app::MsgRole::Thinking, t);
    }
    state.streaming_text.clear();
    full_text.clear();
}

/// 后台消费 LLM 流，推回主循环。cancel 用于打断。
pub(super) async fn consume_stream(
    mut rx: mpsc::UnboundedReceiver<StreamEvent>,
    bt_tx: mpsc::UnboundedSender<Backend>,
    cancel: CancellationToken,
) {
    let mut tool_calls: Vec<sts2_llm::ToolCall> = Vec::new();
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
                    Some(StreamEvent::ToolCall(tc)) => {
                        tool_calls.push(tc);
                    }
                    Some(StreamEvent::Done) => {
                        let _ = bt_tx.send(Backend::StreamDone { tool_calls });
                        return;
                    }
                    Some(StreamEvent::Error(e)) => {
                        let _ = bt_tx.send(Backend::StreamError(e));
                        return;
                    }
                    None => {
                        let _ = bt_tx.send(Backend::StreamDone { tool_calls });
                        return;
                    }
                }
            }
        }
    }
}
