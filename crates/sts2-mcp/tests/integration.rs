//! 集成测试：拉起 sts2-mcp-mock 二进制，用 McpClient 走完整流程。
//! 验证：MCP 握手 → 取状态(可被 sts2-core 反序列化) → 选路 → 战斗 → 出牌 → 奖励 → 回地图。

use serde_json::json;
use sts2_core::{GameState, StateType};
use sts2_mcp::McpClient;

fn mock_binary_path() -> String {
    // Cargo 为集成测试设置 CARGO_BIN_EXE_<name>（编译期可见）
    if let Some(p) = option_env!("CARGO_BIN_EXE_sts2-mcp-mock") {
        return p.to_string();
    }
    if let Some(p) = option_env!("CARGO_BIN_EXE_sts2_mcp_mock") {
        return p.to_string();
    }
    // 回退：从 CARGO_MANIFEST_DIR 推导 target/debug 路径
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    std::path::PathBuf::from(&manifest)
        .join("..")
        .join("..")
        .join("target")
        .join("debug")
        .join("sts2-mcp-mock")
        .to_string_lossy()
        .into_owned()
}

#[tokio::test]
async fn mock_full_flow() {
    let path = mock_binary_path();
    let mut client = McpClient::spawn(&path, &[]).expect("spawn mock");

    // 1. initialize
    let info = client.initialize().await.expect("initialize");
    assert_eq!(info["serverInfo"]["name"], "sts2-mock");

    // 2. list_tools
    let tools = client.list_tools().await.expect("list_tools");
    assert!(tools.len() >= 6, "should have at least 6 tools");

    // 3. get_game_state → Map
    let raw = client.get_game_state("json").await.expect("get state");
    let gs: GameState = serde_json::from_str(&raw).expect("parse state");
    assert_eq!(gs.state_type, StateType::Map);

    // 4. map_choose_node(0) → Combat
    let resp = client
        .call_tool("map_choose_node", json!({"node_index": 0}))
        .await
        .expect("choose node");
    assert!(resp.contains("\"ok\""));

    // 5. get_game_state → Monster, enemy 12 HP
    let raw = client.get_game_state("json").await.expect("get state");
    let gs: GameState = serde_json::from_str(&raw).expect("parse state");
    assert_eq!(gs.state_type, StateType::Monster);
    let enemy = &gs.battle.as_ref().unwrap().enemies[0];
    assert_eq!(enemy.entity_id, "JAW_WORM_0");
    assert_eq!(enemy.hp, 12);

    // 6. play_card(index=2, target=JAW_WORM_0) → enemy 6 HP
    let resp = client
        .call_tool(
            "combat_play_card",
            json!({"card_index": 2, "target": "JAW_WORM_0"}),
        )
        .await
        .expect("play card");
    assert!(resp.contains("\"ok\""));
    let raw = client.get_game_state("json").await.expect("get state");
    let gs: GameState = serde_json::from_str(&raw).expect("parse state");
    assert_eq!(gs.battle.as_ref().unwrap().enemies[0].hp, 6);

    // 7. play_card(index=0, target=JAW_WORM_0) → enemy 0 → Rewards
    let resp = client
        .call_tool(
            "combat_play_card",
            json!({"card_index": 0, "target": "JAW_WORM_0"}),
        )
        .await
        .expect("play card");
    assert!(resp.contains("\"ok\""));
    let raw = client.get_game_state("json").await.expect("get state");
    let gs: GameState = serde_json::from_str(&raw).expect("parse state");
    assert_eq!(gs.state_type, StateType::Rewards);

    // 8. claim gold
    let resp = client
        .call_tool("rewards_claim", json!({"reward_index": 0}))
        .await
        .expect("claim");
    assert!(resp.contains("25 gold"));

    // 9. proceed → Map
    let resp = client
        .call_tool("proceed_to_map", json!({}))
        .await
        .expect("proceed");
    assert!(resp.contains("\"ok\""));
    let raw = client.get_game_state("json").await.expect("get state");
    let gs: GameState = serde_json::from_str(&raw).expect("parse state");
    assert_eq!(gs.state_type, StateType::Map);

    client.shutdown().await.ok();
}
