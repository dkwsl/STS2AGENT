//! LLM 对话编排：取游戏状态 → 构造 prompt → 调 LLM 流式生成 → 终端输出。
//!
//! LLM 角色 = 牌手顾问 + 对话伙伴（不是自动决策者）：
//! - 理解用户要求、回答策略问题、解释出牌原因、分析不这样出的理由。
//! - 末尾附 ACTION 建议供用户确认，但用户可以拒绝或另提方案。

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

    let messages = build_messages(&state_json, &config.model.model, &[], "", None, zh);
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

/// 构造 LLM 消息：system（角色+规则）+ history（对话历史）+ user（状态+用户消息）。
///
/// `user_msg` = 用户本轮输入的自然语言指令/问题（None = 首轮自动发起）。
pub fn build_messages(
    state_json: &str,
    model: &str,
    history: &[ChatTurn],
    state_summary: &str,
    user_msg: Option<&str>,
    zh: bool,
) -> Vec<ChatMessage> {
    let lang = if zh {
        "\n\n请用中文（简体）思考并回答，包括思考过程在内的所有输出都使用中文。"
    } else {
        ""
    };
    let system = ChatMessage::system(format!(
        r#"你是《杀戮尖塔2》的牌手顾问和对话伙伴（模型: {model}）。
你的职责是：
1. 分析当前游戏状态，给出最优行动**建议**（不是命令）。
2. 回答玩家关于策略的问题（"为什么打这张牌""不这样出会怎样"等）。
3. 理解并执行玩家的自然语言指令（"先打小怪""去商店""用药水"等），把指令翻译成具体动作。
4. 如果玩家否决了你的建议，理解原因并给出替代方案。

游戏动作规则（工具名必须严格按以下拼写，参数名也要精确匹配）：
- 战斗(monster/elite/boss): 出牌 combat_play_card(card_index, target) / 用药水 use_potion(slot, target) / 结束回合 combat_end_turn()。从右到左出牌以保持索引稳定。单体牌需 target = 敌人 entity_id（如 JAW_WORM_0）。
- 战斗选牌(hand_select): 选牌 combat_select_card(card_index) / 确认 combat_confirm_selection()。
- 地图(map): 选择节点 map_choose_node(node_index)。
- 奖励(rewards): 领取 rewards_claim(reward_index) / 去地图 proceed_to_map()。
- 卡牌奖励(card_reward): 选卡 rewards_pick_card(card_index) / 跳过 rewards_skip_card()。
- 休息点(rest_site): 选择 rest_choose_option(option_index) / 去地图 proceed_to_map()。
- 商店(shop/fake_merchant): 购买 shop_purchase(item_index) / 去地图 proceed_to_map()。
- 事件(event): 选择 event_choose_option(option_index) / 推进对话 event_advance_dialogue()。
- 卡牌选择(card_select): 选牌 deck_select_card(card_index) / 确认 deck_confirm_selection() / 取消 deck_cancel_selection()。
- 遗物选择(relic_select): 选遗物 relic_select(relic_index) / 跳过 relic_skip()。
- 宝箱(treasure): 领取 treasure_claim_relic(relic_index) / 去地图 proceed_to_map()。
- 菜单/游戏结束(menu/game_over): menu_select(option)。

回复格式要求：
- 先用自然语言与玩家对话、解释你的分析。
- 如果有行动建议，在回复的**最后一行**写上 ACTION 行，格式如下：
  ACTION: <tool_name> | <param>=<value> | ...
- 如果玩家问的是纯策略问题且没有需要执行的即时动作，可以不附 ACTION 行。
- ACTION 行只是**建议**，玩家会确认后才执行。{lang}"#
    ));

    let mut msgs = vec![system];

    // 对话历史
    for turn in history {
        match turn {
            ChatTurn::User(t) => msgs.push(ChatMessage::user(t.clone())),
            ChatTurn::Assistant(t) => msgs.push(ChatMessage::assistant(t.clone())),
        }
    }

    // 当前状态 + 用户消息
    let user_content = {
        let state_part = if state_summary.is_empty() {
            format!("当前游戏状态:\n```json\n{state_json}\n```")
        } else {
            format!("当前状态: {state_summary}\n\n完整状态 JSON:\n```json\n{state_json}\n```")
        };
        match user_msg {
            Some(msg) if !msg.is_empty() => {
                format!("{state_part}\n\n玩家说: {msg}\n\n请回应玩家的问题或指令。如果有行动建议，在最后一行附 ACTION。")
            }
            _ => {
                format!("{state_part}\n\n请分析当前局面并给出行动建议。如果有行动建议，在最后一行附 ACTION。")
            }
        }
    };
    msgs.push(ChatMessage::user(user_content));
    msgs
}

/// 对话历史中的一轮。
#[derive(Debug, Clone)]
pub enum ChatTurn {
    User(String),
    Assistant(String),
}
