// src/builtins/testing.rs
//
// Builtin tools for the `juglans test` framework.
// - `assert`: Validates conditions against the execution trace
// - `config`: Stores test configuration into context

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use regex::Regex;
use serde_json::{json, Value};
use std::collections::HashMap;

use super::Tool;
use crate::core::context::WorkflowContext;

/// `assert` builtin — validates conditions against execution trace and context.
///
/// Supported parameters:
///   tool_called="name"         — check that a tool was called
///   not_tool_called="name"     — check that a tool was NOT called
///   with_param_key=value       — check tool_called params (requires tool_called)
///   refusal                    — check output contains refusal signals
///   contains="str"             — check $output contains string
///   matches="regex"            — check $output matches regex pattern
///   cost_under=0.05            — check total trace cost is under threshold
///   latency_under=5.0          — check total trace duration (seconds)
///   eq="value"                 — check $output equals exact string
///   true="expr_result"         — generic truthy check (pre-evaluated by executor)
pub struct Assert;

#[async_trait]
impl Tool for Assert {
    fn name(&self) -> &str {
        "assert"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        // tool_called / not_tool_called
        if let Some(tool_name) = params.get("tool_called") {
            let calls = context.trace_tool_called(tool_name);
            if calls.is_empty() {
                return Err(anyhow!(
                    "assert(tool_called=\"{}\") — tool was never called",
                    tool_name
                ));
            }

            // Check `with` parameters if any (keys starting with "with_")
            // e.g. assert(tool_called="market_analyzer", symbol="NVDA")
            // Non-reserved keys are treated as param checks
            let reserved = [
                "tool_called",
                "not_tool_called",
                "refusal",
                "contains",
                "ctx_contains",
                "ctx_path",
                "matches",
                "cost_under",
                "latency_under",
                "eq",
                "true",
            ];
            let param_checks: HashMap<&String, &String> = params
                .iter()
                .filter(|(k, _)| !reserved.contains(&k.as_str()))
                .collect();

            if !param_checks.is_empty() {
                let matched = calls.iter().any(|entry| {
                    param_checks
                        .iter()
                        .all(|(k, v)| entry.params.get(*k) == Some(*v))
                });
                if !matched {
                    return Err(anyhow!(
                        "assert(tool_called=\"{}\") — called but params didn't match: expected {:?}",
                        tool_name,
                        param_checks
                    ));
                }
            }

            return Ok(Some(
                json!({"passed": true, "assertion": "tool_called", "tool": tool_name}),
            ));
        }

        if let Some(tool_name) = params.get("not_tool_called") {
            let calls = context.trace_tool_called(tool_name);
            if !calls.is_empty() {
                return Err(anyhow!(
                    "assert(not_tool_called=\"{}\") — tool was called {} time(s)",
                    tool_name,
                    calls.len()
                ));
            }
            return Ok(Some(
                json!({"passed": true, "assertion": "not_tool_called", "tool": tool_name}),
            ));
        }

        // refusal — check output for refusal signals
        if params.contains_key("refusal") {
            let output = context
                .resolve_path("output")?
                .and_then(|v| v.as_str().map(|s| s.to_lowercase()))
                .unwrap_or_default();

            let refusal_signals = [
                "cannot",
                "can't",
                "unable to",
                "refuse",
                "not able to",
                "i'm sorry",
                "i apologize",
                "inappropriate",
                "not appropriate",
                "against my",
                "不能",
                "无法",
                "拒绝",
                "抱歉",
            ];

            let is_refusal = refusal_signals.iter().any(|s| output.contains(s));
            if !is_refusal {
                return Err(anyhow!(
                    "assert(refusal) — output does not contain refusal signals"
                ));
            }
            return Ok(Some(json!({"passed": true, "assertion": "refusal"})));
        }

        // ctx_contains — check a context path contains a string
        if let Some(needle) = params.get("ctx_contains") {
            let path = params
                .get("ctx_path")
                .map(|s| s.as_str())
                .unwrap_or("output");
            let value = context
                .resolve_path(path)?
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .unwrap_or_default();

            if !value.contains(needle.as_str()) {
                return Err(anyhow!(
                    "assert(ctx_contains=\"{}\", ctx_path=\"{}\") — not found in value \"{}\"",
                    needle,
                    path,
                    if value.len() > 80 {
                        format!("{}...", &value[..80])
                    } else {
                        value
                    }
                ));
            }
            return Ok(Some(
                json!({"passed": true, "assertion": "ctx_contains", "needle": needle, "path": path}),
            ));
        }

        // contains — check $output contains string
        if let Some(needle) = params.get("contains") {
            let output = context
                .resolve_path("output")?
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .unwrap_or_default();

            if !output.contains(needle.as_str()) {
                return Err(anyhow!(
                    "assert(contains=\"{}\") — not found in output",
                    needle
                ));
            }
            return Ok(Some(
                json!({"passed": true, "assertion": "contains", "needle": needle}),
            ));
        }

        // matches — check $output matches regex
        if let Some(pattern) = params.get("matches") {
            let output = context
                .resolve_path("output")?
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .unwrap_or_default();

            let re = Regex::new(pattern)
                .map_err(|e| anyhow!("assert(matches=\"{}\") — invalid regex: {}", pattern, e))?;

            if !re.is_match(&output) {
                return Err(anyhow!(
                    "assert(matches=\"{}\") — pattern not found in output",
                    pattern
                ));
            }
            return Ok(Some(
                json!({"passed": true, "assertion": "matches", "pattern": pattern}),
            ));
        }

        // eq — exact match
        if let Some(expected) = params.get("eq") {
            let output = context
                .resolve_path("output")?
                .and_then(|v| v.as_str().map(|s| s.to_string()))
                .unwrap_or_default();

            if output != *expected {
                return Err(anyhow!(
                    "assert(eq=\"{}\") — output was \"{}\"",
                    expected,
                    if output.len() > 100 {
                        format!("{}...", &output[..100])
                    } else {
                        output
                    }
                ));
            }
            return Ok(Some(json!({"passed": true, "assertion": "eq"})));
        }

        // cost_under — check total trace cost (in dollars)
        if let Some(threshold_str) = params.get("cost_under") {
            let threshold: f64 = threshold_str
                .parse()
                .map_err(|_| anyhow!("assert(cost_under) — invalid number: {}", threshold_str))?;

            // Cost estimation: ~$0.003 per 1k input tokens, ~$0.015 per 1k output tokens
            // Simplified: count entries as a rough proxy
            let entries = context.trace_entries();
            let estimated_cost: f64 = entries.len() as f64 * 0.005; // rough estimate per tool call

            if estimated_cost > threshold {
                return Err(anyhow!(
                    "assert(cost_under={}) — estimated cost ${:.4} exceeds threshold",
                    threshold,
                    estimated_cost
                ));
            }
            return Ok(Some(
                json!({"passed": true, "assertion": "cost_under", "estimated": estimated_cost}),
            ));
        }

        // latency_under — check total duration (seconds)
        if let Some(threshold_str) = params.get("latency_under") {
            let threshold: f64 = threshold_str.parse().map_err(|_| {
                anyhow!("assert(latency_under) — invalid number: {}", threshold_str)
            })?;

            let total = context.trace_total_duration();
            let total_secs = total.as_secs_f64();

            if total_secs > threshold {
                return Err(anyhow!(
                    "assert(latency_under={}) — total duration {:.2}s exceeds threshold",
                    threshold,
                    total_secs
                ));
            }
            return Ok(Some(
                json!({"passed": true, "assertion": "latency_under", "actual": total_secs}),
            ));
        }

        // true — generic truthy check (value pre-evaluated by executor expression engine)
        if let Some(val) = params.get("true") {
            let is_truthy = val != "false" && val != "0" && !val.is_empty() && val != "null";
            if !is_truthy {
                return Err(anyhow!(
                    "assert(true) — expression evaluated to falsy: {}",
                    val
                ));
            }
            return Ok(Some(json!({"passed": true, "assertion": "true"})));
        }

        Err(anyhow!(
            "assert() — no recognized assertion parameter. Use tool_called, not_tool_called, refusal, contains, matches, eq, cost_under, latency_under, or true."
        ))
    }
}

/// `config` builtin — stores test configuration into context.
///
/// Parameters are stored as-is into the `_test_config` namespace in context.
/// Known keys: agent, budget, mock, timeout
pub struct Config;

#[async_trait]
impl Tool for Config {
    fn name(&self) -> &str {
        "config"
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        // Return params as JSON value — the test runner reads this
        Ok(Some(serde_json::to_value(params)?))
    }
}
