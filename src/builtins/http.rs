// src/builtins/http.rs
//
// HTTP backend builtins: serve() and response()
//
// serve()    — Marks entry node; web server registers catch-all route upon discovery
// response() — Controls HTTP response status/body/headers

use super::Tool;
use crate::core::context::WorkflowContext;
use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;

/// serve() — HTTP entry point marker
///
/// At startup, the web server scans all .jg/.jgflow files; upon finding a node
/// containing serve(), it registers that workflow as a catch-all HTTP handler.
///
/// At runtime, acts as pass-through returning request summary for debugging.
/// Request data is pre-injected into $input.* by the web server.
pub struct Serve;

#[async_trait]
impl Tool for Serve {
    fn name(&self) -> &str {
        "serve"
    }

    async fn execute(
        &self,
        _params: &HashMap<String, String>,
        context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        // pass-through: return request data summary
        let method = context
            .resolve_path("input.method")
            .ok()
            .flatten()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "unknown".to_string());
        let path = context
            .resolve_path("input.path")
            .ok()
            .flatten()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "/".to_string());
        let query = context.resolve_path("input.query").ok().flatten();
        let has_body = context
            .resolve_path("input.body")
            .ok()
            .flatten()
            .map(|v| !v.is_null())
            .unwrap_or(false);

        // Pre-compute $input.route = "METHOD /path" for convenient switch routing
        let route = format!("{} {}", method, path);
        context.set("input.route".to_string(), json!(route)).ok();

        Ok(Some(json!({
            "method": method,
            "path": path,
            "route": route,
            "query": query,
            "has_body": has_body,
        })))
    }
}

/// response(status=200, body=$output, headers={"X-Custom": "value"})
///
/// Set HTTP response. Writes to $response.status / $response.body / $response.headers
/// for the web server to read after workflow execution completes.
///
/// If the workflow does not call response(), the web server defaults to $output as body with status 200.
pub struct HttpResponse;

#[async_trait]
impl Tool for HttpResponse {
    fn name(&self) -> &str {
        "response"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let status: u16 = params
            .get("status")
            .and_then(|s| s.trim_matches('"').parse().ok())
            .unwrap_or(200);

        let body: Option<Value> = params
            .get("body")
            .map(|b| serde_json::from_str(b).unwrap_or(json!(b)));

        let headers: Option<Value> = params
            .get("headers")
            .and_then(|h| serde_json::from_str(h).ok());

        let file: Option<String> = params.get("file").map(|f| f.trim_matches('"').to_string());

        // Write to $response.* for web server to read
        context.set("response.status".to_string(), json!(status))?;
        if let Some(ref b) = body {
            context.set("response.body".to_string(), b.clone())?;
        }
        if let Some(ref h) = headers {
            context.set("response.headers".to_string(), h.clone())?;
        }
        if let Some(ref f) = file {
            context.set("response.file".to_string(), json!(f))?;
        }

        Ok(body.or(Some(json!({"status": status}))))
    }
}
