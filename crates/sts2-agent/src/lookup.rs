//! 知识库主动查询（LLM 的 ACTION: lookup 入口）：
//! 按名称/内部 ID 在 game-knowledge 表格中查找，支持显示名→内部 ID 回退。

use std::path::Path;

use sts2_core::GameState;
use sts2_mcp::McpClient;

/// LLM 主动查询：按名称/内部 ID 在所有知识库表格中查找匹配行。
/// 遍历 8 个表格文件，按第一列匹配（大小写不敏感、忽略分隔符），返回带分类标注的结果。
/// 总输出截断到 1500 字。查不到返回空串。
pub fn lookup_query(query: &str, knowledge_dir: &str) -> String {
    let dir = Path::new(knowledge_dir);
    // 剥离单字母角色后缀（"STRIKE_R" → "STRIKE"）以匹配 PascalCase 表格列
    let norm = normalize_id(&strip_card_suffix(query.trim()));
    if !dir.exists() || norm.is_empty() {
        return String::new();
    }

    let tables = [
        ("cards.md", "卡牌"),
        ("card-behaviors.md", "卡牌行为"),
        ("monsters.md", "敌人"),
        ("monster-behaviors.md", "敌人行为"),
        ("potions.md", "药水"),
        ("potion-behaviors.md", "药水行为"),
        ("events.md", "事件"),
        ("characters.md", "角色"),
    ];

    let mut out = String::new();
    for (file, label) in tables {
        let Ok(content) = std::fs::read_to_string(dir.join(file)) else {
            continue;
        };
        let mut hits = Vec::new();
        for line in content.lines() {
            let t = line.trim();
            if !t.starts_with('|') || t.contains("---") {
                continue;
            }
            let first = first_column(t);
            let nf = normalize_id(first);
            if !nf.is_empty() && (nf == norm || nf.contains(&norm) || norm.contains(&nf)) {
                hits.push(t.to_string());
            }
        }
        if !hits.is_empty() {
            out.push_str(&format!("[{label}]\n{}\n", hits.join("\n")));
        }
    }

    if out.len() > 1500 {
        out = out.chars().take(1500).collect::<String>() + "\n...";
    }
    out
}

/// 一次查询的结果：used 是实际生效的查询词（可能经过显示名→ID 转换）。
pub struct LookupOutcome {
    pub used: String,
    pub result: String,
}

/// 执行一次智能查询（runner / play 共用的拦截入口）。
/// 先按原词查（LLM 应优先用英文内部 ID）；
/// 查不到时，从 GameState 中找显示名（name，可能是中文）对应的内部 ID，转换后重查。
pub fn perform_lookup(query: &str, gs: &GameState, knowledge_dir: &str) -> LookupOutcome {
    let (used, result) = lookup_query_smart(query, gs, knowledge_dir);
    LookupOutcome { used, result }
}

/// 联网兜底：经 MCP 调游戏 Mod 自带的 search_wiki（数据来自游戏本体，覆盖卡牌/遗物，
/// 含升级变体；仅限当前档案已解锁内容）。返回格式化文本，失败/无结果返回空串。
/// 结果截断到 1000 字。
pub async fn search_wiki_via_mcp(mcp: &mut McpClient, query: &str) -> String {
    let args = serde_json::json!({ "query": query, "item_type": "all", "limit": 3 });
    let text = match mcp.call_tool("search_wiki", args).await {
        Ok(t) => t,
        Err(_) => return String::new(), // mock 模式无此工具，静默降级
    };
    let trimmed = text.trim();
    if trimmed.is_empty() || trimmed.starts_with("Error") {
        return String::new();
    }
    if trimmed.chars().count() > 1000 {
        let cut: String = trimmed.chars().take(1000).collect();
        format!("{cut}\n...")
    } else {
        trimmed.to_string()
    }
}

/// 智能查询：先按原词查；查不到时按显示名转内部 ID 重查。
/// 返回 (实际生效的查询词, 结果)。结果为空表示两层都未命中。
pub fn lookup_query_smart(query: &str, gs: &GameState, knowledge_dir: &str) -> (String, String) {
    let direct = lookup_query(query, knowledge_dir);
    if !direct.is_empty() {
        return (query.to_string(), direct);
    }
    if let Some(id) = find_id_by_display_name(gs, query) {
        let r = lookup_query(&id, knowledge_dir);
        if !r.is_empty() {
            return (id, r);
        }
    }
    (query.to_string(), String::new())
}

/// 在 GameState 中按显示名（UI 名称，可能是中文）查找对象的内部英文 ID。
/// 覆盖：手牌卡牌、遗物、药水、敌人。
fn find_id_by_display_name(gs: &GameState, name: &str) -> Option<String> {
    let target = name.trim().to_lowercase();
    if target.is_empty() {
        return None;
    }
    if let Some(p) = &gs.player {
        // 手牌：name → id（剥角色后缀）
        if let Some(hand) = &p.hand {
            for card in hand {
                if card.name.trim().to_lowercase() == target && !card.id.is_empty() {
                    return Some(strip_card_suffix(&card.id));
                }
            }
        }
        // 遗物
        for relic in &p.relics {
            if relic.name.trim().to_lowercase() == target && !relic.id.is_empty() {
                return Some(relic.id.clone());
            }
        }
        // 药水
        for potion in &p.potions {
            if potion.name.trim().to_lowercase() == target && !potion.id.is_empty() {
                return Some(potion.id.clone());
            }
        }
    }
    // 敌人：name → entity_id（去掉 _N 后缀）
    if let Some(b) = &gs.battle {
        for e in &b.enemies {
            if e.name.trim().to_lowercase() == target && !e.entity_id.is_empty() {
                let eid = e
                    .entity_id
                    .trim_end_matches(|c: char| c.is_ascii_digit() || c == '_')
                    .to_string();
                if !eid.is_empty() {
                    return Some(eid);
                }
            }
        }
    }
    None
}

