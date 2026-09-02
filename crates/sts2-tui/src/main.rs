//! sts2-tui: 终端界面（见 PLAN.md §5.6）。
//! P4 阶段提供 `--decide --mock` 可运行入口；P6 实现完整 ratatui 界面。

#![forbid(unsafe_code)]

use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "sts2-tui", version, about = "STS2 决策 Agent 终端界面")]
struct Cli {
    /// 跳过 UI，仅校验配置与依赖是否就绪。
    #[arg(long)]
    check: bool,
    /// 执行一次 LLM 决策（取状态 → 调 LLM → 打印建议）。
    #[arg(long)]
    decide: bool,
    /// --decide 时使用 Mock MCP server（而非 config 里的真实 server）。
    #[arg(long)]
    mock: bool,
    /// 显示 LLM 思考过程（reasoning）。
    #[arg(long)]
    thinking: bool,
    /// 自动对局：循环取状态→决策→执行→取状态，直到结束或打断。
    #[arg(long)]
    play: bool,
    /// --play 时的最大轮数（默认 20）。
    #[arg(long, default_value = "20")]
    max_turns: u32,
    /// LLM 用中文回答。
    #[arg(long)]
    zh: bool,
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
    } else if cli.decide {
        let cfg = sts2_agent::load_config()?;
        if cfg.model.api_key.is_empty() {
            eprintln!(
                "未配置 API key。请在 config/.env 设置 STS2_OPENAI_API_KEY 或在 config.toml 填写。"
            );
            std::process::exit(1);
        }
        tokio::runtime::Runtime::new()?.block_on(async {
            sts2_agent::decide::run_decide(&cfg, cli.mock, cli.thinking, cli.zh).await
        })
    } else if cli.play {
        let cfg = sts2_agent::load_config()?;
        if cfg.model.api_key.is_empty() {
            eprintln!(
                "未配置 API key。请在 config/.env 设置 STS2_OPENAI_API_KEY 或在 config.toml 填写。"
            );
            std::process::exit(1);
        }
        tokio::runtime::Runtime::new()?.block_on(async {
            sts2_agent::play::run_play(&cfg, cli.mock, cli.thinking, cli.max_turns, cli.zh).await
        })
    } else {
        eprintln!("sts2-tui: 可用命令：--check | --decide [--mock] [--thinking] [--zh] | --play [--mock] [--thinking] [--zh] [--max-turns N]");
        Ok(())
    }
}
