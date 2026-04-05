// src/builtins/http.rs
//
// HTTP backend builtins: serve() and response()
//
// serve()    — Starts inline HTTP server (CLI mode) or pass-through (request mode)
// response() — Controls HTTP response status/body/headers

use super::Tool;
use crate::core::context::WorkflowContext;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::info;

/// serve() — HTTP server entry point
///
/// CLI mode (no input.method): extracts routes from decorator nodes, starts an
/// inline Axum server that dispatches requests to handler functions. Blocks forever.
///
/// Request mode (input.method set): pass-through that returns request summary.
/// Request data is pre-injected into $input.* by the server.
pub struct Serve {
    builtin_registry: Option<std::sync::Weak<super::BuiltinRegistry>>,
}

impl Default for Serve {
    fn default() -> Self {
        Self::new()
    }
}

impl Serve {
    pub fn new() -> Self {
        Self {
            builtin_registry: None,
        }
    }

    pub fn set_registry(&mut self, registry: std::sync::Weak<super::BuiltinRegistry>) {
        self.builtin_registry = Some(registry);
    }
}

/// Route extracted from decorator nodes in the workflow graph
#[derive(Clone, Debug)]
pub struct InlineRoute {
    // fields are pub for web_server access
    pub method: String,
    pub path: String,
    pub handler: String,
}

/// Scan workflow for route declarations: function annotations (new) + _deco_N nodes (legacy).
pub fn extract_routes_from_graph(workflow: &crate::core::graph::WorkflowGraph) -> Vec<InlineRoute> {
    let mut routes = Vec::new();

    // New: scan function annotations for route metadata (from @get/@post decorators)
    for (fn_name, fn_def) in &workflow.functions {
        if let Some(route_val) = fn_def.annotations.get("route") {
            let method = route_val["method"].as_str().unwrap_or("GET").to_uppercase();
            let path = route_val["path"].as_str().unwrap_or("/").to_string();
            if !path.is_empty() {
                routes.push(InlineRoute {
                    method,
                    path,
                    handler: fn_name.clone(),
                });
            }
        }
    }

    routes
}

#[async_trait]
impl Tool for Serve {
    fn name(&self) -> &str {
        "serve"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        // Check if we're in request-handling mode (input.method is already set)
        let existing_method = context
            .resolve_path("input.method")
            .ok()
            .flatten()
            .and_then(|v| v.as_str().map(|s| s.to_string()));

        if let Some(method) = existing_method {
            // Pass-through mode: return request data summary (called from inline server handler)
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

            let route = format!("{} {}", method, path);
            context.set("input.route".to_string(), json!(route)).ok();

            return Ok(Some(json!({
                "method": method,
                "path": path,
                "route": route,
                "query": query,
                "has_body": has_body,
            })));
        }

        // Server mode: start inline HTTP server
        let registry = self
            .builtin_registry
            .as_ref()
            .and_then(|w| w.upgrade())
            .ok_or_else(|| anyhow!("serve(): BuiltinRegistry not available"))?;
        let executor = registry
            .get_executor()
            .ok_or_else(|| anyhow!("serve(): WorkflowExecutor not available"))?;
        let runtime = registry.get_runtime();
        let workflow = context
            .get_root_workflow()
            .ok_or_else(|| anyhow!("serve(): no root workflow found"))?;

        // Extract routes from decorator nodes
        let routes = extract_routes_from_graph(&workflow);
        if routes.is_empty() {
            info!("serve(): no decorator routes found, starting pass-through server");
        }

        for r in &routes {
            info!("  📌 {} {} -> {}()", r.method, r.path, r.handler);
        }

        // Get port from params, config, or default
        let port = params
            .get("port")
            .and_then(|p| p.trim_matches('"').parse::<u16>().ok())
            .or_else(|| {
                context
                    .resolve_path("config.server.port")
                    .ok()
                    .flatten()
                    .and_then(|v| v.as_u64())
                    .map(|p| p as u16)
            })
            .unwrap_or(8080);

        // Start server (blocks indefinitely)
        crate::services::web_server::start_inline_server(routes, workflow, executor, runtime, port)
            .await?;

        Ok(None) // never reached
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
