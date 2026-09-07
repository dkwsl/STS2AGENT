//! game-knowledge 结构化知识库检索 + 状态 JSON 瘦身。
//!
//! 检索：根据当前 GameState 中的 card_id / enemy_id / potion_id / event_id，
//! 去对应的表格文件按行匹配，返回卡牌/敌人/药水/事件的元数据与行为信息。
//! 瘦身：发送状态 JSON 给 LLM 前递归剥离对决策无用但占 token 的字段。

use std::path::Path;

use serde_json::Value;

use sts2_core::{GameState, StateType};

/// 状态 JSON 瘦身：递归删除对决策无用且占 token 的字段。
/// - `keywords`：每个实体的关键词数组，冗长且 LLM 不需要
/// - 值为 `null` 的字段：表示"不适用"，删除不丢语义
///
/// 解析失败时原样返回。
pub fn slim_state_json(state_json: &str) -> String {
    match serde_json::from_str::<Value>(state_json) {
        Ok(mut v) => {
            strip_keys(&mut v);
            serde_json::to_string(&v).unwrap_or_else(|_| state_json.to_string())
        }
        Err(_) => state_json.to_string(),
    }
}

fn strip_keys(v: &mut Value) {
    match v {
        Value::Object(map) => {
            map.remove("keywords");
            map.retain(|_, val| !val.is_null());
            for (_, child) in map.iter_mut() {
                strip_keys(child);
            }
        }
        Value::Array(arr) => {
            for child in arr.iter_mut() {
                strip_keys(child);
            }
        }
        _ => {}
    }
}

// ===== game-knowledge 结构化索引检索 =====
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
    fn slim_state_strips_keywords_and_nulls() {
        let json = r#"{"state_type":"monster","player":{"hp":72,"name":null,"hand":[{"id":"STRIKE_R","keywords":["Strike","攻击"]}],"relics":[{"counter":null}]}}"#;
        let slim = slim_state_json(json);
        assert!(!slim.contains("keywords"));
        assert!(!slim.contains("null"));
        assert!(slim.contains("STRIKE_R"));
        // 往返仍可解析
        let v: serde_json::Value = serde_json::from_str(&slim).unwrap();
        assert_eq!(v["state_type"], "monster");
        assert_eq!(v["player"]["hp"], 72);
    }

    #[test]
    fn slim_state_invalid_json_passthrough() {
        assert_eq!(slim_state_json("not json"), "not json");
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
