//! LLM 对话编排：取游戏状态 → 构造 prompt → 调 LLM 流式生成 → 终端输出。
//!
//! LLM 角色 = 牌手顾问 + 对话伙伴（不是自动决策者）：
//! - 理解用户要求、回答策略问题、解释出牌原因、分析不这样出的理由。
//! - 末尾附 ACTION 建议供用户确认，但用户可以拒绝或另提方案。

use anyhow::Result;
use std::io::Write;

use sts2_core::{Config, GameState};
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
    eprintln!("[诊断] get_game_state 返回 {} 字节", state_json.len());
    if state_json.len() < 800 {
        eprintln!("[诊断] 完整内容:\n{state_json}");
    } else {
        eprintln!(
            "[诊断] 前 800 字符:\n{}",
            state_json.chars().take(800).collect::<String>()
        );
    }
    let gs: GameState = serde_json::from_str(&state_json).unwrap_or_default();
    mcp.shutdown().await.ok();

    eprintln!("[3/3] 请求 LLM 决策...");
    let llm = LlmClient::from_config(&config.model);
    let mut budget = BudgetGuard::new(config.budget.token_limit, config.budget.cost_limit_usd);

    let game_knowledge =
        crate::knowledge::search_game_knowledge(&gs, &config.storage.game_knowledge_dir);

    let messages = build_messages(
        &state_json,
        &config.model.model,
        &[],
        "",
        None,
        false,
        None,
        if game_knowledge.is_empty() {
            None
        } else {
            Some(&game_knowledge)
        },
        None,
        zh,
    );
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
#[allow(clippy::too_many_arguments)]
pub fn build_messages(
    state_json: &str,
    model: &str,
    history: &[ChatTurn],
    state_summary: &str,
    user_msg: Option<&str>,
    auto_mode: bool,
    task: Option<&str>,
    game_knowledge: Option<&str>,
    session_notes: Option<&str>,
    zh: bool,
) -> Vec<ChatMessage> {
    let lang = if zh {
        "\n\n请用中文（简体）思考并回答，包括思考过程在内的所有输出都使用中文。"
    } else {
        ""
    };
    let system = ChatMessage::system(format!(
        r#"你是《杀戮尖塔2》的牌手顾问（模型: {model}）。职责：分析局面，给出结论和打法，回答策略问题，翻译玩家指令为动作。

执行权限——绝对规则：
- 无玩家明确指令时禁止操作游戏、禁止输出 ACTION 行。
- 禁止根据"上一轮的自主模式""对话历史"推断玩家想操作——除非玩家本轮明确说了。
- 只给文字建议时不能带 ACTION 行，除非：
  A. 玩家本轮说了具体操作指令（如"出第二张牌""结束回合""去商店""执行"）。
  B. 玩家本轮明确说了"自己打"（如"自己打""自己打这层""自己打这局"）——唯一触发自主模式的指令。
- 不确定玩家是否在下达指令时，当对话回复，不附 ACTION。
- 自主模式连续操作直到完成或玩家喊停；非自主模式每次只执行一次，执行完即停。

战斗决策：
- 出牌前分析手牌、敌人意图、能量。不要空过回合。
- 考虑斩杀线：能杀则不防御直接输出。
- 攻击意图优先防御，Buff/Debuff/Sleep 意图优先输出。

明确指令（触发执行）："出第二张牌" "结束回合" "自己打" "自己打这层" "执行" "就这样做"
非明确指令（只对话）："你觉得呢" "为什么" "分析一下" "你来吧" "交给你" "自动打"

游戏动作规则（工具名严格按拼写）：
- 战斗: combat_play_card(card_index, target) / use_potion(slot, target) / combat_end_turn()
  AnyEnemy 的牌必填 target=敌人 entity_id（如 JAW_WORM_0）。Self/None 的牌不带 target。
- 地图: map_choose_node(node_index)
- 奖励: rewards_claim(reward_index) / 卡牌奖励: rewards_pick_card(card_index) / rewards_skip_card()
- 卡牌选择: deck_select_card(card_index) / deck_confirm_selection() / deck_cancel_selection()
  选牌后必须确认：deck_select_card → can_confirm=true → deck_confirm_selection。
- 事件: event_choose_option(option_index) / event_advance_dialogue()
- 休息点: rest_choose_option(option_index) / 商店: shop_purchase(item_index)
- 遗物选择: relic_select(relic_index) / relic_skip() / 宝箱: treasure_claim_relic(relic_index)
- 菜单/游戏结束: menu_select(option)
- 完成当前屏幕操作后检查 can_proceed=true 则 proceed_to_map()。rewards/rest_site/shop/treasure 操作完通常需要 proceed。

卡组信息：player.draw_pile 是牌组剩余牌，player.discard_pile 是弃牌堆，player.hand 是手牌。始终关注牌组整体。

回复格式——绝对规则：
- 禁止使用任何 Markdown 语法（不要用 # 标题、**加粗**、- 列表、`代码块`、> 引用等）。界面无法渲染 Markdown，会原样显示符号。
- 用纯文本回复，极简：只输出分析、结论、打法，不废话、不寒暄、不复述状态。
- 回复控制在 5 句以内。能一句话说清就一句话。
- 如果有行动，附 ACTION 行（可多行）：ACTION: <tool_name> | <param>=<value>
- 如果状态是 unknown，直接说"等待游戏加载"。
- 你可以写 NOTE: <内容> 行来记录当前对局的经验教训（如"Jaw Worm 低血量会狂暴""这把缺防御"）。只在对局中有重要发现时才写 NOTE。{lang}"#
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
    let task_part = match task {
        Some(t) if !t.is_empty() => format!("\n\n当前任务: {t}\n思考: 当前状态离完成任务还差什么？下一步做什么能推进任务？\n有些操作需要确认（如选完角色后需要点 confirm 开始游戏，选完遗物后需要 proceed）。注意当前 state_type 是什么，检查是否需要确认/推进操作。"),
        _ => String::new(),
    };
    let game_knowledge_part = match game_knowledge {
        Some(g) if !g.is_empty() => {
            format!("\n\n游戏数据参考（从反编译数据生成，以当前游戏状态 JSON 为准）:\n{g}")
        }
        _ => String::new(),
    };
    let notes_part = match session_notes {
        Some(n) if !n.is_empty() => format!("\n\n往期经验:\n{n}"),
        _ => String::new(),
    };
    let user_content = {
        let state_part = if state_summary.is_empty() {
            format!("当前游戏状态:\n```json\n{state_json}\n```")
        } else {
            format!("当前状态: {state_summary}\n\n完整状态 JSON:\n```json\n{state_json}\n```")
        };
        match user_msg {
            Some(msg) if !msg.is_empty() => {
                format!("{state_part}{game_knowledge_part}{notes_part}{task_part}\n\n玩家说: {msg}\n\n请回应玩家的问题或指令。如果玩家给的是操作指令，给出 ACTION 行。")
            }
            _ => {
                if auto_mode {
                    format!("{state_part}{game_knowledge_part}{notes_part}{task_part}\n\n玩家说了「自己打」，已进入自主模式，你被授权连续操作游戏。请分析当前局面并直接给出 ACTION 行（会自动执行），直到任务完成或玩家喊停。")
                } else {
                    format!("{state_part}{game_knowledge_part}\n\n请分析当前局面，给出行动建议。注意：不要输出 ACTION 行，只给文字建议。")
                }
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
