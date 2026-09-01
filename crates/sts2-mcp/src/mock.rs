//! Mock 游戏状态机：与真实 STS2MCP 契约同构，提供一段脚本化战斗序列。
//!
//! 流程：Map → choose_node(0) → Combat(Jaw Worm 12HP) → 2× Strike 杀敌 → Rewards → proceed → Map。
//! 不模拟完整游戏机制，仅推进状态并返回合理 JSON，供端到端演示与测试。

use serde_json::{json, Value};

#[derive(Debug, Clone, PartialEq)]
enum Phase {
    Map,
    Combat,
    Rewards,
}

#[derive(Debug, Clone)]
struct MockCard {
    id: &'static str,
    name: &'static str,
    kind: &'static str,
    cost: &'static str,
    target_type: &'static str,
    description: &'static str,
    damage: i32,
    block: i32,
}

const STRIKE: MockCard = MockCard {
    id: "STRIKE_R",
    name: "Strike",
    kind: "Attack",
    cost: "1",
    target_type: "AnyEnemy",
    description: "Deal 6 damage.",
    damage: 6,
    block: 0,
};

const DEFEND: MockCard = MockCard {
    id: "DEFEND_R",
    name: "Defend",
    kind: "Skill",
    cost: "1",
    target_type: "Self",
    description: "Gain 5 Block.",
    damage: 0,
    block: 5,
};

pub struct MockGame {
    phase: Phase,
    round: u32,
    player_hp: i32,
    player_max_hp: i32,
    player_block: i32,
    player_energy: i32,
    player_gold: i32,
    enemy_hp: i32,
    enemy_max_hp: i32,
    enemy_name: &'static str,
    enemy_entity_id: &'static str,
    enemy_damage: i32,
    hand: Vec<MockCard>,
}

impl Default for MockGame {
    fn default() -> Self {
        Self::new()
    }
}

impl MockGame {
    pub fn new() -> Self {
        Self {
            phase: Phase::Map,
            round: 0,
            player_hp: 72,
            player_max_hp: 80,
            player_block: 0,
            player_energy: 3,
            player_gold: 99,
            enemy_hp: 12,
            enemy_max_hp: 12,
            enemy_name: "Jaw Worm",
            enemy_entity_id: "JAW_WORM_0",
            enemy_damage: 5,
            hand: Vec::new(),
        }
    }

