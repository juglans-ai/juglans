// src/handlers/chat/helpers.rs
use crate::services::mcp::McpTool;
use serde_json::{json, Value};

pub fn merge_tools(client_tools: &Option<Vec<Value>>, mcp_tools: &[McpTool]) -> Option<Vec<Value>> {
    let mut all_tools = Vec::new();
    if let Some(ct) = client_tools {
        all_tools.extend(ct.clone());
    }
    for tool in mcp_tools {
        let openai_tool = json!({
            "type": "function",
            "function": {
                "name": tool.name,
                "description": tool.description,
                "parameters": tool.input_schema
            }
        });
        all_tools.push(openai_tool);
    }
    if all_tools.is_empty() {
        None
    } else {
        Some(all_tools)
    }
}

pub fn try_repair_json(input: &str) -> String {
    use lazy_static::lazy_static;
    use regex::Regex;
    lazy_static! {
        static ref RE: Regex = Regex::new(r#":\s*([a-zA-Z_][a-zA-Z0-9_]*)"#).unwrap();
        // 修复 identifier 内部多余引号: CRYPTO:"BTC".OKX → CRYPTO:BTC.OKX
        static ref RE_IDENT: Regex = Regex::new(
            r#"([A-Z_]+):"([A-Za-z0-9_]+)"\.([A-Za-z0-9_]+)"#
        ).unwrap();
    }

    // Step 1: 原有修复 unquoted values
    let step1 = RE
        .replace_all(input, |c: &regex::Captures| match &c[1] {
            "true" | "false" | "null" => c[0].to_string(),
            v => c[0].replace(v, &format!("\"{}\"", v)),
        })
        .to_string();

    // Step 2: 修复 identifier 内部引号 (CRYPTO:"BTC".OKX → CRYPTO:BTC.OKX)
    // 必须在 RE 之后执行，因为 RE 会将 CRYPTO:BTC 中的 :BTC 误加引号变成 CRYPTO:"BTC"
    RE_IDENT.replace_all(&step1, "$1:$2.$3").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repair_malformed_identifier_single() {
        let input = r#"{"instruments":["CRYPTO:"BTC".OKX@USDT_SPOT"]}"#;
        let result = try_repair_json(input);
        assert_eq!(result, r#"{"instruments":["CRYPTO:BTC.OKX@USDT_SPOT"]}"#);
    }

    #[test]
    fn test_repair_malformed_identifier_multiple() {
        let input =
            r#"{"instruments":["CRYPTO:"BTC".OKX@USDT_SPOT","CRYPTO:"ETH".OKX@USDT_SPOT"]}"#;
        let result = try_repair_json(input);
        assert_eq!(
            result,
            r#"{"instruments":["CRYPTO:BTC.OKX@USDT_SPOT","CRYPTO:ETH.OKX@USDT_SPOT"]}"#
        );
    }

    #[test]
    fn test_repair_preserves_correct_json() {
        let input = r#"{"instruments":["CRYPTO:BTC.OKX@USDT_SPOT"]}"#;
        let result = try_repair_json(input);
        assert_eq!(result, input);
    }

    #[test]
    fn test_repair_result_is_valid_json() {
        let input =
            r#"{"instruments":["CRYPTO:"BTC".OKX@USDT_SPOT","CRYPTO:"ETH".OKX@USDT_SPOT"]}"#;
        let result = try_repair_json(input);
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(&result);
        assert!(
            parsed.is_ok(),
            "Repaired JSON should be parseable, got: {}",
            result
        );
    }

    #[test]
    fn test_repair_unquoted_value_still_works() {
        let input = r#"{"key": someValue}"#;
        let result = try_repair_json(input);
        assert_eq!(result, r#"{"key": "someValue"}"#);
    }

    #[test]
    fn test_repair_preserves_bool_null() {
        let input = r#"{"a": true, "b": false, "c": null}"#;
        let result = try_repair_json(input);
        assert_eq!(result, input);
    }
}
