//! 知识库检索：从 data/knowledge/raw/*.md 按关键词检索相关段落。
//!
//! 流程：读取所有 .md → 按段落分块 → 关键词匹配（含同义词扩展）→ 按命中数排序 → 返回 top 5。
//! 段落太长（>500字）自动按句号切分。总输出截断到 1500 字。

use std::collections::HashMap;
use std::path::Path;

use sts2_core::{GameState, StateType};

/// 从 GameState 提取搜索关键词。
pub fn extract_keywords(gs: &GameState) -> Vec<String> {
    let mut keywords = Vec::new();

    // 角色名："The Ironclad" → "ironclad"
    if let Some(p) = &gs.player {
        let ch = p.character.to_lowercase();
        let ch = ch.strip_prefix("the ").unwrap_or(&ch);
        keywords.push(ch.to_string());
    }

    // 敌人名 + entity_id
    if let Some(b) = &gs.battle {
        for e in &b.enemies {
            let name = e.name.to_lowercase();
            keywords.push(name.clone());
            // entity_id: "JAW_WORM_0" → "jaw_worm"
            let eid = e.entity_id.to_lowercase();
            let eid = eid.trim_end_matches(|c: char| c.is_ascii_digit() || c == '_');
            if !eid.is_empty() {
                keywords.push(eid.to_string());
            }
        }
    }

    // 手牌名 + id
    if let Some(p) = &gs.player {
        if let Some(hand) = &p.hand {
            for card in hand {
                keywords.push(card.name.to_lowercase());
                // id: "STRIKE_R" → "strike"
                let id = card.id.to_lowercase();
                let id = id.split('_').next().unwrap_or(&id);
                if id.len() > 2 {
                    keywords.push(id.to_string());
                }
            }
        }
    }

    // 遗物名
    if let Some(p) = &gs.player {
        for relic in &p.relics {
            keywords.push(relic.name.to_lowercase());
            let id = relic.id.to_lowercase();
            let id = id.split('_').next().unwrap_or(&id);
            if id.len() > 2 {
                keywords.push(id.to_string());
            }
        }
    }

    // 药水名
    if let Some(p) = &gs.player {
        for potion in &p.potions {
            keywords.push(potion.name.to_lowercase());
        }
    }

    // state_type
    match gs.state_type {
        StateType::Monster | StateType::Elite | StateType::Boss => {
            keywords.push("combat".to_string());
        }
        StateType::Map => {
            keywords.push("map".to_string());
        }
        StateType::Event => {
            keywords.push("event".to_string());
        }
        StateType::Rewards | StateType::CardReward => {
            keywords.push("rewards".to_string());
        }
        StateType::Shop | StateType::FakeMerchant => {
            keywords.push("shop".to_string());
        }
        _ => {}
    }

    // 去重
    keywords.sort();
    keywords.dedup();
    keywords
}

/// 中英文同义词映射（小写英文 → 中文变体列表）。
fn synonym_map() -> HashMap<&'static str, Vec<&'static str>> {
    let mut m = HashMap::new();
    // 角色
    m.insert("ironclad", vec!["铁甲战士", "铁甲"]);
    m.insert("silent", vec!["静默猎手", "猎手", "静默"]);
    m.insert("defect", vec!["故障机器人", "缺陷体"]);
    m.insert("regent", vec!["储君", "摄政王"]);
    m.insert("necrobinder", vec!["亡灵契师", "亡灵缚师", "亡灵"]);
    // 游戏术语
    m.insert("attack", vec!["攻击"]);
    m.insert("skill", vec!["技能"]);
    m.insert("power", vec!["能力"]);
    m.insert("block", vec!["格挡"]);
    m.insert("energy", vec!["能量"]);
    m.insert("strength", vec!["力量"]);
    m.insert("vulnerable", vec!["易伤"]);
    m.insert("weak", vec!["虚弱"]);
    m.insert("relic", vec!["遗物"]);
    m.insert("potion", vec!["药水"]);
    m.insert("combat", vec!["战斗"]);
    m.insert("boss", vec!["首领"]);
    m.insert("elite", vec!["精英"]);
    m.insert("exhaust", vec!["消耗"]);
    m.insert("draw", vec!["抽牌", "抽卡"]);
    m.insert("deck", vec!["牌组", "卡组"]);
    m.insert("hand", vec!["手牌"]);
    m
}

