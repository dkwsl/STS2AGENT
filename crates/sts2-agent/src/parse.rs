//! 解析 LLM 输出的 ACTION 行为 MCP 工具调用。
//!
//! 格式：`ACTION: tool_name | key=value | key=value | ...`
//! 值自动推断类型：纯整数 → i64，否则字符串。
//! 常见别名归一化：end_turn→combat_end_turn 等。

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

/// 解析后的动作。
#[derive(Debug, Clone)]
pub struct ParsedAction {
    pub tool: String,
    pub args: Value,
}

/// 解析 LLM 的原生工具调用（OpenAI 兼容 tool_calls）。
/// name 走 normalize_tool 归一化，arguments 为 JSON 字符串（空则视为无参）。
pub fn parse_tool_call(name: &str, arguments: &str) -> Result<ParsedAction> {
    let tool = normalize_tool(name);
    let args: serde_json::Map<String, Value> = if arguments.trim().is_empty() {
        Default::default()
    } else {
        serde_json::from_str::<Value>(arguments)
            .with_context(|| format!("invalid tool arguments JSON: {arguments}"))?
            .as_object()
            .cloned()
            .unwrap_or_default()
    };
    let mut args = args;
    normalize_args(&tool, &mut args);
    Ok(ParsedAction {
        tool,
        args: Value::Object(args),
    })
}

/// 从 LLM 完整输出中提取并解析**所有** ACTION 行（支持多步操作）。
pub fn parse_action(text: &str) -> Result<ParsedAction> {
    let action_line = text
        .lines()
        .find(|l| is_action_line(l))
        .ok_or_else(|| anyhow::anyhow!("LLM 输出中未找到 ACTION: 行"))?;
    parse_single_action(action_line)
}

/// 从 LLM 输出中提取 NOTE 行（LLM 自主记录的经验）。
pub fn extract_notes(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| l.trim_start().to_uppercase().starts_with("NOTE:"))
        .map(|l| {
            l.trim_start()
                .split_once(':')
                .map(|x| x.1)
                .unwrap_or("")
                .trim()
                .to_string()
        })
        .filter(|s| !s.is_empty())
        .collect()
}

/// 判断一行是否是 NOTE 行。
pub fn is_note_line(line: &str) -> bool {
    line.trim_start().to_uppercase().starts_with("NOTE:")
}

pub fn is_action_line(line: &str) -> bool {
    let trimmed = line.trim_start();
    if trimmed.to_uppercase().starts_with("ACTION:") {
        return true;
    }
    // 无前缀但匹配 tool_name | key=value 模式
    if let Some((tool, rest)) = trimmed.split_once('|') {
        let tool = tool.trim();
        // 工具名: 只含字母数字下划线
        let valid = tool.chars().all(|c| c.is_alphanumeric() || c == '_');
        let has_param = rest.contains('=');
        return valid && !tool.is_empty() && has_param;
    }
    false
}

/// 提取 ACTION 行中的工具名和参数部分（去掉 "ACTION:" 前缀）。
fn extract_action_text(line: &str) -> &str {
    let trimmed = line.trim_start();
    if trimmed.to_uppercase().starts_with("ACTION:") {
        trimmed.split_once(':').map(|x| x.1).unwrap_or("").trim()
    } else {
        trimmed
    }
}

fn parse_single_action(action_line: &str) -> Result<ParsedAction> {
    let rest = extract_action_text(action_line);

    let parts: Vec<&str> = rest.split('|').map(|s| s.trim()).collect();
    if parts.is_empty() || parts[0].is_empty() {
        bail!("ACTION 行缺少 tool name");
    }

    // LLM 有时输出 "combat_end_turn()" 带括号，需去掉
    let tool_raw = parts[0].trim_end_matches("()").trim();
    let tool = normalize_tool(tool_raw);
    let mut args = serde_json::Map::new();
    for part in &parts[1..] {
        if let Some((k, v)) = part.split_once('=') {
            let k = k.trim().trim_start_matches('_'); // LLM 有时输出 _index 代替 index
            let v = v.trim();
            if !k.is_empty() {
                args.insert(k.to_string(), parse_value(v));
            }
        }
    }
    normalize_args(&tool, &mut args);

    Ok(ParsedAction {
        tool,
        args: Value::Object(args),
    })
}

