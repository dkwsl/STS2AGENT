//! serde 往返单测：用 STS2MCP `raw-full.md` 真实样本验证领域模型。

use serde_json::{json, Value};
use sts2_core::{Action, StateType, Turn};

const COMBAT_JSON: &str = r#"{
  "state_type": "monster",
  "battle": {
    "round": 1,
    "turn": "player",
    "is_play_phase": true,
    "enemies": [
      {
        "entity_id": "JAW_WORM_0",
        "combat_id": 42,
        "name": "Jaw Worm",
        "hp": 44, "max_hp": 44, "block": 0,
        "status": [],
        "intents": [
          {"type":"Attack","label":"11","title":"Attack","description":"Deals 11 damage."}
        ]
      }
    ]
  },
  "run": {"act":1,"floor":3,"ascension":0},
  "player": {
    "character":"The Ironclad","hp":72,"max_hp":80,"block":0,"gold":99,
    "energy":3,"max_energy":3,
    "hand":[{"index":0,"id":"STRIKE_R","name":"Strike","type":"Attack","cost":"1","star_cost":null,"description":"Deal 6 damage.","target_type":"AnyEnemy","can_play":true,"unplayable_reason":null,"is_upgraded":false,"keywords":[]}],
    "draw_pile_count":15,"discard_pile_count":3,"exhaust_pile_count":1,
    "status":[],
    "relics":[{"id":"BURNING_BLOOD","name":"Burning Blood","description":"At the end of combat, heal 6 HP.","counter":null,"keywords":[]}],
    "potions":[{"id":"SWIFT_POTION","name":"Swift Potion","description":"Draw 3 cards.","slot":0,"can_use_in_combat":true,"target_type":"None","keywords":[]}],
    "max_potion_slots":3,
    "future_player_field":"x"
  },
  "top_future_field": 123
}"#;

const MENU_JSON: &str = r#"{
  "state_type": "menu",
  "message": "No run in progress. Player is in the main menu.",
  "menu_screen": "main",
  "options": ["continue","singleplayer","multiplayer","compendium","timeline","settings","quit"]
}"#;

#[test]
fn parse_combat_state() {
    let gs: sts2_core::GameState = serde_json::from_str(COMBAT_JSON).expect("combat parse");

    assert_eq!(gs.state_type, StateType::Monster);
    assert_eq!(gs.run.as_ref().unwrap().act, 1);
    assert_eq!(gs.run.as_ref().unwrap().floor, 3);

    let battle = gs.battle.as_ref().expect("battle");
    assert_eq!(battle.round, Some(1));
    assert_eq!(battle.turn, Some(Turn::Player));
    assert_eq!(battle.is_play_phase, Some(true));
    let enemy = &battle.enemies[0];
    assert_eq!(enemy.entity_id, "JAW_WORM_0");
    assert_eq!(enemy.combat_id, Some(42));
    assert_eq!(enemy.hp, 44);
    let intent = &enemy.intents[0];
    assert_eq!(intent.kind, "Attack");
    assert_eq!(intent.label.as_deref(), Some("11"));

    let p = gs.player.as_ref().expect("player");
    assert_eq!(p.character, "The Ironclad");
    assert_eq!(p.hp, 72);
    assert_eq!(p.energy, Some(3));
    assert_eq!(p.max_potion_slots, 3);
    let card = &p.hand.as_ref().unwrap()[0];
    assert_eq!(card.index, Some(0));
    assert_eq!(card.id, "STRIKE_R");
    assert_eq!(card.kind, "Attack");
    assert_eq!(card.cost, "1");
    assert_eq!(card.target_type.as_deref(), Some("AnyEnemy"));
    let relic = &p.relics[0];
    assert_eq!(relic.id, "BURNING_BLOOD");
    assert_eq!(relic.counter, None);
    let potion = &p.potions[0];
    assert_eq!(potion.slot, 0);
    assert_eq!(potion.target_type.as_deref(), Some("None"));

    // 未识别字段被 extra 兜底保留。
    assert_eq!(
        p.extra.get("future_player_field").and_then(Value::as_str),
        Some("x")
    );
    assert_eq!(
        gs.extra.get("top_future_field").and_then(Value::as_i64),
        Some(123)
    );
}

