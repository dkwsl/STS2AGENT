//! sts2-tui: 终端界面——自然语言对话模式。
//!
//! 可用命令：
//! --check              校验配置
//! --decide [--mock]     单次 LLM 决策（裸文本）
//! --play [--mock]       自动对局（裸文本）
//! --tui [--mock]        交互式 ratatui 对话界面

#![forbid(unsafe_code)]

mod app;
mod runner;
mod ui;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "sts2-tui", version, about = "STS2 决策 Agent 终端界面")]
struct Cli {
    #[arg(long)]
    check: bool,
    #[arg(long)]
    decide: bool,
    #[arg(long)]
    play: bool,
    #[arg(long)]
    tui: bool,
    #[arg(long)]
    mock: bool,
    #[arg(long)]
    thinking: bool,
    #[arg(long)]
    zh: bool,
    #[arg(long, default_value_t = 0)]
    /// 最大执行轮数；0 = 不限（默认）。预算（token/成本）仍会兜底中断。
    max_turns: u32,
    /// 列出历史会话。
    #[arg(long)]
    list: bool,
    /// 加载历史会话回放。
    #[arg(long)]
    load: Option<String>,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    if cli.check {
        let cfg = sts2_agent::load_config();
        match cfg {
            Ok(c) => {
                println!(
                    "config ok: model={}, mcp.command={}",
                    c.model.model, c.mcp.command
                );
                Ok(())
            }
            Err(e) => {
                eprintln!("config error: {e:#}");
                std::process::exit(1);
            }
        }
    } else if cli.list {
        let cfg = sts2_agent::load_config()?;
        let store = sts2_agent::storage::SessionStore::from_dir(&cfg.storage.sessions_dir);
        match store.list() {
            Ok(sessions) => {
                if sessions.is_empty() {
                    println!("暂无历史会话。");
                } else {
                    println!(
                        "{:<16} {:<20} {:<6} {:<10} {:<4}",
                        "ID", "模型", "轮数", "成本", "结束"
                    );
                    for m in sessions {
                        println!(
                            "{:<16} {:<20} {:<6} ${:<9.4} {}",
                            m.id,
                            m.model,
                            m.turn_count,
                            m.total_cost,
                            if m.finished { "是" } else { "否" }
                        );
                    }
                }
                Ok(())
            }
            Err(e) => {
                eprintln!("列出会话失败: {e:#}");
                std::process::exit(1);
            }
        }
    } else if cli.tui {
        let cfg = sts2_agent::load_config()?;
        if cfg.model.api_key.is_empty() {
            eprintln!("未配置 API key。");
            std::process::exit(1);
        }
        // --tui --load <id>：恢复历史会话上下文继续对话（R5）
        let resume = cli.load.clone();
        tokio::runtime::Runtime::new()?.block_on(async {
            runner::run(
                &cfg,
                cli.mock,
                cli.thinking,
                cli.zh,
                cli.play,
                cli.max_turns,
                resume,
            )
            .await
        })
    } else if let Some(id) = &cli.load {
        let cfg = sts2_agent::load_config()?;
        let store = sts2_agent::storage::SessionStore::from_dir(&cfg.storage.sessions_dir);
        match store.load(id) {
            Ok(session) => {
                println!("=== 会话 {} ===", session.id);
                println!(
                    "模型: {} | 轮数: {} | 成本: ${:.4}\n",
                    session.model,
                    session.turns.len(),
                    session.total_cost
                );
                for t in &session.turns {
                    println!("--- 第 {} 轮 [{}] ---", t.turn, t.state_summary);
                    if !t.agent_text.is_empty() {
                        println!("{}", t.agent_text);
                    }
                    if let Some(a) = &t.action {
                        println!("ACTION: {a}");
                    }
                    if let Some(r) = &t.result {
                        println!("结果: {r}");
                    }
                    println!();
                }
                println!(
                    "总用量: input={}, output={}, cost=${:.4}",
                    session.total_input, session.total_output, session.total_cost
                );
                Ok(())
            }
            Err(e) => {
                eprintln!("加载会话失败: {e:#}");
                std::process::exit(1);
            }
        }
    } else if cli.decide {
        let cfg = sts2_agent::load_config()?;
        if cfg.model.api_key.is_empty() {
            eprintln!("未配置 API key。");
            std::process::exit(1);
        }
        tokio::runtime::Runtime::new()?.block_on(async {
            sts2_agent::decide::run_decide(&cfg, cli.mock, cli.thinking, cli.zh).await
        })
    } else if cli.play {
        let cfg = sts2_agent::load_config()?;
        if cfg.model.api_key.is_empty() {
            eprintln!("未配置 API key。");
            std::process::exit(1);
        }
        tokio::runtime::Runtime::new()?.block_on(async {
            sts2_agent::play::run_play(&cfg, cli.mock, cli.thinking, cli.max_turns, cli.zh).await
        })
    } else {
        eprintln!("sts2-tui: 可用命令：--check | --list | --load <id> | --tui [--mock] [--zh] [--max-turns N] | --decide [--mock] [--zh] | --play [--mock] [--zh]");
        eprintln!("  --tui 进入交互式对话界面");
        Ok(())
    }
}
