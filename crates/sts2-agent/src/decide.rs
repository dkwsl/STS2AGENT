//! LLM 决策编排：取游戏状态 → 构造 prompt → 调 LLM 流式生成 → 终端输出。

use anyhow::Result;
use std::io::Write;

use sts2_core::Config;
use sts2_llm::{BudgetGuard, ChatMessage, LlmClient, StreamEvent};
use sts2_mcp::McpClient;

/// 一次决策：取状态 → LLM 流式输出 → 打印。`show_thinking` 控制是否打印 reasoning。
pub async fn run_decide(
    config: &Config,
    use_mock: bool,
    show_thinking: bool,
    zh: bool,
) -> Result<()> {
    let (command, args) = if use_mock {
        ("./target/debug/sts2-mcp-mock".to_string(), Vec::new())
    } else {
        (config.mcp.command.clone(), config.mcp.args.clone())
    };

    eprintln!(
        "[1/3] 连接 MCP server ({})...",
        if use_mock { "mock" } else { "real" }
    );
    let mut mcp = McpClient::spawn(&command, &args)?;
    mcp.initialize().await?;

    eprintln!("[2/3] 获取游戏状态...");
    let state_json = mcp.get_game_state("json").await?;
    mcp.shutdown().await.ok();

    eprintln!("[3/3] 请求 LLM 决策...");
    let llm = LlmClient::from_config(&config.model);
    let mut budget = BudgetGuard::new(config.budget.token_limit, config.budget.cost_limit_usd);

    let messages = build_messages(&state_json, &config.model.model, &[], "", zh);
    let mut rx = llm.chat_stream(&messages)?;

    println!("--- LLM 决策 ---");
    while let Some(ev) = rx.recv().await {
        match ev {
            StreamEvent::Delta(text) => {
                print!("{text}");
                std::io::stdout().flush().ok();
            }
            StreamEvent::Reasoning(text) if show_thinking => {
                eprint!("{text}");
                std::io::stderr().flush().ok();
            }
            StreamEvent::Usage(u) => {
                budget.record(&u, config.model.price_in, config.model.price_out);
                eprintln!("\n--- 用量: {} ---", budget.summary());
            }
            StreamEvent::Done => break,
            StreamEvent::Error(e) => {
                eprintln!("\n[错误] {e}");
                break;
            }
            _ => {}
        }
    }
    println!();
    Ok(())
}

/// 构造 LLM 消息：system（规则）+ history（上轮摘要）+ user（当前状态）。
pub(crate) fn build_messages(
    state_json: &str,
    model: &str,
    history: &[String],
    state_summary: &str,
    zh: bool,
) -> Vec<ChatMessage> {
    let lang = if zh {
        "\n\n请用中文（简体）思考并回答，包括思考过程在内的所有输出都使用中文。"
    } else {
        ""
    };
    let system = ChatMessage::system(format!(
        r#"You are an expert Slay the Spire 2 decision agent (model: {model}).
Given the current game state JSON, decide the single best action to take now.

Rules:
- Combat (state_type monster/elite/boss): play a card (combat_play_card, card_index + target for single-target), use a potion (use_potion), or end the turn (combat_end_turn). Play cards right-to-left to keep indices stable. Single-target cards need target = enemy entity_id (e.g. JAW_WORM_0).
- Map (state_type map): choose a node (map_choose_node, node_index).
- Rewards: claim (rewards_claim, reward_index) or proceed (proceed_to_map).
- Rest site: rest or smith (rest_choose_option, option_index).
- Shop: buy (shop_purchase, item_index) or proceed (proceed_to_map).
- Event: choose an option (choose_event_option, option_index).

Respond in exactly this format:
ACTION: <tool_name> | <param>=<value> | ...
REASON: <one or two sentences>{lang}"#
    ));
    let mut msgs = vec![system];
    for h in history {
        msgs.push(ChatMessage::assistant(h.clone()));
        msgs.push(ChatMessage::user("What's the next action?"));
    }
    let user_content = if state_summary.is_empty() {
        format!("Current game state:\n```json\n{state_json}\n```\n\nWhat is the best action to take right now?")
    } else {
        format!("Current state: {state_summary}\n\nFull state JSON:\n```json\n{state_json}\n```\n\nWhat is the best action to take right now?")
    };
    msgs.push(ChatMessage::user(user_content));
    msgs
}