#[test]
fn roundtrip_combat_state() {
    let gs: sts2_core::GameState = serde_json::from_str(COMBAT_JSON).expect("parse");
    let s = serde_json::to_string(&gs).expect("serialize");
    let gs2: sts2_core::GameState = serde_json::from_str(&s).expect("reparse");
    assert_eq!(gs2.state_type, StateType::Monster);
    let e = gs2.battle.unwrap().enemies;
    assert_eq!(e[0].entity_id, "JAW_WORM_0");
    assert_eq!(gs2.player.unwrap().energy, Some(3));
    // extra 往返仍保留
    let reparsed: Value = serde_json::from_str(&s).unwrap();
    assert_eq!(reparsed["top_future_field"].as_i64(), Some(123));
}

#[test]
fn parse_menu_state() {
    let gs: sts2_core::GameState = serde_json::from_str(MENU_JSON).expect("menu parse");
    assert_eq!(gs.state_type, StateType::Menu);
    assert_eq!(gs.menu_screen.as_deref(), Some("main"));
    let opts = gs.options.expect("options");
    assert_eq!(opts.len(), 7);
    assert_eq!(opts[1], json!("singleplayer"));
    assert!(gs.run.is_none());
    assert!(gs.player.is_none());
}

#[test]
fn unknown_state_type_falls_back() {
    let j = r#"{"state_type":"totally_new_screen","foo":1}"#;
    let gs: sts2_core::GameState = serde_json::from_str(j).expect("parse");
    assert_eq!(gs.state_type, StateType::Unknown);
    assert_eq!(gs.extra.get("foo").and_then(Value::as_i64), Some(1));
}

#[test]
fn state_type_serde_variants() {
    assert_eq!(
        serde_json::from_str::<StateType>(r#""monster""#).unwrap(),
        StateType::Monster
    );
    assert_eq!(
        serde_json::from_str::<StateType>(r#""boss""#).unwrap(),
        StateType::Boss
    );
    assert_eq!(
        serde_json::from_str::<StateType>(r#""weird""#).unwrap(),
        StateType::Unknown
    );
}

#[test]
fn action_serializes_to_request_body() {
    let a = Action::PlayCard {
        card_index: 0,
        target: Some("JAW_WORM_0".into()),
    };
    let v: Value = serde_json::to_value(&a).unwrap();
    assert_eq!(v["action"], "play_card");
    assert_eq!(v["card_index"], 0);
    assert_eq!(v["target"], "JAW_WORM_0");

    let a2 = Action::UsePotion {
        slot: 0,
        target: None,
    };
    let v2: Value = serde_json::to_value(&a2).unwrap();
    assert_eq!(v2["action"], "use_potion");
    assert_eq!(v2["slot"], 0);
    // 无 target 时不输出该字段。
    assert!(v2.get("target").is_none());

    let v3: Value = serde_json::to_value(&Action::EndTurn).unwrap();
    assert_eq!(v3, json!({"action":"end_turn"}));
}

#[test]
fn action_roundtrip() {
    let cases = vec![
        Action::PlayCard {
            card_index: 2,
            target: Some("KIN_PRIEST_0".into()),
        },
        Action::EndTurn,
        Action::UsePotion {
            slot: 1,
            target: None,
        },
        Action::MenuSelect {
            option: "singleplayer".into(),
            seed: None,
        },
        Action::CrystalSphereClickCell { x: 4, y: 7 },
    ];
    for a in cases {
        let s = serde_json::to_string(&a).unwrap();
        let back: Action = serde_json::from_str(&s).unwrap();
        assert_eq!(a, back, "roundtrip for {s}");
    }
}
