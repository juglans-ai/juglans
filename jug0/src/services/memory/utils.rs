// src/services/memory/utils.rs
use qdrant_client::qdrant::value::Kind;
use serde_json::Value;
use std::collections::HashMap;

/// 将 JSON Value 转换为 Qdrant 的 Value 类型
pub fn json_to_qdrant_value(v: Value) -> qdrant_client::qdrant::Value {
    match v {
        Value::Null => qdrant_client::qdrant::Value {
            kind: Some(Kind::NullValue(0)),
        },
        Value::Bool(b) => qdrant_client::qdrant::Value {
            kind: Some(Kind::BoolValue(b)),
        },
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                qdrant_client::qdrant::Value {
                    kind: Some(Kind::IntegerValue(i)),
                }
            } else {
                qdrant_client::qdrant::Value {
                    kind: Some(Kind::DoubleValue(n.as_f64().unwrap_or(0.0))),
                }
            }
        }
        Value::String(s) => qdrant_client::qdrant::Value {
            kind: Some(Kind::StringValue(s)),
        },
        Value::Array(arr) => {
            let values: Vec<qdrant_client::qdrant::Value> =
                arr.into_iter().map(json_to_qdrant_value).collect();
            qdrant_client::qdrant::Value {
                kind: Some(Kind::ListValue(qdrant_client::qdrant::ListValue { values })),
            }
        }
        Value::Object(map) => {
            let fields: HashMap<String, qdrant_client::qdrant::Value> = map
                .into_iter()
                .map(|(k, v)| (k, json_to_qdrant_value(v)))
                .collect();
            qdrant_client::qdrant::Value {
                kind: Some(Kind::StructValue(qdrant_client::qdrant::Struct { fields })),
            }
        }
    }
}

/// 将 Qdrant 的 Value 类型转回 JSON Value
pub fn qdrant_value_to_json(v: qdrant_client::qdrant::Value) -> Value {
    match v.kind {
        Some(Kind::NullValue(_)) => Value::Null,
        Some(Kind::BoolValue(b)) => Value::Bool(b),
        Some(Kind::IntegerValue(i)) => Value::Number(i.into()),
        Some(Kind::DoubleValue(d)) => serde_json::Number::from_f64(d)
            .map(Value::Number)
            .unwrap_or(Value::Null),
        Some(Kind::StringValue(s)) => Value::String(s),
        Some(Kind::ListValue(list)) => {
            Value::Array(list.values.into_iter().map(qdrant_value_to_json).collect())
        }
        Some(Kind::StructValue(s)) => {
            let map: serde_json::Map<String, Value> = s
                .fields
                .into_iter()
                .map(|(k, v)| (k, qdrant_value_to_json(v)))
                .collect();
            Value::Object(map)
        }
        None => Value::Null,
    }
}

/// 将 Qdrant Payload 转换为标准的 JSON Map
pub fn qdrant_payload_to_map(
    payload: HashMap<String, qdrant_client::qdrant::Value>,
) -> HashMap<String, Value> {
    payload
        .into_iter()
        .map(|(k, v)| (k, qdrant_value_to_json(v)))
        .collect()
}

/// 清洗 LLM 返回的 JSON 字符串，移除 Markdown 代码块标记
pub fn clean_json_response(input: &str) -> String {
    input
        .replace("```json", "")
        .replace("```", "")
        .trim()
        .to_string()
}
