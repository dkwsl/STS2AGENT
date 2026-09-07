//! 知识库自动注入：决策前按当前 GameState 的 ID 查表，
//! 生成注入 LLM prompt 的"游戏数据参考"上下文。

use std::path::Path;

use sts2_core::{GameState, StateType};

use super::lookup::{lookup_in_table, normalize_id, strip_card_suffix};

/// 从 GameState 的 card_id / enemy_id / potion_id 查 game-knowledge 表格，
/// 返回匹配行 + playbook 相关段落。总输出截断到 2000 字。
pub fn search_game_knowledge(gs: &GameState, knowledge_dir: &str) -> String {
    let dir = Path::new(knowledge_dir);
    if !dir.exists() {
        return String::new();
    }

    let mut budget = Budget::new(2000);

    // 1. 手牌 + 牌组的 card_id → cards.md + card-behaviors.md
    let card_ids = collect_card_ids(gs);
    if !card_ids.is_empty() {
        budget.append(
            "卡牌索引",
            &lookup_in_table(&card_ids, &dir.join("cards.md")),
        );
        budget.append(
            "卡牌行为",
            &lookup_in_table(&card_ids, &dir.join("card-behaviors.md")),
        );
    }

    // 2. 检测知识库未命中的手牌 → 显式列出，强制 LLM 查询或承认不确定（不占预算）
    let unknown_cards = find_unknown_hand_cards(gs, dir);
    if !unknown_cards.is_empty() {
        budget.prepend_raw(&unknown_cards);
    }

    // 3. enemy_id → monsters.md + monster-behaviors.md
    let enemy_ids = collect_enemy_ids(gs);
    if !enemy_ids.is_empty() {
        budget.append(
            "敌人索引",
            &lookup_in_table(&enemy_ids, &dir.join("monsters.md")),
        );
        budget.append(
            "敌人行为",
            &lookup_in_table(&enemy_ids, &dir.join("monster-behaviors.md")),
        );
    }

    // 4. potion_id → potions.md + potion-behaviors.md
    let potion_ids = collect_potion_ids(gs);
    if !potion_ids.is_empty() {
        budget.append(
            "药水索引",
            &lookup_in_table(&potion_ids, &dir.join("potions.md")),
        );
        budget.append(
            "药水行为",
            &lookup_in_table(&potion_ids, &dir.join("potion-behaviors.md")),
        );
    }

    // 5. event_id → events.md
    if let Some(event_id) = extract_event_id(gs) {
        let ids = vec![event_id];
        budget.append("事件索引", &lookup_in_table(&ids, &dir.join("events.md")));
    }

    // 6. playbook 的战斗/地图/事件相关段落（按 state_type）
    let playbook_snippet = lookup_playbook(gs, &dir.join("playbook.md"));
    if !playbook_snippet.is_empty() {
        budget.append("决策指引", &playbook_snippet);
    }

    budget.finish()
}

/// 检测手牌中知识库未命中的卡牌，生成强指令提示（可为空）。
pub fn find_unknown_hand_cards(gs: &GameState, dir: &Path) -> String {
    let cards_path = dir.join("cards.md");
    if !cards_path.exists() {
        return String::new();
    }
    let mut unknown: Vec<String> = Vec::new();
    if let Some(p) = &gs.player {
        if let Some(hand) = &p.hand {
            for card in hand {
                let q = strip_card_suffix(&card.id);
                if !q.is_empty()
                    && lookup_in_table(std::slice::from_ref(&q), &cards_path).is_empty()
                    && lookup_in_table(std::slice::from_ref(&card.name), &cards_path).is_empty()
                {
                    unknown.push(format!("{} [id={}]", card.name, card.id));
                }
            }
        }
    }
    if unknown.is_empty() {
        String::new()
    } else {
        format!(
            "\n[!] 以下手牌在本地知识库表格中未收录: {}\n注意：若下方「知识库查询记录」中已有这些牌的信息，以查询记录为准，不要再查询或声称查不到。对仍无信息的牌，明说\"不确定\"，禁止凭猜测出牌或评价。\n",
            unknown.join("、")
        )
    }
}

/// 输出预算：控制注入上下文总长度。
struct Budget {
    remaining: usize,
    out: String,
}

impl Budget {
    fn new(max: usize) -> Self {
        Self {
            remaining: max,
            out: String::new(),
        }
    }

    /// 追加一段带标题的内容，超预算时截断。
    fn append(&mut self, title: &str, content: &str) {
        if content.is_empty() || self.remaining == 0 {
            return;
        }
        let header = format!("[{title}]\n");
        let total = header.len() + content.len() + 2;
        if total >= self.remaining {
            let remaining = self.remaining.saturating_sub(header.len() + 2);
            if remaining > 20 {
                let truncated: String = content.chars().take(remaining).collect();
                self.out.push_str(&header);
                self.out.push_str(&truncated);
                self.out.push_str("...\n\n");
                self.remaining = 0;
            }
            return;
        }
        self.out.push_str(&header);
        self.out.push_str(content);
        self.out.push_str("\n\n");
        self.remaining = self.remaining.saturating_sub(total);
    }

    /// 追加原始文本到最前面（高优先级提示，不占预算、不截断）。
    fn prepend_raw(&mut self, text: &str) {
        self.out.insert_str(0, text);
    }

    fn finish(self) -> String {
        self.out
    }
}

/// 收集 GameState 中所有 card_id（手牌 + 抽牌堆 + 弃牌堆）。
fn collect_card_ids(gs: &GameState) -> Vec<String> {
    let mut ids = Vec::new();
    if let Some(p) = &gs.player {
        if let Some(hand) = &p.hand {
            for card in hand {
                if !card.id.is_empty() {
                    ids.push(strip_card_suffix(&card.id));
                }
                if !card.name.is_empty() {
                    ids.push(card.name.clone());
                }
            }
        }
        for cards in p.draw_pile.iter().chain(p.discard_pile.iter()) {
            for card in cards {
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
    gs.event
        .as_ref()
        .and_then(|ev| ev.event_id.as_ref())
        .filter(|id| !id.is_empty())
        .cloned()
}

/// 去重。
fn dedup_ids(mut ids: Vec<String>) -> Vec<String> {
    ids.sort();
    ids.dedup();
    ids
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn search_flags_unknown_hand_cards() {
        let dir = "game-knowledge";
        if !Path::new(dir).exists() {
            return;
        }
        use sts2_core::{Card, Player};
        let known = Card {
            id: "BASH".into(),
            name: "Bash".into(),
            ..Default::default()
        };
        let unknown = Card {
            id: "WEIRD_NEW_CARD_X".into(),
            name: "怪牌".into(),
            ..Default::default()
        };
        let p = Player {
            hand: Some(vec![known, unknown]),
            ..Default::default()
        };
        let gs = GameState {
            state_type: StateType::Monster,
            player: Some(p),
            ..Default::default()
        };
        let result = search_game_knowledge(&gs, dir);
        assert!(result.contains("知识库未收录"), "should flag unknown card");
        assert!(result.contains("怪牌"));
        assert!(!result.contains("Bash [id=BASH]"), "known card not flagged");
    }

    #[test]
    fn search_finds_hand_card() {
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
    fn search_finds_enemy() {
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
}