/// 参数名归一化：HTTP 动作用 `index`，MCP 工具用各自的具体参数名。
fn normalize_args(tool: &str, args: &mut serde_json::Map<String, Value>) {
    if let Some(v) = args.remove("index") {
        let new_key = match tool {
            "rewards_claim" => "reward_index",
            "map_choose_node" => "node_index",
            "event_choose_option" | "rest_choose_option" => "option_index",
            "shop_purchase" => "item_index",
            "relic_select" | "treasure_claim_relic" => "relic_index",
            "bundle_select" => "bundle_index",
            _ => "index",
        };
        args.insert(new_key.to_string(), v);
    }
    // target 必须是字符串类型（MCP server 要求 string，LLM 可能输出整数）
    if let Some(v) = args.get("target") {
        if !v.is_string() {
            let s = match v {
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                _ => v.to_string(),
            };
            args.insert("target".to_string(), Value::String(s));
        }
    }
    // slot 必须是整数
    if let Some(Value::String(s)) = args.get("slot") {
        if let Ok(n) = s.parse::<i64>() {
            args.insert("slot".to_string(), Value::Number(n.into()));
        }
    }
    // option 必须是字符串（menu_select 的 option 参数，LLM 可能输出整数）
    if let Some(v) = args.get("option") {
        if !v.is_string() {
            let s = match v {
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                _ => v.to_string(),
            };
            args.insert("option".to_string(), Value::String(s));
        }
    }
    // tool 参数必须是字符串
    if let Some(v) = args.get("tool") {
        if !v.is_string() {
            let s = match v {
                Value::Number(n) => n.to_string(),
                _ => v.to_string(),
            };
            args.insert("tool".to_string(), Value::String(s));
        }
    }
}

