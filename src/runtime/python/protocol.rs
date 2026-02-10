// src/runtime/python/protocol.rs
//
// JSON-RPC protocol types for Python worker communication

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

/// Request types that can be sent to the Python worker
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum PythonRequest {
    /// Call a function/method on a module or reference
    Call {
        id: String,
        target: String,
        method: String,
        args: Vec<Value>,
        kwargs: HashMap<String, Value>,
    },
    /// Get an attribute from a reference
    Getattr {
        id: String,
        target: String,
        attr: String,
    },
    /// Delete references (garbage collection)
    Del {
        id: String,
        refs: Vec<String>,
    },
    /// Health check
    Ping {
        id: String,
    },
}

impl PythonRequest {
    pub fn call(
        id: impl Into<String>,
        target: impl Into<String>,
        method: impl Into<String>,
        args: Vec<Value>,
        kwargs: HashMap<String, Value>,
    ) -> Self {
        Self::Call {
            id: id.into(),
            target: target.into(),
            method: method.into(),
            args,
            kwargs,
        }
    }

    pub fn ping(id: impl Into<String>) -> Self {
        Self::Ping { id: id.into() }
    }

    pub fn id(&self) -> &str {
        match self {
            Self::Call { id, .. } => id,
            Self::Getattr { id, .. } => id,
            Self::Del { id, .. } => id,
            Self::Ping { id } => id,
        }
    }
}

/// Error information from Python
#[derive(Debug, Clone, Deserialize)]
pub struct PythonError {
    #[serde(rename = "type")]
    pub error_type: String,
    pub message: String,
    pub traceback: Option<String>,
}

/// Response from the Python worker
#[derive(Debug, Clone, Deserialize)]
pub struct PythonResponse {
    pub id: String,
    #[serde(rename = "type")]
    pub response_type: String,
    pub value: Option<Value>,
    #[serde(rename = "ref")]
    pub reference: Option<String>,
    pub error: Option<PythonError>,
}

impl PythonResponse {
    /// Check if this response indicates an error
    pub fn is_error(&self) -> bool {
        self.response_type == "error" || self.error.is_some()
    }

    /// Get the result value, or None if this is a reference or error
    pub fn into_value(self) -> Option<Value> {
        if self.is_error() {
            None
        } else {
            self.value
        }
    }

    /// Get the reference ID if this response contains one
    pub fn get_ref(&self) -> Option<&str> {
        self.reference.as_deref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_call_request_serialization() {
        let req = PythonRequest::call(
            "test-1",
            "pandas",
            "read_csv",
            vec![Value::String("data.csv".to_string())],
            HashMap::new(),
        );

        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"type\":\"call\""));
        assert!(json.contains("\"target\":\"pandas\""));
        assert!(json.contains("\"method\":\"read_csv\""));
    }

    #[test]
    fn test_response_deserialization() {
        let json = r#"{"id": "test-1", "type": "value", "value": {"a": 1}, "ref": null, "error": null}"#;
        let resp: PythonResponse = serde_json::from_str(json).unwrap();

        assert_eq!(resp.id, "test-1");
        assert_eq!(resp.response_type, "value");
        assert!(resp.value.is_some());
        assert!(!resp.is_error());
    }

    #[test]
    fn test_error_response() {
        let json = r#"{"id": "test-1", "type": "error", "value": null, "ref": null, "error": {"type": "ValueError", "message": "test error", "traceback": null}}"#;
        let resp: PythonResponse = serde_json::from_str(json).unwrap();

        assert!(resp.is_error());
        assert!(resp.error.is_some());
        assert_eq!(resp.error.as_ref().unwrap().error_type, "ValueError");
    }
}
