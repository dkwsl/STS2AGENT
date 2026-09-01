//! sts2-mcp-mock: Mock MCP server 二进制入口。
//! 从 stdin 逐行读取 JSON-RPC，经 MockGame 处理，向 stdout 逐行写回响应。
//! 配置示例：command = "sts2-mcp-mock"（需在 PATH 或用全路径）。

#![forbid(unsafe_code)]

use std::io::{self, BufRead, Write};

use sts2_mcp::MockGame;

fn main() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = io::BufWriter::new(stdout.lock());
    let mut game = MockGame::new();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let msg: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        // notifications（无 id）不回复
        if msg.get("id").is_none() {
            continue;
        }
        if let Some(resp) = game.handle_message(&msg) {
            let _ = writeln!(out, "{}", serde_json::to_string(&resp).unwrap_or_default());
            let _ = out.flush();
        }
    }
}
