//! LLM 决策编排：取游戏状态 → 构造 prompt → 调 LLM 流式生成 → 终端输出。
//! 这是第一个可运行成果：Mock 状态 → LLM 决策 + 解释 + token/成本。

use anyhow::Result;
use std::io::Write;

use sts2_core::Config;
use sts2_llm::{BudgetGuard, ChatMessage, LlmClient, StreamEvent};
use sts2_mcp::McpClient;

pub async fn run_decide(config: &Config, use_mock: bool) -> Result<()> {
    let (command, args) = if use_mock {
        ("./target/debug/sts2-mcp-mock".to_string(), Vec::new())
    } else {
        (config.mcp.command.clone(), config.mcp.args.clone())
    };

    // 1. 连接 MCP
    eprintln!(
        "[1/3] 连接 MCP server ({})...",
        if use_mock { "mock" } else { "real" }
    );
    let mut mcp = McpClient::spawn(&command, &args)?;
    mcp.initialize().await?;

    // 2. 取状态
    eprintln!("[2/3] 获取游戏状态...");
    let state_json = mcp.get_game_state("json").await?;
    mcp.shutdown().await.ok();

    // 3. 调 LLM
    eprintln!("[3/3] 请求 LLM 决策...");
    let llm = LlmClient::from_config(&config.model);
    let mut budget = BudgetGuard::new(config.budget.token_limit, config.budget.cost_limit_usd);

    let messages = build_messages(&state_json, &config.model.model);
    let mut rx = llm.chat_stream(&messages)?;

    println!("--- LLM 决策 ---");
    while let Some(ev) = rx.recv().await {
        match ev {
            StreamEvent::Delta(text) => {
                print!("{text}");
                std::io::stdout().flush().ok();
            }
            StreamEvent::Reasoning(text) => {
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
        }
    }
    println!();
    Ok(())
}

fn build_messages(state_json: &str, model: &str) -> Vec<ChatMessage> {
    let system = ChatMessage::system(format!(
        r#"You are an expert Slay the Spire 2 decision agent (model: {model}).
Given the current game state JSON, decide the single best action to take now.

Rules:
- Combat (state_type monster/elite/boss): play a card (combat_play_card, card_index + target for single-target), use a potion (use_potion), or end the turn (combat_end_turn). Play cards right-to-left to keep indices stable. Single-target cards need target = enemy entity_id (e.g. JAW_WORM_0).
- Map (state_type map): choose a node (map_choose_node, node_index).
- Rewards: claim (rewards_claim, reward_index) or proceed (proceed_to_map).
- Rest site: rest or smith (rest_choose_option, option_index).
- Shop: buy (shop_purchase, item_index) or proceed.
- Event: choose an option (choose_event_option, option_index).

Respond in exactly this format:
ACTION: <tool_name> | <param>=<value> | ...
REASON: <one or two sentences>"#
    ));
    let user = ChatMessage::user(format!(
        "Current game state:\n```json\n{state_json}\n```\n\nWhat is the best action to take right now?"
    ));
    vec![system, user]
}