/// 扩展关键词：加入中文同义词。
pub fn expand_keywords(keywords: &[String]) -> Vec<String> {
    let syn = synonym_map();
    let mut result: Vec<String> = Vec::new();

    for kw in keywords {
        let lower = kw.to_lowercase();
        result.push(lower.clone());
        if let Some(synonyms) = syn.get(lower.as_str()) {
            for s in synonyms {
                result.push((*s).to_string());
            }
        }
        // 中文 → 英文：直接查反向
        for (eng, cns) in &syn {
            if cns.iter().any(|cn| kw.contains(cn)) {
                result.push((*eng).to_string());
            }
        }
    }

    // 去重
    result.sort();
    result.dedup();
    result
}

/// 搜索知识库，返回匹配段落拼接的文本。
pub fn search_knowledge(keywords: &[String], knowledge_dir: &str) -> String {
    let dir = Path::new(knowledge_dir);
    if !dir.exists() {
        return String::new();
    }

    let expanded = expand_keywords(keywords);
    if expanded.is_empty() {
        return String::new();
    }

    let mut chunks: Vec<(String, usize)> = Vec::new(); // (text, match_count)

    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return String::new(),
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let paragraphs = split_paragraphs(&content);

        for para in paragraphs {
            let lower_para = para.to_lowercase();
            let mut count = 0;
            for kw in &expanded {
                let lower_kw = kw.to_lowercase();
                if lower_para.contains(&lower_kw) {
                    count += 1;
                }
            }
            if count > 0 {
                chunks.push((para, count));
            }
        }
    }

    if chunks.is_empty() {
        return String::new();
    }

    // 按命中数排序，取 top 5
    chunks.sort_by(|a, b| b.1.cmp(&a.1));

    let mut result = String::new();
    let mut total_len = 0;
    let max_len = 1500;

    for (text, _) in chunks.iter().take(5) {
        let len = text.len();
        if total_len + len > max_len {
            let remaining = max_len.saturating_sub(total_len);
            if remaining > 50 {
                // 按字符边界截断，不能用字节索引
                let truncated: String = text.chars().take(remaining).collect();
                result.push_str(&truncated);
                result.push_str("...\n");
            }
            break;
        }
        result.push_str(text);
        result.push_str("\n---\n");
        total_len += len + 5;
    }

    result
}

/// 将文件内容按段落分块。
/// 跳过元数据头（来源/标题/分类/爬取日期）。
/// 段落 > 500 字时按句号切分为 ≤300 字的块。
fn split_paragraphs(content: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut in_metadata = true;

    for line in content.lines() {
        let trimmed = line.trim();

        // 跳过元数据头
        if in_metadata {
            if trimmed.starts_with("来源:")
                || trimmed.starts_with("标题:")
                || trimmed.starts_with("分类:")
                || trimmed.starts_with("爬取日期:")
                || trimmed.starts_with("更新日期:")
            {
                continue;
            }
            if trimmed.is_empty() {
                in_metadata = false;
                continue;
            }
            continue;
        }

        if trimmed.is_empty() {
            if !current.is_empty() {
                result.push(current.trim().to_string());
                current.clear();
            }
        } else {
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(trimmed);
        }
    }
    if !current.is_empty() {
        result.push(current.trim().to_string());
    }

    // 太长的段落按句号切分
    let mut final_result = Vec::new();
    for para in result {
        if para.len() > 500 {
            let sentences: Vec<&str> = para.split('。').collect();
            let mut chunk = String::new();
            for s in &sentences {
                if s.is_empty() {
                    continue;
                }
                if chunk.len() + s.len() > 300 && !chunk.is_empty() {
                    final_result.push(chunk.trim().to_string() + "。");
                    chunk.clear();
                }
                chunk.push_str(s);
                chunk.push('。');
            }
            if !chunk.is_empty() {
                final_result.push(chunk.trim().to_string());
            }
        } else {
            final_result.push(para);
        }
    }

    final_result
}

// ===== game-knowledge 结构化索引检索 =====
//
// game-knowledge/ 是从游戏反编译数据生成的结构化表格，按内部 ID 索引：
//   cards.md / card-behaviors.md   — 卡牌元数据 + 行为
//   monsters.md / monster-behaviors.md — 敌人 HP + 意图行为
//   potions.md / potion-behaviors.md — 药水元数据 + 行为
//   events.md / characters.md      — 事件 / 角色开局信息
//   playbook.md / agent-reference.md — 决策流程指引
//
// 与 search_knowledge（段落分块+关键词模糊匹配）不同，这里按行精确匹配 ID。

