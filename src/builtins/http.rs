// src/builtins/http.rs
//
// HTTP backend builtins: serve() and response()
//
// serve()    — 标记入口节点，web server 扫描到即注册 catch-all 路由
// response() — 控制 HTTP 响应 status/body/headers

use super::Tool;
use crate::core::context::WorkflowContext;
use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;

/// serve() — HTTP 入口标记
///
/// web server 启动时扫描所有 .jg/.jgflow，发现含 serve() 的节点后，
/// 将该 workflow 注册为 catch-all HTTP handler。
///
/// 运行时作为 pass-through，返回请求摘要供调试。
/// 请求数据由 web server 预注入到 $input.*
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
        // pass-through: 返回请求数据摘要
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

        // 预计算 $input.route = "METHOD /path"，方便 switch 路由
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
/// 设置 HTTP 响应。写入 $response.status / $response.body / $response.headers
/// 供 web server 在 workflow 执行完毕后读取。
///
/// 如果 workflow 未调用 response()，web server 默认用 $output 作为 body，status 200。
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

        // 写入 $response.* 供 web server 读取
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
