//! sts2-tui: 终端界面（见 PLAN.md §5.6）。
//! P0 仅提供可编译入口；P6 实现建议/进度/打断/历史/设置/用量面板。

#![forbid(unsafe_code)]

use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "sts2-tui", version, about = "STS2 决策 Agent 终端界面")]
struct Cli {
    /// 跳过 UI，仅校验配置与依赖是否就绪。
    #[arg(long)]
    check: bool,
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
    } else {
        eprintln!("sts2-tui: UI 尚未实现（P6）；可用 --check 校验配置。");
        Ok(())
    }
}