/// 从 GameState 的 card_id / enemy_id / potion_id 查 game-knowledge 表格，
/// 返回匹配行 + playbook 相关段落。总输出截断到 2000 字。
pub fn search_game_knowledge(gs: &GameState, knowledge_dir: &str) -> String {
    let dir = Path::new(knowledge_dir);
    if !dir.exists() {
        return String::new();
    }

    let mut result = String::new();
    let max_len = 2000;

    // 1. 手牌 + 牌组的 card_id → cards.md + card-behaviors.md
    let card_ids = collect_card_ids(gs);
    if !card_ids.is_empty() {
        let table_rows = lookup_in_table(&card_ids, &dir.join("cards.md"));
        let behavior_rows = lookup_in_table(&card_ids, &dir.join("card-behaviors.md"));
        append_section(
            &mut result,
            "卡牌索引",
            &table_rows,
            &mut max_len_counter(&max_len),
        );
        append_section(
            &mut result,
            "卡牌行为",
            &behavior_rows,
            &mut max_len_counter(&max_len),
        );
    }

    // 2. enemy_id → monsters.md + monster-behaviors.md
    let enemy_ids = collect_enemy_ids(gs);
    if !enemy_ids.is_empty() {
        let table_rows = lookup_in_table(&enemy_ids, &dir.join("monsters.md"));
        let behavior_rows = lookup_in_table(&enemy_ids, &dir.join("monster-behaviors.md"));
        append_section(
            &mut result,
            "敌人索引",
            &table_rows,
            &mut max_len_counter(&max_len),
        );
        append_section(
            &mut result,
            "敌人行为",
            &behavior_rows,
            &mut max_len_counter(&max_len),
        );
    }

    // 3. potion_id → potions.md + potion-behaviors.md
    let potion_ids = collect_potion_ids(gs);
    if !potion_ids.is_empty() {
        let table_rows = lookup_in_table(&potion_ids, &dir.join("potions.md"));
        let behavior_rows = lookup_in_table(&potion_ids, &dir.join("potion-behaviors.md"));
        append_section(
            &mut result,
            "药水索引",
            &table_rows,
            &mut max_len_counter(&max_len),
        );
        append_section(
            &mut result,
            "药水行为",
            &behavior_rows,
            &mut max_len_counter(&max_len),
        );
    }

    // 4. event_id → events.md
    if let Some(event_id) = extract_event_id(gs) {
        let ids = vec![event_id];
        let table_rows = lookup_in_table(&ids, &dir.join("events.md"));
        append_section(
            &mut result,
            "事件索引",
            &table_rows,
            &mut max_len_counter(&max_len),
        );
    }

    // 5. playbook 的战斗/地图/事件相关段落（按 state_type）
    let playbook_snippet = lookup_playbook(gs, &dir.join("playbook.md"));
    if !playbook_snippet.is_empty() {
        append_section(
            &mut result,
            "决策指引",
            &playbook_snippet,
            &mut max_len_counter(&max_len),
        );
    }

    result
}

/// 可变计数器封装，用于 append_section 截断控制。
struct MaxLenCounter {
    remaining: usize,
}

impl MaxLenCounter {
    fn new(max: usize) -> Self {
        Self { remaining: max }
    }
}

fn max_len_counter(max: &usize) -> MaxLenCounter {
    MaxLenCounter::new(*max)
}

/// 收集 GameState 中所有 card_id（手牌 + 抽牌堆 + 弃牌堆）。
fn collect_card_ids(gs: &GameState) -> Vec<String> {
    let mut ids = Vec::new();
    if let Some(p) = &gs.player {
        if let Some(hand) = &p.hand {
            for card in hand {
                if !card.id.is_empty() {
                    ids.push(card.id.clone());
                }
                if !card.name.is_empty() {
                    ids.push(card.name.clone());
                }
            }
        }
        if let Some(draw) = &p.draw_pile {
            for card in draw {
                if !card.name.is_empty() {
                    ids.push(card.name.clone());
                }
            }
        }
        if let Some(discard) = &p.discard_pile {
            for card in discard {
                if !card.name.is_empty() {
                    ids.push(card.name.clone());
                }
            }
        }
    }
    dedup_ids(ids)
}

/// 收集 GameState 中所有 enemy_id（entity_id 去掉 _0 后缀）。
fn collect_enemy_ids(gs: &GameState) -> Vec<String> {
    let mut ids = Vec::new();
    if let Some(b) = &gs.battle {
        for e in &b.enemies {
            if !e.entity_id.is_empty() {
                // "JAW_WORM_0" → "JawWorm"
                let eid = e
                    .entity_id
                    .trim_end_matches(|c: char| c.is_ascii_digit() || c == '_');
                if !eid.is_empty() {
                    ids.push(normalize_id(eid));
                }
            }
            if !e.name.is_empty() {
                ids.push(normalize_id(&e.name));
            }
        }
    }
    dedup_ids(ids)
}

