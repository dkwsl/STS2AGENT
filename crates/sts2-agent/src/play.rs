//! 自动对局循环（裸文本模式）：取状态 → LLM 决策 → 解析 → 执行 → 循环。
//! 每轮自动存盘到会话历史（R5）。

use std::io::Write;

use anyhow::Result;
use sts2_core::{Config, GameState, StateType};
use sts2_llm::{BudgetGuard, LlmClient, StreamEvent, Usage};
use sts2_mcp::McpClient;

use crate::decide::build_messages;
use crate::parse::parse_action;
use crate::storage::{Session, SessionStore, TurnRecord};

pub async fn run_play(
    config: &Config,
    use_mock: bool,
    show_thinking: bool,
    max_turns: u32,
    zh: bool,
) -> Result<()> {
    let (command, args) = if use_mock {
        ("./target/debug/sts2-mcp-mock".to_string(), Vec::new())
    } else {
        (config.mcp.command.clone(), config.mcp.args.clone())
    };

    let mut mcp = McpClient::spawn(&command, &args)?;
    mcp.initialize().await?;
    eprintln!(
        "已连接 MCP server ({})。开始自动对局（最多 {max_turns} 轮，Ctrl+C 打断）。\n",
        if use_mock { "mock" } else { "real" }
    );

    let llm = LlmClient::from_config(&config.model);
    let mut budget = BudgetGuard::new(config.budget.token_limit, config.budget.cost_limit_usd);
    let _ = zh;

    // 会话存储
    let store = SessionStore::from_dir(&config.storage.sessions_dir);
    let mut session = Session::new(&config.model.model);

    // 本次对局的知识库主动查询状态
    let mut lookup_context = String::new();
    let mut lookup_rounds: u32 = 0;

    for turn in 1..=max_turns {
        // 1. 取状态
        let state_json = mcp.get_game_state("json").await?;
        let gs: GameState = serde_json::from_str(&state_json).unwrap_or_default();
        let summary = state_summary(&gs);

        println!("═══ 第 {turn} 轮 [{summary}] ═══");

        if gs.state_type == StateType::GameOver {
            println!("游戏结束。");
            break;
        }
        if gs.state_type == StateType::Unknown {
            println!("未知状态，停止。");
            break;
        }

        // 2. 预算检查
        if budget.is_over_budget() {
            println!("预算超限，停止。{}", budget.summary());
            break;
        }

        // 3. LLM 决策（game_knowledge + 本次对局的主动查询记录）
        let mut game_knowledge =
            crate::context::search_game_knowledge(&gs, &config.storage.game_knowledge_dir);
        if !lookup_context.is_empty() {
            game_knowledge.push_str("\n=== 知识库查询记录 ===\n");
            game_knowledge.push_str(&lookup_context);
        }
        let messages = build_messages(
            &state_json,
            &config.model.model,
            &[],
            &summary,
            None,
            true,
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
        let mut full_text = String::new();
        let mut turn_usage = Usage::default();

        while let Some(ev) = rx.recv().await {
            match ev {
                StreamEvent::Delta(text) => {
                    print!("{text}");
                    std::io::stdout().flush().ok();
                    full_text.push_str(&text);
                }
                StreamEvent::Reasoning(text) if show_thinking => {
                    eprint!("{text}");
                    std::io::stderr().flush().ok();
                }
                StreamEvent::Usage(u) => {
                    turn_usage = u.clone();
                    budget.record(&u, config.model.price_in, config.model.price_out);
                }
                StreamEvent::Done => break,
                StreamEvent::Error(e) => {
                    eprintln!("\n[LLM 错误] {e}");
                    break;
                }
                _ => {}
            }
        }
        println!();

        // 4. 解析 ACTION
        let action_line = full_text
            .lines()
            .find(|l| l.trim_start().to_uppercase().starts_with("ACTION:"))
            .unwrap_or("")
            .to_string();

        let action = match parse_action(&full_text) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("[解析失败] {e:#}，跳过本轮。");
                continue;
            }
        };

        // 4.5 知识库查询：拦截，不发给游戏，结果注入下一轮上下文（本地 → Wiki 兜底）
        if action.tool == "lookup" {
            let query = action
                .args
                .get("query")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            if query.is_empty() || lookup_rounds >= 3 {
                eprintln!("[查询] 无效或已达上限（3 次），跳过。");
                continue;
            }
            lookup_rounds += 1;
            println!("[查询知识库] {query}…");
            let (used, result) = {
                let direct = crate::lookup::lookup_query_smart(
                    &query,
                    &gs,
                    &config.storage.game_knowledge_dir,
                );
                if direct.1.is_empty() {
                    // 本地未命中 → 联网查游戏 Wiki 兜底
                    println!("[查询] 🌐 本地未命中，查询游戏 Wiki…");
                    let wiki = crate::lookup::search_wiki_via_mcp(&mut mcp, &query).await;
                    if wiki.is_empty() {
                        (query.clone(), String::new())
                    } else {
                        (format!("{query} (wiki)"), wiki)
                    }
                } else {
                    direct
                }
            };
            if result.is_empty() {
                lookup_context.push_str(&format!("[查询 {query}]: 知识库与 Wiki 均无记录\n"));
                println!("[查询] 未找到 {query}");
            } else {
                if used != query {
                    println!("[查询] 实际命中: {used}");
                }
                lookup_context.push_str(&format!("[查询 {query} → {used}]:\n{result}\n"));
                println!("[查询] 命中 {} 字，继续分析…", result.chars().count());
            }
            continue;
        }

        // 5. 执行
        println!(
            "[执行] {} | {}",
            action.tool,
            serde_json::to_string(&action.args).unwrap_or_default()
        );
        let (success, result_msg) = match mcp.call_tool(&action.tool, action.args.clone()).await {
            Ok(r) => (true, r.chars().take(120).collect::<String>()),
            Err(e) => (false, format!("{e:#}")),
        };
        println!("[结果] {result_msg}");

        // 6. 存盘
        let agent_text = full_text
            .lines()
            .filter(|l| !l.trim_start().to_uppercase().starts_with("ACTION:"))
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string();

        session.turns.push(TurnRecord {
            turn,
            state_summary: summary,
            state_json: state_json.clone(),
            agent_text,
            action: if action_line.is_empty() {
                None
            } else {
                Some(action_line)
            },
            result: Some(result_msg),
            success,
            user_input: None,
            input_tokens: turn_usage.prompt_tokens,
            output_tokens: turn_usage.completion_tokens,
        });
        session.total_input = budget.total_input();
        session.total_output = budget.total_output();
        session.total_cost = budget.total_cost();
        let _ = store.save(&session);

        eprintln!("[用量] {} | 会话已存盘: {}\n", budget.summary(), session.id);
    }

    session.finished = true;
    let _ = store.save(&session);
    mcp.shutdown().await.ok();
    println!("═══ 对局结束 ═══");
    println!("总用量: {}", budget.summary());
    println!(
        "会话 ID: {} （可用 --load {} 回放）",
        session.id, session.id
    );
    Ok(())
}

fn state_summary(gs: &GameState) -> String {
    match gs.state_type {
        StateType::Map => "地图".into(),
        StateType::Monster | StateType::Elite | StateType::Boss => {
            let battle = gs.battle.as_ref();
            let p = gs.player.as_ref();
            match (battle, p) {
                (Some(b), Some(p)) => format!(
                    "战斗 R{} | {}/{} HP, {} 能量 | 敌人: {}",
                    b.round.unwrap_or(0),
                    p.hp,
                    p.max_hp,
                    p.energy.unwrap_or(0),
                    b.enemies.first().map(|e| e.name.as_str()).unwrap_or("?")
                ),
                _ => "战斗".into(),
            }
        }
        StateType::Rewards => "奖励".into(),
        StateType::RestSite => "休息点".into(),
        StateType::Shop | StateType::FakeMerchant => "商店".into(),
        StateType::Event => "事件".into(),
        StateType::Treasure => "宝箱".into(),
        _ => format!("{:?}", gs.state_type),
    }
}
