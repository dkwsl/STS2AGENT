//! MCP 客户端：以 stdio 拉起 MCP server（真实 Python 或 Rust Mock），
//! 通过 JSON-RPC 2.0 调用工具。实现 MCP 协议子集：
//! initialize / notifications/initialized / tools/list / tools/call。

use std::process::Stdio;

use anyhow::{Context, Result};
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

pub struct McpClient {
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    child: Child,
}

impl McpClient {
    /// 拉起 MCP server 子进程并连接其 stdio。
    pub fn spawn(command: &str, args: &[String]) -> Result<Self> {
        let mut child = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .context(format!("failed to spawn MCP server: {command}"))?;

        let stdin = BufWriter::new(child.stdin.take().context("no stdin")?);
        let stdout = BufReader::new(child.stdout.take().context("no stdout")?);

        Ok(Self {
            stdin,
            stdout,
            next_id: 0,
            child,
        })
    }

    fn next_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    async fn send(&mut self, msg: &Value) -> Result<()> {
        let line = serde_json::to_string(msg)? + "\n";
        self.stdin.write_all(line.as_bytes()).await?;
        self.stdin.flush().await?;
        Ok(())
    }

    async fn send_notification(&mut self, method: &str) -> Result<()> {
        self.send(&json!({"jsonrpc": "2.0", "method": method}))
            .await
    }

    async fn request(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id();
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))
            .await?;
        self.read_response(id).await
    }

    /// 逐行读取，跳过通知（无 id），返回匹配 id 的响应。
    async fn read_response(&mut self, expected_id: u64) -> Result<Value> {
        let mut line = String::new();
        loop {
            line.clear();
            let n = self.stdout.read_line(&mut line).await?;
            if n == 0 {
                anyhow::bail!("MCP server closed stdout (waiting for id={expected_id})");
            }
            let msg: Value = serde_json::from_str(line.trim())
                .with_context(|| format!("invalid JSON-RPC line: {line}"))?;
            if msg.get("id").is_none() {
                continue;
            }
            let id = msg["id"].as_u64().context("invalid id")?;
            if id != expected_id {
                continue;
            }
            if let Some(err) = msg.get("error") {
                anyhow::bail!("MCP error: {err}");
            }
            return Ok(msg["result"].clone());
        }
    }

    /// MCP 握手。
    pub async fn initialize(&mut self) -> Result<Value> {
        let params = json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "sts2-agent", "version": "0.1.0"}
        });
        let result = self.request("initialize", params).await?;
        self.send_notification("notifications/initialized").await?;
        Ok(result)
    }

    /// 列出 server 暴露的工具。
    pub async fn list_tools(&mut self) -> Result<Vec<Value>> {
        let result = self.request("tools/list", json!({})).await?;
        Ok(result["tools"].as_array().cloned().unwrap_or_default())
    }

    /// 调用工具，返回 content[0].text 文本；若 isError=true 则报错。
    pub async fn call_tool(&mut self, name: &str, arguments: Value) -> Result<String> {
        let params = json!({"name": name, "arguments": arguments});
        let result = self.request("tools/call", params).await?;
        if result
            .get("isError")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            let text = result["content"][0]["text"].as_str().unwrap_or("unknown");
            anyhow::bail!("tool '{name}' error: {text}");
        }
        let text = result["content"][0]["text"]
            .as_str()
            .context("unexpected tool call result format")?;
        Ok(text.to_string())
    }

    /// 便捷：取游戏状态（format="json" 返回 JSON 字符串）。
    pub async fn get_game_state(&mut self, format: &str) -> Result<String> {
        self.call_tool("get_game_state", json!({"format": format}))
            .await
    }

    /// 终止子进程。
    pub async fn shutdown(&mut self) -> Result<()> {
        let _ = self.child.kill().await;
        Ok(())
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}