/// 收集 GameState 中所有 potion_id。
fn collect_potion_ids(gs: &GameState) -> Vec<String> {
    let mut ids = Vec::new();
    if let Some(p) = &gs.player {
        for potion in &p.potions {
            if !potion.id.is_empty() {
                ids.push(potion.id.clone());
            }
            if !potion.name.is_empty() {
                ids.push(normalize_id(&potion.name));
            }
        }
    }
    dedup_ids(ids)
}

/// 从 GameState 提取 event_id（仅 Event 状态）。
fn extract_event_id(gs: &GameState) -> Option<String> {
    if let Some(ev) = &gs.event {
        if let Some(id) = &ev.event_id {
            if !id.is_empty() {
                return Some(id.clone());
            }
        }
    }
    None
}

/// ID 归一化：去掉空格、下划线、连字符，PascalCase。
/// "jaw_worm" → "jawworm"，用于大小写不敏感匹配。
fn normalize_id(s: &str) -> String {
    s.to_lowercase().replace(['_', '-', ' '], "")
}

/// 去重。
fn dedup_ids(mut ids: Vec<String>) -> Vec<String> {
    ids.sort();
    ids.dedup();
    ids
}

/// 在表格文件中按 ID 查找匹配行。
/// 表格格式：`| Name | ... |`，第一列是 Name（内部 ID）。
/// 大小写不敏感、忽略下划线/连字符/空格后匹配。
fn lookup_in_table(ids: &[String], file_path: &Path) -> String {
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
        if !in_table && !trimmed.starts_with("| ---") {
            // 表头行（| Name | Cost | ...）也跳过
            if trimmed.to_lowercase().contains("name") {
                in_table = true;
                continue;
            }
        }

        // 提取第一列内容
        let first_col = trimmed
            .trim_start_matches('|')
            .split('|')
            .next()
            .unwrap_or("")
            .trim();
        let norm_first = normalize_id(first_col);

        if normalized_ids
            .iter()
            .any(|id| norm_first == *id || norm_first.contains(id) || id.contains(&norm_first))
        {
            hits.push(trimmed.to_string());
        }
    }

    hits.join("\n")
}

/// 从 playbook.md 提取与当前 state_type 相关的段落。
fn lookup_playbook(gs: &GameState, file_path: &Path) -> String {
    let content = match std::fs::read_to_string(file_path) {
        Ok(c) => c,
        Err(_) => return String::new(),
    };

    // 按状态选择相关段落
    let keywords: Vec<&str> = match gs.state_type {
        StateType::Monster | StateType::Elite | StateType::Boss => vec!["Combat", "Monster"],
        StateType::Map => vec!["Core", "Route"],
        StateType::Event => vec!["Event"],
        StateType::Rewards | StateType::CardReward => vec!["Reward"],
        StateType::Shop | StateType::FakeMerchant => vec!["Shop"],
        StateType::RestSite => vec!["Rest"],
        _ => vec![],
    };

    if keywords.is_empty() {
        return String::new();
    }

    // 按段落（空行分隔）提取含关键词的段落
    let mut result = Vec::new();
    let mut current = String::new();
    for line in content.lines() {
        if line.trim().is_empty() {
            if !current.is_empty() {
                let lower = current.to_lowercase();
                if keywords.iter().any(|kw| lower.contains(&kw.to_lowercase())) {
                    result.push(current.clone());
                }
                current.clear();
            }
        } else {
            if !current.is_empty() {
                current.push('\n');
            }
            current.push_str(line);
        }
    }
    if !current.is_empty() {
        let lower = current.to_lowercase();
        if keywords.iter().any(|kw| lower.contains(&kw.to_lowercase())) {
            result.push(current);
        }
    }

    result.join("\n\n")
}

