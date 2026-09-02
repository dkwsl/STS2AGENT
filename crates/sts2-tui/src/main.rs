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
    #[arg(long, default_value = "20")]
    max_turns: u32,
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
    } else if cli.tui {
        let cfg = sts2_agent::load_config()?;
        if cfg.model.api_key.is_empty() {
            eprintln!("未配置 API key。");
            std::process::exit(1);
        }
        tokio::runtime::Runtime::new()?.block_on(async {
            runner::run(
                &cfg,
                cli.mock,
                cli.thinking,
                cli.zh,
                cli.play,
                cli.max_turns,
            )
            .await
        })
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
        eprintln!("sts2-tui: 可用命令：--check | --decide [--mock] [--zh] | --play [--mock] [--zh] | --tui [--mock] [--zh] [--max-turns N]");
        eprintln!("  --tui 进入交互式对话界面");
        Ok(())
    }
}
