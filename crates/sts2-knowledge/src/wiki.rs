//! Wiki 搜索抽象：解耦知识检索与具体传输（MCP / 其他后端）。

use sts2_mcp::McpClient;

/// Wiki 搜索器：知识库本地未命中时的联网兜底通道。
/// 失败应返回 Err（调用方静默降级为"无记录"）。
/// 返回 boxed future 以支持 dyn 分发。
pub trait WikiSearcher {
    fn search_wiki(
        &mut self,
        query: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<String>> + Send + '_>>;
}

/// 基于 MCP `search_wiki` 工具的实现（游戏 Mod 自带 wiki，
/// 覆盖卡牌/遗物、含升级变体，仅限当前档案已解锁内容）。
pub struct McpWikiSearcher<'a> {
    pub client: &'a mut McpClient,
}

impl WikiSearcher for McpWikiSearcher<'_> {
    fn search_wiki(
        &mut self,
        query: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<String>> + Send + '_>>
    {
        let args = serde_json::json!({ "query": query, "item_type": "all", "limit": 3 });
        let client = &mut *self.client;
        Box::pin(async move {
            // mock 模式无此工具 → Err → 调用方静默降级
            client.call_tool("search_wiki", args).await
        })
    }
}

/// 基于 HTTP 直连游戏 API 的实现（独立 server 场景，不经 MCP）。
pub struct HttpWikiSearcher {
    pub base_url: String,
    pub client: reqwest::Client,
}

impl WikiSearcher for HttpWikiSearcher {
    fn search_wiki(
        &mut self,
        query: &str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<String>> + Send + '_>>
    {
        let url = format!("{}/wiki", self.base_url);
        let client = self.client.clone();
        let query = query.to_string();
        Box::pin(async move {
            let resp = client
                .get(&url)
                .query(&[
                    ("query", query.as_str()),
                    ("item_type", "all"),
                    ("limit", "3"),
                ])
                .timeout(std::time::Duration::from_secs(10))
                .send()
                .await?;
            Ok(resp.text().await?)
        })
    }
}