/// 把一个 section 追加到 result，带截断控制。
fn append_section(result: &mut String, title: &str, content: &str, counter: &mut MaxLenCounter) {
    if content.is_empty() || counter.remaining == 0 {
        return;
    }
    let header = format!("[{title}]\n");
    let total = header.len() + content.len() + 2;
    if total >= counter.remaining {
        let remaining = counter.remaining.saturating_sub(header.len() + 2);
        if remaining > 20 {
            let truncated: String = content.chars().take(remaining).collect();
            result.push_str(&header);
            result.push_str(&truncated);
            result.push_str("...\n\n");
            counter.remaining = 0;
        }
        return;
    }
    result.push_str(&header);
    result.push_str(content);
    result.push_str("\n\n");
    counter.remaining = counter.remaining.saturating_sub(total);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_ironclad() {
        let kw = vec!["ironclad".to_string()];
        let expanded = expand_keywords(&kw);
        assert!(expanded.contains(&"ironclad".to_string()));
        assert!(expanded.contains(&"铁甲战士".to_string()));
        assert!(expanded.contains(&"铁甲".to_string()));
    }

    #[test]
    fn expand_chinese_to_english() {
        let kw = vec!["铁甲战士".to_string()];
        let expanded = expand_keywords(&kw);
        assert!(expanded.contains(&"铁甲战士".to_string()));
        assert!(expanded.contains(&"ironclad".to_string()));
    }

    #[test]
    fn split_skips_metadata() {
        let content = "来源: https://example.com\n标题: 测试\n分类: test\n\n# 标题\n段落1\n\n段落2";
        let paras = split_paragraphs(content);
        assert!(paras
            .iter()
            .all(|p| !p.contains("来源:") && !p.contains("标题:")));
        assert!(paras.iter().any(|p| p.contains("段落1")));
        assert!(paras.iter().any(|p| p.contains("段落2")));
    }

    #[test]
    fn split_long_paragraph() {
        let long = "这是一段很长的文字。".repeat(100);
        let paras = split_paragraphs(&format!("\n\n{}", long));
        assert!(paras.len() > 1);
        for p in &paras {
            assert!(p.len() <= 350); // 允许一些余量
        }
    }

    #[test]
    fn extract_ironclad_keywords() {
        use sts2_core::{Battle, Enemy};
        let e = Enemy {
            entity_id: "JAW_WORM_0".into(),
            name: "Jaw Worm".into(),
            ..Default::default()
        };
        let b = Battle {
            enemies: vec![e],
            ..Default::default()
        };
        let p = sts2_core::Player {
            character: "The Ironclad".into(),
            ..Default::default()
        };
        let gs = GameState {
            state_type: StateType::Monster,
            battle: Some(b),
            player: Some(p),
            ..Default::default()
        };

        let kw = extract_keywords(&gs);
        assert!(kw.contains(&"ironclad".to_string()));
        assert!(kw.contains(&"jaw worm".to_string()));
        assert!(kw.contains(&"jaw_worm".to_string()));
        assert!(kw.contains(&"combat".to_string()));
    }

    #[test]
    fn search_finds_ironclad() {
        let dir = "data/knowledge/raw";
        if !Path::new(dir).exists() {
            return; // CI 环境可能没有知识库
        }
        let result = search_knowledge(&["ironclad".to_string()], dir);
        assert!(!result.is_empty());
        assert!(
            result.to_lowercase().contains("铁甲") || result.to_lowercase().contains("ironclad")
        );
    }

    #[test]
    fn game_knowledge_lookup_card() {
        let dir = "game-knowledge";
        if !Path::new(dir).exists() {
            return;
        }
        use sts2_core::{Card, Player};
        let card = Card {
            id: "StrikeIronclad".into(),
            name: "Strike".into(),
            ..Default::default()
        };
        let p = Player {
            character: "Ironclad".into(),
            hand: Some(vec![card]),
            ..Default::default()
        };
        let gs = GameState {
            state_type: StateType::Monster,
            player: Some(p),
            ..Default::default()
        };
        let result = search_game_knowledge(&gs, dir);
        assert!(!result.is_empty(), "should find StrikeIronclad in cards.md");
    }

    #[test]
    fn game_knowledge_lookup_enemy() {
        let dir = "game-knowledge";
        if !Path::new(dir).exists() {
            return;
        }
        use sts2_core::{Battle, Enemy};
        let e = Enemy {
            entity_id: "JAW_WORM_0".into(),
            name: "Jaw Worm".into(),
            ..Default::default()
        };
        let b = Battle {
            enemies: vec![e],
            ..Default::default()
        };
        let gs = GameState {
            state_type: StateType::Monster,
            battle: Some(b),
            ..Default::default()
        };
        let result = search_game_knowledge(&gs, dir);
        assert!(!result.is_empty(), "should find JawWorm in monsters.md");
    }

    #[test]
    fn normalize_id_strips_separators() {
        assert_eq!(normalize_id("JAW_WORM_0"), "jawworm0");
        assert_eq!(normalize_id("Jaw Worm"), "jawworm");
        assert_eq!(normalize_id("StrikeIronclad"), "strikeironclad");
    }
}