    /// 处理一条 JSON-RPC 消息，返回可选响应（None = 通知，无需回复）。
    pub fn handle_message(&mut self, msg: &Value) -> Option<Value> {
        let id = msg.get("id")?;
        let method = msg.get("method")?.as_str()?;
        let result = match method {
            "initialize" => json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "sts2-mock", "version": "0.1.0"}
            }),
            "tools/list" => json!({"tools": tool_list()}),
            "tools/call" => {
                let name = msg["params"]["name"].as_str().unwrap_or("");
                let args = &msg["params"]["arguments"];
                let (text, is_error) = self.call_tool(name, args);
                json!({
                    "content": [{"type": "text", "text": text}],
                    "isError": is_error
                })
            }
            _ => {
                return Some(json!({
                    "jsonrpc": "2.0", "id": id,
                    "error": {"code": -32601, "message": format!("method not found: {method}")}
                }));
            }
        };
        Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
    }

    fn call_tool(&mut self, name: &str, args: &Value) -> (String, bool) {
        match name {
            "get_game_state" => {
                let format = args
                    .get("format")
                    .and_then(|v| v.as_str())
                    .unwrap_or("markdown");
                if format == "json" {
                    (
                        serde_json::to_string_pretty(&self.state_json()).unwrap_or_default(),
                        false,
                    )
                } else {
                    (self.state_markdown(), false)
                }
            }
            "combat_play_card" => self.combat_play_card(args),
            "combat_end_turn" => self.combat_end_turn(),
            "map_choose_node" => self.map_choose_node(args),
            "rewards_claim" => self.rewards_claim(args),
            "proceed_to_map" => self.proceed_to_map(),
            "rewards_pick_card" => (
                r#"{"status":"ok","message":"Card added to deck."}"#.into(),
                false,
            ),
            "rewards_skip_card" => (
                r#"{"status":"ok","message":"Card reward skipped."}"#.into(),
                false,
            ),
            "use_potion" => (
                r#"{"status":"error","error":"No potions available in mock."}"#.into(),
                true,
            ),
            "discard_potion" => (
                r#"{"status":"error","error":"No potions available in mock."}"#.into(),
                true,
            ),
            "menu_select" => (
                r#"{"status":"error","error":"Not in menu state."}"#.into(),
                true,
            ),
            _ => (
                format!(r#"{{"status":"error","error":"tool '{name}' not implemented in mock"}}"#),
                true,
            ),
        }
    }

    fn state_json(&self) -> Value {
        match self.phase {
            Phase::Map => self.map_json(),
            Phase::Combat => self.combat_json(),
            Phase::Rewards => self.rewards_json(),
        }
    }

    fn map_json(&self) -> Value {
        json!({
            "state_type": "map",
            "run": {"act": 1, "floor": 3, "ascension": 0},
            "player": self.player_json(),
            "map": {
                "current_position": {"col": 3, "row": 2, "type": "Monster"},
                "next_options": [
                    {"index": 0, "col": 2, "row": 3, "type": "Monster"},
                    {"index": 1, "col": 3, "row": 3, "type": "RestSite"},
                    {"index": 2, "col": 4, "row": 3, "type": "Shop"}
                ]
            }
        })
    }

    fn combat_json(&self) -> Value {
        let hand: Vec<Value> = self
            .hand
            .iter()
            .enumerate()
            .map(|(i, c)| {
                json!({
                    "index": i,
                    "id": c.id,
                    "name": c.name,
                    "type": c.kind,
                    "cost": c.cost,
                    "star_cost": null,
                    "description": c.description,
                    "target_type": c.target_type,
                    "can_play": self.player_energy >= c.cost.parse::<i32>().unwrap_or(0),
                    "unplayable_reason": null,
                    "is_upgraded": false,
                    "keywords": []
                })
            })
            .collect();

        json!({
            "state_type": "monster",
            "battle": {
                "round": self.round,
                "turn": "player",
                "is_play_phase": true,
                "enemies": [{
                    "entity_id": self.enemy_entity_id,
                    "combat_id": 1,
                    "name": self.enemy_name,
                    "hp": self.enemy_hp,
                    "max_hp": self.enemy_max_hp,
                    "block": 0,
                    "status": [],
                    "intents": [{
                        "type": "Attack",
                        "label": self.enemy_damage.to_string(),
                        "title": "Attack",
                        "description": format!("Deals {} damage.", self.enemy_damage)
                    }]
                }]
            },
            "run": {"act": 1, "floor": 3, "ascension": 0},
            "player": {
                "character": "The Ironclad",
                "hp": self.player_hp,
                "max_hp": self.player_max_hp,
                "block": self.player_block,
                "gold": self.player_gold,
                "energy": self.player_energy,
                "max_energy": 3,
                "hand": hand,
                "draw_pile_count": 10,
                "discard_pile_count": 0,
                "exhaust_pile_count": 0,
                "status": [],
                "relics": [{
                    "id": "BURNING_BLOOD",
                    "name": "Burning Blood",
                    "description": "At the end of combat, heal 6 HP.",
                    "counter": null,
                    "keywords": []
                }],
                "potions": [],
                "max_potion_slots": 3
            }
        })
    }

    fn rewards_json(&self) -> Value {
        json!({
            "state_type": "rewards",
            "run": {"act": 1, "floor": 3, "ascension": 0},
            "player": self.player_json(),
            "rewards": {
                "items": [
                    {"index": 0, "type": "gold", "description": "Obtain 25 gold.", "gold_amount": 25},
                    {"index": 1, "type": "potion", "description": "Obtain a potion.", "potion_id": "FIRE_POTION", "potion_name": "Fire Potion"}
                ],
                "can_proceed": true
            }
        })
    }

    fn player_json(&self) -> Value {
        json!({
            "character": "The Ironclad",
            "hp": self.player_hp,
            "max_hp": self.player_max_hp,
            "block": self.player_block,
            "gold": self.player_gold,
            "relics": [{
                "id": "BURNING_BLOOD",
                "name": "Burning Blood",
                "description": "At the end of combat, heal 6 HP.",
                "counter": null,
                "keywords": []
            }],
            "potions": [],
            "max_potion_slots": 3,
            "status": []
        })
    }

    fn state_markdown(&self) -> String {
        match self.phase {
            Phase::Map => format!(
                "## Map (Act 1, Floor 3)\nPlayer: The Ironclad, {}/{} HP, {} gold\n\nNext options:\n[0] Monster\n[1] RestSite\n[2] Shop",
                self.player_hp, self.player_max_hp, self.player_gold
            ),
            Phase::Combat => format!(
                "## Combat (Round {})\nPlayer: {}/{} HP, {} block, {}/3 energy\nEnemy: {} ({}/{} HP)\nIntent: Attack {}",
                self.round, self.player_hp, self.player_max_hp, self.player_block,
                self.player_energy, self.enemy_name, self.enemy_hp, self.enemy_max_hp, self.enemy_damage
            ),
            Phase::Rewards => format!(
                "## Rewards\nPlayer: {}/{} HP, {} gold\n\n[0] 25 gold\n[1] Fire Potion",
                self.player_hp, self.player_max_hp, self.player_gold
            ),
        }
    }

    // ---- 动作处理 ----

    fn combat_play_card(&mut self, args: &Value) -> (String, bool) {
        if self.phase != Phase::Combat {
            return (
                r#"{"status":"error","error":"Not in combat."}"#.into(),
                true,
            );
        }
        let idx = match args.get("card_index").and_then(|v| v.as_u64()) {
            Some(i) => i as usize,
            None => {
                return (
                    r#"{"status":"error","error":"Missing card_index."}"#.into(),
                    true,
                )
            }
        };
        if idx >= self.hand.len() {
            return (
                r#"{"status":"error","error":"Invalid card_index."}"#.into(),
                true,
            );
        }
        let card = self.hand[idx].clone();
        let cost = card.cost.parse::<i32>().unwrap_or(0);
        if self.player_energy < cost {
            return (
                r#"{"status":"error","error":"Not enough energy."}"#.into(),
                true,
            );
        }
        if card.kind == "Attack" && args.get("target").and_then(|v| v.as_str()).is_none() {
            return (
                r#"{"status":"error","error":"Card requires a target."}"#.into(),
                true,
            );
        }
        // 结算
        self.player_energy -= cost;
        self.enemy_hp = (self.enemy_hp - card.damage).max(0);
        self.player_block += card.block;
        let target = args.get("target").and_then(|v| v.as_str()).unwrap_or("");
        let msg = if card.damage > 0 {
            format!(
                r#"{{"status":"ok","message":"Playing '{}' targeting {}"}}"#,
                card.name, target
            )
        } else {
            format!(
                r#"{{"status":"ok","message":"Playing '{}' (gain {} Block)"}}"#,
                card.name, card.block
            )
        };
        self.hand.remove(idx);
        if self.enemy_hp <= 0 {
            self.phase = Phase::Rewards;
        }
        (msg, false)
    }

    fn combat_end_turn(&mut self) -> (String, bool) {
        if self.phase != Phase::Combat {
            return (
                r#"{"status":"error","error":"Not in combat."}"#.into(),
                true,
            );
        }
        let dmg = (self.enemy_damage - self.player_block).max(0);
        self.player_hp -= dmg;
        self.player_block = 0;
        self.player_energy = 3;
        self.round += 1;
        self.hand = vec![STRIKE.clone(), DEFEND.clone(), STRIKE.clone()];
        (r#"{"status":"ok","message":"Turn ended."}"#.into(), false)
    }

    fn map_choose_node(&mut self, args: &Value) -> (String, bool) {
        if self.phase != Phase::Map {
            return (r#"{"status":"error","error":"Not on map."}"#.into(), true);
        }
        let node = match args.get("node_index").and_then(|v| v.as_u64()) {
            Some(n) => n,
            None => {
                return (
                    r#"{"status":"error","error":"Missing node_index."}"#.into(),
                    true,
                )
            }
        };
        if node != 0 {
            return (
                r#"{"status":"error","error":"Mock only supports node 0 (Monster)."}"#.into(),
                true,
            );
        }
        self.phase = Phase::Combat;
        self.round = 1;
        self.player_energy = 3;
        self.player_block = 0;
        self.enemy_hp = self.enemy_max_hp;
        self.hand = vec![STRIKE.clone(), DEFEND.clone(), STRIKE.clone()];
        (
            r#"{"status":"ok","message":"Entering combat."}"#.into(),
            false,
        )
    }

    fn rewards_claim(&mut self, args: &Value) -> (String, bool) {
        if self.phase != Phase::Rewards {
            return (
                r#"{"status":"error","error":"Not on rewards screen."}"#.into(),
                true,
            );
        }
        let idx = match args.get("reward_index").and_then(|v| v.as_u64()) {
            Some(i) => i,
            None => {
                return (
                    r#"{"status":"error","error":"Missing reward_index."}"#.into(),
                    true,
                )
            }
        };
        match idx {
            0 => {
                self.player_gold += 25;
                (
                    r#"{"status":"ok","message":"Claimed 25 gold."}"#.into(),
                    false,
                )
            }
            1 => (
                r#"{"status":"ok","message":"Claimed Fire Potion."}"#.into(),
                false,
            ),
            _ => (
                r#"{"status":"error","error":"Invalid reward_index."}"#.into(),
                true,
            ),
        }
    }

    fn proceed_to_map(&mut self) -> (String, bool) {
        if self.phase != Phase::Rewards {
            return (
                r#"{"status":"error","error":"Cannot proceed from current screen."}"#.into(),
                true,
            );
        }
        self.phase = Phase::Map;
        self.player_hp = (self.player_hp + 6).min(self.player_max_hp); // Burning Blood heal
        (
            r#"{"status":"ok","message":"Proceeding to map."}"#.into(),
            false,
        )
    }
}

fn tool_list() -> Vec<Value> {
    vec![
        json!({
            "name": "get_game_state",
            "description": "Get the current Slay the Spire 2 game state.",
            "inputSchema": {"type": "object", "properties": {"format": {"type": "string", "default": "markdown"}}}
        }),
        json!({
            "name": "combat_play_card",
            "description": "Play a card from hand.",
            "inputSchema": {"type": "object", "properties": {"card_index": {"type": "integer"}, "target": {"type": "string"}}, "required": ["card_index"]}
        }),
        json!({
            "name": "combat_end_turn",
            "description": "End the current turn.",
            "inputSchema": {"type": "object", "properties": {}}
        }),
        json!({
            "name": "map_choose_node",
            "description": "Choose a map node to travel to.",
            "inputSchema": {"type": "object", "properties": {"node_index": {"type": "integer"}}, "required": ["node_index"]}
        }),
        json!({
            "name": "rewards_claim",
            "description": "Claim a reward.",
            "inputSchema": {"type": "object", "properties": {"reward_index": {"type": "integer"}}, "required": ["reward_index"]}
        }),
        json!({
            "name": "proceed_to_map",
            "description": "Proceed to map.",
            "inputSchema": {"type": "object", "properties": {}}
        }),
        json!({
            "name": "rewards_pick_card",
            "description": "Pick a card reward.",
            "inputSchema": {"type": "object", "properties": {"card_index": {"type": "integer"}}, "required": ["card_index"]}
        }),
        json!({
            "name": "rewards_skip_card",
            "description": "Skip card reward.",
            "inputSchema": {"type": "object", "properties": {}}
        }),
        json!({
            "name": "use_potion",
            "description": "Use a potion.",
            "inputSchema": {"type": "object", "properties": {"slot": {"type": "integer"}, "target": {"type": "string"}}, "required": ["slot"]}
        }),
        json!({
            "name": "discard_potion",
            "description": "Discard a potion.",
            "inputSchema": {"type": "object", "properties": {"slot": {"type": "integer"}}, "required": ["slot"]}
        }),
        json!({
            "name": "menu_select",
            "description": "Select a menu option.",
            "inputSchema": {"type": "object", "properties": {"option": {"type": "string"}, "seed": {"type": "string"}}, "required": ["option"]}
        }),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn full_combat_scenario() {
        let mut g = MockGame::new();
        assert_eq!(g.phase, Phase::Map);

        // choose_node(0) → combat
        let (_, err) = g.call_tool("map_choose_node", &json!({"node_index": 0}));
        assert!(!err);
        assert_eq!(g.phase, Phase::Combat);
        assert_eq!(g.enemy_hp, 12);
        assert_eq!(g.hand.len(), 3);

        // play Strike at index 2 (rightmost) → enemy 6 HP
        let (_, err) = g.combat_play_card(&json!({"card_index": 2, "target": "JAW_WORM_0"}));
        assert!(!err);
        assert_eq!(g.enemy_hp, 6);
        assert_eq!(g.hand.len(), 2);

        // play Strike at index 0 (shifted) → enemy 0 → rewards
        let (_, err) = g.combat_play_card(&json!({"card_index": 0, "target": "JAW_WORM_0"}));
        assert!(!err);
        assert_eq!(g.phase, Phase::Rewards);

        // claim gold
        let (_, err) = g.rewards_claim(&json!({"reward_index": 0}));
        assert!(!err);
        assert_eq!(g.player_gold, 124); // 99 + 25

        // proceed → map
        let (_, err) = g.proceed_to_map();
        assert!(!err);
        assert_eq!(g.phase, Phase::Map);
    }
}
