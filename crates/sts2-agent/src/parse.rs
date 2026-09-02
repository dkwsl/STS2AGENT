//! 解析 LLM 输出的 ACTION 行为 MCP 工具调用。
//!
//! 格式：`ACTION: tool_name | key=value | key=value | ...`
//! 值自动推断类型：纯整数 → i64，否则字符串。
//! 常见别名归一化：end_turn→combat_end_turn 等。

use anyhow::{bail, Result};
use serde_json::{json, Value};

/// 解析后的动作。
#[derive(Debug, Clone)]
pub struct ParsedAction {
    pub tool: String,
    pub args: Value,
}

/// 从 LLM 完整输出中提取并解析 ACTION 行。
pub fn parse_action(text: &str) -> Result<ParsedAction> {
    let action_line = text
        .lines()
        .find(|l| l.trim_start().to_uppercase().starts_with("ACTION:"))
        .ok_or_else(|| anyhow::anyhow!("LLM 输出中未找到 ACTION: 行"))?;

    let rest = action_line
        .trim_start()
        .split_once(':')
        .map(|x| x.1)
        .ok_or_else(|| anyhow::anyhow!("ACTION: 行格式错误"))?
        .trim();

    let parts: Vec<&str> = rest.split('|').map(|s| s.trim()).collect();
    if parts.is_empty() || parts[0].is_empty() {
        bail!("ACTION 行缺少 tool name");
    }

    let tool = normalize_tool(parts[0]);
    let mut args = serde_json::Map::new();
    for part in &parts[1..] {
        if let Some((k, v)) = part.split_once('=') {
            let k = k.trim();
            let v = v.trim();
            if !k.is_empty() {
                args.insert(k.to_string(), parse_value(v));
            }
        }
    }

    Ok(ParsedAction {
        tool,
        args: Value::Object(args),
    })
}

/// 工具名归一化（处理 LLM 可能用的别名）。
fn normalize_tool(name: &str) -> String {
    match name.to_lowercase().as_str() {
        "end_turn" | "endturn" => "combat_end_turn".into(),
        "play_card" | "playcard" => "combat_play_card".into(),
        "choose_map_node" | "map_node" => "map_choose_node".into(),
        "claim_reward" | "claim" => "rewards_claim".into(),
        "proceed" | "proceed_to_map" => "proceed_to_map".into(),
        "skip_card" | "skip" => "rewards_skip_card".into(),
        "pick_card" | "pick" => "rewards_pick_card".into(),
        "select_card" => "deck_select_card".into(),
        "confirm" | "confirm_selection" => "deck_confirm_selection".into(),
        "cancel" | "cancel_selection" => "deck_cancel_selection".into(),
        "rest" | "rest_option" => "rest_choose_option".into(),
        "shop_buy" | "buy" => "shop_purchase".into(),
        other => other.to_string(),
    }
}

/// 值类型推断：纯整数 → i64，否则字符串。
fn parse_value(v: &str) -> Value {
    if let Ok(n) = v.parse::<i64>() {
        json!(n)
    } else {
        json!(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple() {
        let text = "Some reasoning...\nACTION: combat_end_turn\nREASON: ...";
        let a = parse_action(text).unwrap();
        assert_eq!(a.tool, "combat_end_turn");
        assert!(a.args.as_object().unwrap().is_empty());
    }

    #[test]
    fn parse_with_int_param() {
        let text = "ACTION: map_choose_node | node_index=2\nREASON: ...";
        let a = parse_action(text).unwrap();
        assert_eq!(a.tool, "map_choose_node");
        assert_eq!(a.args["node_index"], 2);
    }

    #[test]
    fn parse_with_string_param() {
        let text = "ACTION: combat_play_card | card_index=0 | target=JAW_WORM_0";
        let a = parse_action(text).unwrap();
        assert_eq!(a.tool, "combat_play_card");
        assert_eq!(a.args["card_index"], 0);
        assert_eq!(a.args["target"], "JAW_WORM_0");
    }

    #[test]
    fn normalize_aliases() {
        assert_eq!(normalize_tool("end_turn"), "combat_end_turn");
        assert_eq!(normalize_tool("play_card"), "combat_play_card");
        assert_eq!(normalize_tool("proceed"), "proceed_to_map");
        assert_eq!(normalize_tool("buy"), "shop_purchase");
    }

    #[test]
    fn case_insensitive_prefix() {
        let text = "action: combat_end_turn\nreason: done";
        let a = parse_action(text).unwrap();
        assert_eq!(a.tool, "combat_end_turn");
    }

    #[test]
    fn missing_action_errors() {
        assert!(parse_action("just reasoning, no action").is_err());
    }
}
