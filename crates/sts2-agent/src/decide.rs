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
        None,
        &[],
        zh,
    );
    let mut rx = llm.chat_stream(&messages, Some(tool_definitions()))?;

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
            StreamEvent::ToolCall(tc) => {
                println!("\n[工具调用] {}({})", tc.name, tc.arguments);
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
    plan: Option<&str>,
    recent_actions: &[String],
    zh: bool,
) -> Vec<ChatMessage> {
    let lang = if zh {
        "\n\n请用中文（简体）思考并回答，包括思考过程在内的所有输出都使用中文。"
    } else {
        ""
    };
    let system = ChatMessage::system(format!(
        r#"你是《杀戮尖塔2》的牌手顾问（模型: {model}）。职责：分析局面，给出结论和打法，回答策略问题，翻译玩家指令为动作。

执行权限——绝对规则（内核强制门禁，违反会被否决）：
- 你没有直接操作游戏的权限。一切游戏操作工具调用只有在自主模式开启时才会被执行；非自主模式下发起的游戏操作会被系统无条件否决。
- 玩家说"自己打""自己打这层""自己打这局"等自主指令时：调用 auto_start 工具（task=任务描述）。系统会开启自主模式并让你逐步执行（每轮给你最新状态，你给出下一步操作，直到任务完成）。
- 玩家给单次操作指令（如"出第二张牌""结束回合""执行"）时：依次调用 auto_start（task=指令）→ 该操作 → auto_stop 三个工具。
- 任务完成或需要停止时：调用 auto_stop（自主模式内的每一轮都如此，完成即关）。
- 纯对话/分析（玩家没让操作）时不发起任何工具调用。
- 禁止根据"上一轮的自主模式""对话历史"推断玩家想操作——除非玩家本轮明确说了。
- lookup 查询不受自主模式限制，随时可用。

战斗决策：
- 出牌前分析手牌、敌人意图、能量。不要空过回合。
- 考虑斩杀线：能杀则不防御直接输出。但不贪输出：已确定能获胜的战斗，优先用防御/低费牌减少战损（掉血、消耗资源），不追求多余的伤害或最快的击杀。
- 攻击意图优先防御，Buff/Debuff/Sleep 意图优先输出。

策略记忆——跨回合计划（PLAN）：
- 需要多步推进时（战斗连招、多回合规划），输出一行 PLAN: <计划>（如"PLAN: 先压血到16，下回合痛击+打击斩杀"）。计划变化时输出新的 PLAN 覆盖，未变化不必重复。
- 系统会在每轮把你的 PLAN 与最近已执行操作回显给你，作为跨回合记忆；不要重复已执行的操作。

思考纪律——保持推理简短流程化：
- 思考按固定流程，不超过 6 步：① 当前目标/局面（1 句）→ ② 列 2-3 个候选方案 → ③ 每个方案一句话算清关键数值 → ④ 选定 + 一句理由。
- 禁止冗长的内心独白：不复述状态 JSON、不逐张枚举无关手牌、不反复推翻自己已经得出的结论。
- 拿牌/路线等规划决策同样走该流程，数值只算关键项（能打到多少、挨多少、差多少）。

表达与决策习惯：
- 不要使用"最优""最好""最佳"等绝对性词语——局势评估有不确定性，用"更倾向""理由是"这类表述并给出依据。
- 拿牌/买牌决策不能只看单卡输出：先分析当前牌组缺什么（防御/过牌/续航/成长）、已经拿了什么、以及这张卡和现有牌组的配合度；同时说明不适合拿的牌及原因。
- 前期决策要有全局视角：考虑后期打法（牌组成型方向、血量控制、金币规划、精英/Boss 压力），为后续留出容错空间，不为眼前小利透支后期。

事实来源——防幻觉规则：
- 卡牌/遗物/药水/敌人的效果一律以状态 JSON 里的 description 字段为准；知识库参考只是辅助，两者冲突时信 description。
- 不确定某机制时就直说"不确定"，禁止根据杀戮尖塔1的经验推测杀戮尖塔2的机制——这是两代游戏，数值和规则不同。
- 数字（伤害/格挡/费用）只引用状态 JSON 里可见的，不要编造。
- 数值计算——非常重要：状态 JSON 中的数值已经计入力量、易伤、虚弱、格挡等 buff 的影响（如敌人意图的攻击力、卡牌的实际伤害）。直接使用 JSON 里的数字做决策，禁止再自行加减 buff 修正重复计算。

知识库主动查询——硬性要求，不是可选项：
- 遇到不认识的卡牌、敌人、遗物、药水、事件，或对某个对象的效果/行为不确定时，必须先输出查询动作再决策（查询不操作游戏）：
  ACTION: lookup | query=<名称或内部ID>
- 特别地：游戏数据参考中标注「知识库未收录」的手牌，若其 description 不足以判断用法，必须查询或明说"不确定"，禁止直接给出打法。
- 优先用英文内部 ID 查询（状态 JSON 的 id 字段）；查不到再用显示名——系统会自动转换。
- 查询分两级：先查本地知识库表格；本地未命中自动查游戏内 Wiki（卡牌/遗物数据，含升级变体）。两级都无记录才会返回"无记录"。
- 查询结果会自动附在下一轮的"知识库查询记录"里，届时继续完成任务。
- 同一对象不要重复查询；每次任务最多查 3 次，用完就基于现有信息决策。

操作规则（工具的详细参数见工具定义）：
- 模式切换（本地请求，不发给游戏）: auto_start 开启自主模式 / auto_stop 关闭自主模式
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
- 操作游戏一律通过工具调用（tool call）发起，不要把操作写进文本。仅当工具调用不可用时才用 ACTION: <tool_name> | <param>=<value> 行代替（此时 auto_start/auto_stop 也按此格式）。
- 如果状态是 unknown，直接说"等待游戏加载"。
- 你可以写 NOTE: <内容> 行来记录当前对局的经验教训（如"Jaw Worm 低血量会狂暴""这把缺防御"）。只在对局中有重要发现时才写 NOTE。{lang}"#
    ));

    let mut msgs = vec![system];

    // 对话历史：只保留最近 10 条（控制上下文增长，前缀稳定部分利于缓存）
    let recent_start = history.len().saturating_sub(10);
    for turn in &history[recent_start..] {
        match turn {
            ChatTurn::User(t) => msgs.push(ChatMessage::user(t.clone())),
            ChatTurn::Assistant(t) => msgs.push(ChatMessage::assistant(t.clone())),
        }
    }

    // 当前状态（瘦身：剥离 keywords / null 字段）+ 用户消息
    let state_json = &crate::slim::slim_state_json(state_json);
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
    let mut plan_part = String::new();
    if let Some(p) = plan {
        if !p.is_empty() {
            plan_part.push_str(&format!(
                "\n\n当前计划（你此前声明，若已过时请在 PLAN 中更新）: {p}"
            ));
        }
    }
    if !recent_actions.is_empty() {
        plan_part.push_str(&format!(
            "\n\n最近已执行的操作（不要重复执行）: {}",
            recent_actions.join(" → ")
        ));
    }
    let user_content = {
        let state_part = if state_summary.is_empty() {
            format!("当前游戏状态:\n```json\n{state_json}\n```")
        } else {
            format!("当前状态: {state_summary}\n\n完整状态 JSON:\n```json\n{state_json}\n```")
        };
        match user_msg {
            Some(msg) if !msg.is_empty() => {
                format!("{state_part}{game_knowledge_part}{notes_part}{plan_part}{task_part}\n\n玩家说: {msg}\n\n请回应玩家的问题或指令。如果玩家要操作游戏，通过工具调用发起（自主模式规则见系统提示）。")
            }
            _ => {
                if auto_mode {
                    format!("{state_part}{game_knowledge_part}{notes_part}{plan_part}{task_part}\n\n你正处于自主模式，游戏操作工具调用会被执行。尽量一次性给出本回合的全部操作（可连续多个工具调用，如出多张牌后结束回合），减少往返；只有当需要看到操作结果才能决定下一步时才停下等下一轮。任务全部完成时才调用 auto_stop；未完成绝不调用。不要只给文字分析而不给操作。")
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

/// 工具定义（OpenAI 兼容 tools 数组）：游戏操作 + 本地请求（lookup/auto）。
/// 与 parse.rs 的 normalize_tool 工具名对齐。
pub fn tool_definitions() -> serde_json::Value {
    use serde_json::json;

    let f = |name: &str, desc: &str, params: serde_json::Value| {
        json!({
            "type": "function",
            "function": {
                "name": name,
                "description": desc,
                "parameters": params,
            }
        })
    };
    let obj = |props: serde_json::Value, required: &[&str]| {
        let mut p = json!({ "type": "object", "properties": props });
        if !required.is_empty() {
            p["required"] = json!(required);
        }
        p
    };
    let int = |d: &str| json!({ "type": "integer", "description": d });
    let s = |d: &str| json!({ "type": "string", "description": d });

    json!([
        f("lookup", "查询本地知识库/游戏Wiki获取卡牌、敌人、遗物、药水、事件的信息。不操作游戏，随时可用。",
            obj(json!({ "query": s("名称或内部ID，优先英文ID（状态JSON的id字段）") }), &["query"])),
        f("auto_start", "开启自主模式（玩家说\"自己打\"或单次操作指令时调用）。task 填任务描述。",
            obj(json!({ "task": s("任务描述，如：自己打这层 / 出第二张牌") }), &["task"])),
        f("auto_stop", "关闭自主模式。仅当任务已全部完成时调用。", obj(json!({}), &[])),
        f("combat_play_card", "战斗：出手牌。target_type=AnyEnemy 的牌必须带 target（敌人 entity_id，如 JAW_WORM_0）。",
            obj(json!({ "card_index": int("手牌索引"), "target": s("敌人entity_id，如 JAW_WORM_0") }), &["card_index"])),
        f("combat_end_turn", "战斗：结束回合。", obj(json!({}), &[])),
        f("use_potion", "战斗：使用药水。slot 是药水槽索引；单体药水须带 target。",
            obj(json!({ "slot": int("药水槽索引"), "target": s("敌人entity_id") }), &["slot"])),
        f("discard_potion", "战斗：丢弃药水。", obj(json!({ "slot": int("药水槽索引") }), &["slot"])),
        f("map_choose_node", "地图：选择下一个节点。", obj(json!({ "node_index": int("节点索引") }), &["node_index"])),
        f("rewards_claim", "奖励屏：领取奖励（从右到左领避免索引漂移）。", obj(json!({ "reward_index": int("奖励索引") }), &["reward_index"])),
        f("rewards_pick_card", "卡牌奖励：选一张卡。", obj(json!({ "card_index": int("卡牌索引") }), &["card_index"])),
        f("rewards_skip_card", "卡牌奖励：跳过。", obj(json!({}), &[])),
        f("deck_select_card", "卡牌选择屏：选牌。选后若 can_confirm=true 必须 deck_confirm_selection。",
            obj(json!({ "card_index": int("卡牌索引") }), &["card_index"])),
        f("deck_confirm_selection", "卡牌选择屏：确认。", obj(json!({}), &[])),
        f("deck_cancel_selection", "卡牌选择屏：取消。", obj(json!({}), &[])),
        f("event_choose_option", "事件：选择选项（含 Proceed）。", obj(json!({ "option_index": int("选项索引") }), &["option_index"])),
        f("event_advance_dialogue", "事件：推进对话。", obj(json!({}), &[])),
        f("rest_choose_option", "休息点：选择（休息/锻造等）。", obj(json!({ "option_index": int("选项索引") }), &["option_index"])),
        f("shop_purchase", "商店：购买商品。", obj(json!({ "item_index": int("商品索引") }), &["item_index"])),
        f("proceed_to_map", "当前屏幕操作完成且回到地图（can_proceed=true 时）。", obj(json!({}), &[])),
        f("relic_select", "遗物选择：拿遗物。", obj(json!({ "relic_index": int("遗物索引") }), &["relic_index"])),
        f("relic_skip", "遗物选择：跳过。", obj(json!({}), &[])),
        f("treasure_claim_relic", "宝箱：拿遗物。", obj(json!({ "relic_index": int("遗物索引") }), &["relic_index"])),
        f("menu_select", "菜单/游戏结束：选择选项。", obj(json!({ "option": s("选项名") }), &["option"])),
    ])
}