/// 剥离卡牌内部 ID 的单字母角色后缀段：
/// 真实游戏格式 "STRIKE_R"（R=Regent 等角色缩写），知识库表格是 "StrikeRegent"。
/// 按下划线分段后丢弃长度 ≤2 的段，重组为 "STRIKE"，使 normalize 后可前缀命中。
pub(crate) fn strip_card_suffix(id: &str) -> String {
    let parts: Vec<&str> = id.split('_').filter(|s| s.len() > 2).collect();
    if parts.is_empty() {
        id.to_string()
    } else {
        parts.join("_")
    }
}

/// ID 归一化：去掉空格、下划线、连字符，转小写。
/// "jaw_worm" → "jawworm"，用于大小写不敏感匹配。
pub(crate) fn normalize_id(s: &str) -> String {
    s.to_lowercase().replace(['_', '-', ' '], "")
}

/// 提取 Markdown 表格行的第一列内容。
pub(crate) fn first_column(table_line: &str) -> &str {
    table_line
        .trim_start_matches('|')
        .split('|')
        .next()
        .unwrap_or("")
        .trim()
}

/// 在表格文件中按 ID 查找匹配行（context.rs 的自动注入也用）。
/// 表格格式：`| Name | ... |`，第一列是 Name（内部 ID）。
/// 大小写不敏感、忽略下划线/连字符/空格后匹配。
pub(crate) fn lookup_in_table(ids: &[String], file_path: &Path) -> String {
    let content = match std::fs::read_to_string(file_path) {
        Ok(c) => c,
        Err(_) => return String::new(),
    };

    let normalized_ids: Vec<String> = ids.iter().map(|id| normalize_id(id)).collect();
    let mut hits = Vec::new();
    let mut in_table = false;

    for line in content.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with('|') {
            in_table = false;
            continue;
        }
        // 第一行 | --- | --- | 是表头分隔，跳过
        if trimmed.contains("---") {
            in_table = true;
            continue;
        }
        if !in_table {
            // 表头行（| Name | Cost | ...）也跳过
            if trimmed.to_lowercase().contains("name") {
                in_table = true;
                continue;
            }
        }

        let norm_first = normalize_id(first_column(trimmed));
        if normalized_ids
            .iter()
            .any(|id| norm_first == *id || norm_first.contains(id) || id.contains(&norm_first))
        {
            hits.push(trimmed.to_string());
        }
    }

    hits.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn lookup_query_finds_card() {
        let dir = "game-knowledge";
        if !Path::new(dir).exists() {
            return;
        }
        let result = lookup_query("StrikeIronclad", dir);
        assert!(result.contains("[卡牌]"), "should hit cards.md: {result}");
        assert!(result.contains("StrikeIronclad"));
    }

    #[test]
    fn lookup_query_finds_monster_case_insensitive() {
        let dir = "game-knowledge";
        if !Path::new(dir).exists() {
            return;
        }
        let result = lookup_query("jaw worm", dir);
        assert!(
            result.contains("[敌人]"),
            "should hit monsters.md: {result}"
        );
    }

    #[test]
    fn lookup_query_miss_returns_empty() {
        let dir = "game-knowledge";
        if !Path::new(dir).exists() {
            return;
        }
        assert!(lookup_query("NoSuchThing12345", dir).is_empty());
        assert!(lookup_query("", dir).is_empty());
    }

    #[test]
    fn lookup_smart_falls_back_to_display_name() {
        let dir = "game-knowledge";
        if !Path::new(dir).exists() {
            return;
        }
        use sts2_core::{Card, Player, StateType};
        // 中文显示名"打击"，内部 ID StrikeIronclad
        let card = Card {
            id: "StrikeIronclad".into(),
            name: "打击".into(),
            ..Default::default()
        };
        let p = Player {
            hand: Some(vec![card]),
            ..Default::default()
        };
        let gs = GameState {
            state_type: StateType::Monster,
            player: Some(p),
            ..Default::default()
        };
        // 英文 ID 直接命中
        let (used, r) = lookup_query_smart("StrikeIronclad", &gs, dir);
        assert_eq!(used, "StrikeIronclad");
        assert!(!r.is_empty());
        // 中文显示名查不到表格 → 自动转 ID 再查
        let (used, r) = lookup_query_smart("打击", &gs, dir);
        assert_eq!(used, "StrikeIronclad", "should fall back to internal id");
        assert!(r.contains("StrikeIronclad"));
    }

    #[test]
    fn strip_card_suffix_matches_real_id_format() {
        let dir = "game-knowledge";
        if !Path::new(dir).exists() {
            return;
        }
        assert_eq!(strip_card_suffix("STRIKE_R"), "STRIKE");
        let r = lookup_query("STRIKE_R", dir);
        assert!(
            r.contains("[卡牌]"),
            "real-format id should hit cards.md: {r}"
        );
    }

    #[test]
    fn normalize_id_strips_separators() {
        assert_eq!(normalize_id("JAW_WORM_0"), "jawworm0");
        assert_eq!(normalize_id("Jaw Worm"), "jawworm");
        assert_eq!(normalize_id("StrikeIronclad"), "strikeironclad");
    }
}