/// 工具名归一化：把 HTTP 动作名/别名统一映射为 MCP 工具名。
fn normalize_tool(name: &str) -> String {
    match name.to_lowercase().as_str() {
        // 战斗
        "play_card" | "playcard" | "combat_play_card" => "combat_play_card".into(),
        "end_turn" | "endturn" | "combat_end_turn" => "combat_end_turn".into(),
        // 地图
        "choose_map_node" | "map_node" | "map_choose_node" => "map_choose_node".into(),
        // 奖励
        "claim_reward" | "claim" | "rewards_claim" => "rewards_claim".into(),
        "proceed" | "proceed_to_map" => "proceed_to_map".into(),
        "skip_card" | "skip_card_reward" | "rewards_skip_card" => "rewards_skip_card".into(),
        "pick_card" | "pick" | "select_card_reward" | "rewards_pick_card" => {
            "rewards_pick_card".into()
        }
        // 事件
        "choose_event_option" | "event_option" | "event_choose_option" => {
            "event_choose_option".into()
        }
        "advance_dialogue" | "event_advance_dialogue" => "event_advance_dialogue".into(),
        // 休息
        "rest" | "rest_option" | "choose_rest_option" | "rest_choose_option" => {
            "rest_choose_option".into()
        }
        // 商店
        "shop_buy" | "buy" | "shop_purchase" => "shop_purchase".into(),
        // 卡牌选择
        "select_card" | "deck_select_card" => "deck_select_card".into(),
        "confirm" | "confirm_selection" | "deck_confirm_selection" => {
            "deck_confirm_selection".into()
        }
        "cancel" | "cancel_selection" | "deck_cancel_selection" => "deck_cancel_selection".into(),
        // 遗物
        "select_relic" | "relic_select" => "relic_select".into(),
        "skip_relic" | "skip_relic_selection" | "relic_skip" => "relic_skip".into(),
        // 宝箱
        "claim_treasure_relic" | "treasure_claim_relic" => "treasure_claim_relic".into(),
        // Bundle
        "select_bundle" | "bundle_select" => "bundle_select".into(),
        "confirm_bundle" | "confirm_bundle_selection" | "bundle_confirm_selection" => {
            "bundle_confirm_selection".into()
        }
        "cancel_bundle" | "cancel_bundle_selection" | "bundle_cancel_selection" => {
            "bundle_cancel_selection".into()
        }
        // 菜单
        "menu_select" => "menu_select".into(),
        // 知识库查询（本地拦截，不发给游戏）
        "query" | "query_knowledge" | "knowledge" | "lookup" => "lookup".into(),
        // 自主模式切换（本地拦截，不发给游戏）
        "auto" | "auto_on" | "autoplay" | "auto_start" => "auto_start".into(),
        "stop_auto" | "auto_off" | "stop_play" | "auto_stop" => "auto_stop".into(),
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
        // HTTP 动作名 → MCP 工具名（本次补全的关键映射）
        assert_eq!(normalize_tool("choose_event_option"), "event_choose_option");
        assert_eq!(normalize_tool("advance_dialogue"), "event_advance_dialogue");
        assert_eq!(normalize_tool("choose_rest_option"), "rest_choose_option");
        assert_eq!(normalize_tool("select_relic"), "relic_select");
        assert_eq!(normalize_tool("skip_relic_selection"), "relic_skip");
        assert_eq!(
            normalize_tool("claim_treasure_relic"),
            "treasure_claim_relic"
        );
        assert_eq!(normalize_tool("select_bundle"), "bundle_select");
        assert_eq!(normalize_tool("select_card_reward"), "rewards_pick_card");
        assert_eq!(normalize_tool("skip_card_reward"), "rewards_skip_card");
    }

    #[test]
    fn param_index_normalization() {
        // HTTP 动作风格: choose_event_option | index=0 → event_choose_option | option_index=0
        let a = parse_action("ACTION: choose_event_option | index=0").unwrap();
        assert_eq!(a.tool, "event_choose_option");
        assert_eq!(a.args["option_index"], 0);
        assert!(a.args.get("index").is_none());

        // map_choose_node | index=2 → node_index=2
        let a = parse_action("ACTION: map_choose_node | index=2").unwrap();
        assert_eq!(a.args["node_index"], 2);

        // rewards_claim | index=0 → reward_index=0
        let a = parse_action("ACTION: rewards_claim | index=0").unwrap();
        assert_eq!(a.args["reward_index"], 0);

        // shop_purchase | index=5 → item_index=5
        let a = parse_action("ACTION: shop_purchase | index=5").unwrap();
        assert_eq!(a.args["item_index"], 5);
    }

    #[test]
    fn menu_select_option_coerced_to_string() {
        // LLM 可能输出整数 option，但 MCP server 要求字符串（pydantic string_type）
        let a = parse_action("ACTION: menu_select | option=0").unwrap();
        assert_eq!(a.tool, "menu_select");
        assert_eq!(a.args["option"], "0");
        assert!(a.args["option"].is_string());

        // 直接传字符串应保持不变
        let a = parse_action("ACTION: menu_select | option=main_menu").unwrap();
        assert_eq!(a.args["option"], "main_menu");

        // crystal_sphere 的 tool 参数同样要求字符串
        let a = parse_action("ACTION: crystal_sphere_set_tool | tool=big").unwrap();
        assert_eq!(a.args["tool"], "big");
    }

    #[test]
    fn case_insensitive_prefix() {
        let text = "action: combat_end_turn\nreason: done";
        let a = parse_action(text).unwrap();
        assert_eq!(a.tool, "combat_end_turn");
    }

    #[test]
    fn extract_notes_basic() {
        let text =
            "分析中...\nNOTE: Jaw Worm 低血量会狂暴\nACTION: combat_end_turn\nNOTE: 这把缺防御牌";
        let notes = extract_notes(text);
        assert_eq!(notes.len(), 2);
        assert!(notes[0].contains("Jaw Worm"));
        assert!(notes[1].contains("缺防御牌"));
    }

    #[test]
    fn extract_notes_empty() {
        let text = "只是分析，没有笔记";
        let notes = extract_notes(text);
        assert!(notes.is_empty());
    }

    #[test]
    fn is_note_line_check() {
        assert!(is_note_line("NOTE: test"));
        assert!(is_note_line("  note: test"));
        assert!(!is_note_line("ACTION: test"));
        assert!(!is_note_line("这只是个注释"));
    }

    #[test]
    fn parse_no_param_action() {
        let a = parse_action("ACTION: proceed_to_map").unwrap();
        assert_eq!(a.tool, "proceed_to_map");
        assert!(a.args.as_object().unwrap().is_empty());

        let a = parse_action("ACTION: combat_end_turn").unwrap();
        assert_eq!(a.tool, "combat_end_turn");
    }

    #[test]
    fn missing_action_errors() {
        assert!(parse_action("just reasoning, no action").is_err());
    }
}
