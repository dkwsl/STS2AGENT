//! 只读 MCP 知识 server：把 STS2 知识库暴露为两个工具，供任意 MCP 客户端挂载。
//!
//! 工具：
//! - lookup(query)：按名称/内部 ID 查询知识库（本地表格 + Wiki 兜底）
//! - context_for_state(state_json)：按游戏状态自动注入相关知识行
//!
//! stdio JSON-RPC 2.0，协议子集与 sts2-mcp-mock 相同
//! （initialize / notifications/initialized / tools/list / tools/call）。
//!
//! 用法：sts2-knowledge-serve [--dir <knowledge_dir>] [--wiki-url <game_http>]
//! 默认 knowledge_dir = game-knowledge；wiki 兜底默认关闭（--wiki-url 指向
//! 游戏 HTTP API 时启用，经 sts2mcp 的 search_wiki 端点）。

use std::io::{BufRead, Write};

use serde_json::{json, Value};
use sts2_knowledge::HttpWikiSearcher;

fn tool_definitions() -> Value {
    let f = |name: &str, desc: &str, params: Value| {
        json!({
            "type": "function",
            "function": { "name": name, "description": desc, "parameters": params }
        })
    };
    json!([
        f("lookup", "按名称或内部ID查询杀戮尖塔2知识库（卡牌/敌人/药水/事件/角色，含行为详情）。本地未命中时若配置了 Wiki 会自动兜底。",
          json!({ "type": "object", "properties": { "query": { "type": "string", "description": "名称或内部ID，优先英文内部ID" } }, "required": ["query"] })),
        f("context_for_state", "传入完整游戏状态JSON，返回与当前局面相关的知识库内容（手牌/敌人/药水/事件表格行 + playbook 决策指引 + 未收录手牌标注）。",
          json!({ "type": "object", "properties": { "state_json": { "type": "string", "description": "完整游戏状态JSON" } }, "required": ["state_json"] })),
    ])
}

fn handle_message(msg: &Value, dir: &str, wiki: &mut Option<HttpWikiSearcher>) -> Option<Value> {
    let id = msg.get("id")?;
    let method = msg["method"].as_str().unwrap_or("");
    let result = match method {
        "initialize" => json!({
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": { "name": "sts2-knowledge", "version": "0.1.0" }
        }),
        "tools/list" => json!({ "tools": tool_definitions() }),
        "tools/call" => {
            let name = msg["params"]["name"].as_str().unwrap_or("");
            let args = &msg["params"]["arguments"];
            let rt = tokio::runtime::Runtime::new().ok();
            let text = match rt {
                Some(rt) => rt.block_on(async {
                    match name {
                        "lookup" => {
                            let query = args["query"].as_str().unwrap_or("");
                            let (used, result) =
                                sts2_knowledge::lookup_query_smart(query, &Default::default(), dir);
                            if !result.is_empty() {
                                result
                            } else if let Some(w) = wiki.as_mut() {
                                let r = sts2_knowledge::search_wiki(w, query).await;
                                if r.is_empty() {
                                    format!("未找到 {query}（本地与 Wiki 均无记录）")
                                } else {
                                    format!("[wiki {used}]\n{r}")
                                }
                            } else {
                                format!("未找到 {query}")
                            }
                        }
                        "context_for_state" => {
                            let sj = args["state_json"].as_str().unwrap_or("{}");
                            let gs: sts2_core::GameState =
                                serde_json::from_str(sj).unwrap_or_default();
                            sts2_knowledge::search_game_knowledge(&gs, dir)
                        }
                        _ => format!("unknown tool: {name}"),
                    }
                }),
                None => "internal error: no tokio runtime".into(),
            };
            json!({ "content": [ { "type": "text", "text": text } ] })
        }
        // 通知不回复
        _ => return None,
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn main() -> anyhow::Result<()> {
    let mut dir = "game-knowledge".to_string();
    let mut wiki_url: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--dir" => dir = args.next().unwrap_or(dir),
            "--wiki-url" => wiki_url = args.next(),
            _ => {}
        }
    }

    let mut wiki = wiki_url.map(|base_url| HttpWikiSearcher {
        base_url,
        client: reqwest::Client::new(),
    });

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut line = String::new();
    loop {
        line.clear();
        let n = stdin.lock().read_line(&mut line)?;
        if n == 0 {
            break;
        }
        let Ok(msg) = serde_json::from_str::<Value>(line.trim()) else {
            continue;
        };
        if let Some(resp) = handle_message(&msg, &dir, &mut wiki) {
            writeln!(stdout, "{resp}")?;
            stdout.flush()?;
        }
    }
    Ok(())
}
