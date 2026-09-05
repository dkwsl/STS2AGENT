//! 会话历史存储：每轮对话 + 状态快照 + 决策 + 执行结果存为 JSON，
//! 支持列出历史会话、加载回放、导出（R5）。

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// 一次完整会话。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub created_at: u64,
    pub model: String,
    pub turns: Vec<TurnRecord>,
    pub total_input: u64,
    pub total_output: u64,
    pub total_cost: f64,
    pub finished: bool,
}

/// 单轮记录。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnRecord {
    pub turn: u32,
    pub state_summary: String,
    pub state_json: String,
    /// LLM 完整回复（去掉 ACTION 行后的对话文本）。
    pub agent_text: String,
    /// ACTION 行（如有）。
    pub action: Option<String>,
    /// 执行结果。
    pub result: Option<String>,
    pub success: bool,
    /// 用户输入（如有）。
    pub user_input: Option<String>,
    /// 本轮 token 用量。
    pub input_tokens: u64,
    pub output_tokens: u64,
}

impl Session {
    pub fn new(model: &str) -> Self {
        let id = format!(
            "{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );
        Self {
            id,
            created_at: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            model: model.to_string(),
            turns: Vec::new(),
            total_input: 0,
            total_output: 0,
            total_cost: 0.0,
            finished: false,
        }
    }
}

/// 会话存储管理器。
pub struct SessionStore {
    dir: PathBuf,
}

impl SessionStore {
    /// 从 Config 创建，使用 config.storage.sessions_dir。
    pub fn from_dir(dir: &str) -> Self {
        Self {
            dir: PathBuf::from(dir),
        }
    }

    /// 确保目录存在。
    fn ensure_dir(&self) -> Result<()> {
        std::fs::create_dir_all(&self.dir).context("create sessions dir")?;
        Ok(())
    }

    /// 保存会话（覆盖写）。
    pub fn save(&self, session: &Session) -> Result<()> {
        self.ensure_dir()?;
        let path = self.dir.join(format!("{}.json", session.id));
        let json = serde_json::to_string_pretty(session).context("serialize session")?;
        std::fs::write(&path, json).context("write session file")?;
        Ok(())
    }

    /// 加载会话。
    pub fn load(&self, id: &str) -> Result<Session> {
        let path = self.dir.join(format!("{id}.json"));
        let json = std::fs::read_to_string(&path).context("read session file")?;
        let session: Session = serde_json::from_str(&json).context("parse session")?;
        Ok(session)
    }

    /// 列出所有历史会话（按时间倒序）。
    pub fn list(&self) -> Result<Vec<SessionMeta>> {
        self.ensure_dir()?;
        let mut metas = Vec::new();
        for entry in std::fs::read_dir(&self.dir).context("read sessions dir")? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json") {
                if let Ok(json) = std::fs::read_to_string(&path) {
                    if let Ok(session) = serde_json::from_str::<Session>(&json) {
                        metas.push(SessionMeta {
                            id: session.id.clone(),
                            created_at: session.created_at,
                            model: session.model.clone(),
                            turn_count: session.turns.len() as u32,
                            total_cost: session.total_cost,
                            finished: session.finished,
                        });
                    }
                }
            }
        }
        metas.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        Ok(metas)
    }

    /// 列出会话文件路径（用于 --list 输出）。
    pub fn list_paths(&self) -> Result<Vec<PathBuf>> {
        self.ensure_dir()?;
        let mut paths = Vec::new();
        for entry in std::fs::read_dir(&self.dir).context("read sessions dir")? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "json") {
                paths.push(path);
            }
        }
        paths.sort();
        Ok(paths)
    }
}

/// 会话摘要（列表用）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMeta {
    pub id: String,
    pub created_at: u64,
    pub model: String,
    pub turn_count: u32,
    pub total_cost: f64,
    pub finished: bool,
}

/// 辅助：检查路径存在。
pub fn dir_exists(dir: &str) -> bool {
    Path::new(dir).exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_save_load_roundtrip() {
        let tmp = "/tmp/sts2_test_sessions";
        let _ = std::fs::remove_dir_all(tmp);
        let store = SessionStore::from_dir(tmp);

        let mut session = Session::new("test-model");
        session.turns.push(TurnRecord {
            turn: 1,
            state_summary: "地图".into(),
            state_json: r#"{"state_type":"map"}"#.into(),
            agent_text: "选择打怪".into(),
            action: Some("map_choose_node | node_index=0".into()),
            result: Some("ok".into()),
            success: true,
            user_input: None,
            input_tokens: 100,
            output_tokens: 50,
        });
        session.total_input = 100;
        session.total_output = 50;
        session.total_cost = 0.01;

        store.save(&session).unwrap();
        let loaded = store.load(&session.id).unwrap();
        assert_eq!(loaded.model, "test-model");
        assert_eq!(loaded.turns.len(), 1);
        assert_eq!(loaded.turns[0].state_summary, "地图");
        assert_eq!(
            loaded.turns[0].action.as_deref(),
            Some("map_choose_node | node_index=0")
        );
        assert_eq!(loaded.total_cost, 0.01);

        let list = store.list().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].turn_count, 1);

        let _ = std::fs::remove_dir_all(tmp);
    }
}
