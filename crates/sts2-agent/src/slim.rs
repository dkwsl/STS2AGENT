//! 状态 JSON 瘦身：发送给 LLM 前剥离对决策无用但占 token 的字段。

use serde_json::Value;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_keywords_and_nulls() {
        let json = r#"{"state_type":"monster","player":{"hp":72,"name":null,"hand":[{"id":"STRIKE_R","keywords":["Strike","攻击"]}],"relics":[{"counter":null}]}}"#;
        let slim = slim_state_json(json);
        assert!(!slim.contains("keywords"));
        assert!(!slim.contains("null"));
        assert!(slim.contains("STRIKE_R"));
        let v: serde_json::Value = serde_json::from_str(&slim).unwrap();
        assert_eq!(v["state_type"], "monster");
        assert_eq!(v["player"]["hp"], 72);
    }

    #[test]
    fn invalid_json_passthrough() {
        assert_eq!(slim_state_json("not json"), "not json");
    }
}
