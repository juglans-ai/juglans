use anyhow::{anyhow, Context};
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::mpsc;
use tracing::debug;

use super::messages::{Attachment, AttachmentKind};

// ---------------------------------------------------------------------------
// ClaudeEvent — events sent from the background parser task to the TUI
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum ClaudeEvent {
    /// Incremental assistant text token
    TextDelta(String),
    /// Incremental thinking token
    ThinkingDelta(String),
    /// A tool_use content block started
    ToolUseStart { _id: String, name: String },
    /// Incremental JSON for the current tool_use input
    ToolInputDelta(String),
    /// A content block ended
    ContentBlockStop { _index: u32 },
    /// One assistant turn ended (may be followed by another after tool use)
    MessageStop,
    /// Complete assistant message snapshot — replaces streaming content
    AssistantSnapshot { content: String },
    /// Final result with cost / usage
    Result {
        cost_usd: f64,
        _duration_ms: u64,
        input_tokens: u64,
        output_tokens: u64,
        session_id: String,
    },
    /// Permission request from claude subprocess (needs user approval)
    PermissionRequest {
        request_id: String,
        _tool_name: String,
        _input: serde_json::Value,
    },
    /// The claude subprocess exited
    ProcessExited {
        _success: bool,
        error: Option<String>,
    },
}

// ---------------------------------------------------------------------------
// ClaudeProcess — handle to the running subprocess (drop = kill)
// ---------------------------------------------------------------------------

pub struct ClaudeProcess {
    child: Child,
    stdin: Option<ChildStdin>,
}

impl ClaudeProcess {
    pub fn kill(&mut self) {
        let _ = self.child.start_kill();
    }

    /// Send a NDJSON line to the claude subprocess via stdin.
    pub async fn send_response(&mut self, json_line: &str) -> anyhow::Result<()> {
        if let Some(stdin) = &mut self.stdin {
            stdin
                .write_all(format!("{}\n", json_line).as_bytes())
                .await?;
            stdin.flush().await?;
            Ok(())
        } else {
            Err(anyhow!("stdin not available"))
        }
    }
}

impl Drop for ClaudeProcess {
    fn drop(&mut self) {
        self.kill();
    }
}

// ---------------------------------------------------------------------------
// Serde types for NDJSON parsing
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum StreamLine {
    #[serde(rename = "stream_event")]
    StreamEvent {
        #[allow(dead_code)]
        session_id: Option<String>,
        event: StreamInnerEvent,
    },
    #[serde(rename = "assistant")]
    #[allow(dead_code)]
    Assistant { message: serde_json::Value },
    #[serde(rename = "result")]
    ResultLine {
        #[allow(dead_code)]
        #[serde(default)]
        result: Option<String>,
        #[serde(default)]
        total_cost_usd: Option<f64>,
        #[serde(default)]
        duration_ms: Option<u64>,
        #[serde(default)]
        session_id: Option<String>,
        #[serde(default)]
        usage: Option<UsagePayload>,
    },
    #[serde(rename = "control_request")]
    ControlRequest {
        request_id: String,
        request: ControlRequestBody,
    },
    #[serde(rename = "control_response")]
    #[allow(dead_code)]
    ControlResponse { response: serde_json::Value },
    #[serde(rename = "system")]
    #[allow(dead_code)]
    System {
        #[serde(default)]
        subtype: Option<String>,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "subtype")]
enum ControlRequestBody {
    #[serde(rename = "can_use_tool")]
    CanUseTool {
        tool_name: String,
        #[serde(default)]
        input: serde_json::Value,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum StreamInnerEvent {
    #[serde(rename = "content_block_start")]
    ContentBlockStart {
        #[allow(dead_code)]
        index: u32,
        content_block: ContentBlock,
    },
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta {
        #[allow(dead_code)]
        index: u32,
        delta: Delta,
    },
    #[serde(rename = "content_block_stop")]
    ContentBlockStop { index: u32 },
    #[serde(rename = "message_start")]
    #[allow(dead_code)]
    MessageStart { message: serde_json::Value },
    #[serde(rename = "message_delta")]
    #[allow(dead_code)]
    MessageDelta {
        delta: serde_json::Value,
        usage: Option<serde_json::Value>,
    },
    #[serde(rename = "message_stop")]
    MessageStop,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum ContentBlock {
    #[serde(rename = "text")]
    #[allow(dead_code)]
    Text { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        #[allow(dead_code)]
        input: serde_json::Value,
    },
    #[serde(rename = "thinking")]
    #[allow(dead_code)]
    Thinking { thinking: String },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum Delta {
    #[serde(rename = "text_delta")]
    Text { text: String },
    #[serde(rename = "input_json_delta")]
    InputJson { partial_json: String },
    #[serde(rename = "thinking_delta")]
    Thinking { thinking: String },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
struct UsagePayload {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

// ---------------------------------------------------------------------------
// spawn_claude — launch the subprocess and return handle + event receiver
// ---------------------------------------------------------------------------

/// Build the `content` field for a user message.
/// Without attachments: plain string (backward compatible).
/// With attachments: array of content blocks.
pub fn build_content_blocks(text: &str, attachments: &[Attachment]) -> Value {
    if attachments.is_empty() {
        return Value::String(text.to_string());
    }

    let mut blocks: Vec<Value> = Vec::new();

    for att in attachments {
        match &att.kind {
            AttachmentKind::Image {
                media_type,
                base64_data,
            } => {
                blocks.push(json!({
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": media_type,
                        "data": base64_data,
                    }
                }));
            }
            AttachmentKind::TextFile { content } => {
                blocks.push(json!({
                    "type": "text",
                    "text": format!("<file name=\"{}\">\n{}\n</file>", att.file_name, content)
                }));
            }
        }
    }

    if !text.is_empty() {
        blocks.push(json!({
            "type": "text",
            "text": text,
        }));
    }

    Value::Array(blocks)
}

pub async fn spawn_claude(
    prompt: &str,
    model: &str,
    session_id: Option<&str>,
    cwd: &str,
    attachments: &[Attachment],
) -> anyhow::Result<(ClaudeProcess, mpsc::UnboundedReceiver<ClaudeEvent>)> {
    let claude_bin = std::env::var("CLAUDE_BIN").unwrap_or_else(|_| "claude".to_string());

    let mut cmd = Command::new(&claude_bin);
    cmd.arg("--output-format")
        .arg("stream-json")
        .arg("--input-format")
        .arg("stream-json")
        .arg("--include-partial-messages")
        .arg("--verbose")
        .arg("--dangerously-skip-permissions")
        .arg("--model")
        .arg(map_model_name(model));

    if let Some(sid) = session_id {
        cmd.arg("--resume").arg(sid);
    }

    // Resolve ~ in cwd
    let real_cwd = if let Some(stripped) = cwd.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            format!("{}/{}", home.to_string_lossy(), stripped)
        } else {
            cwd.to_string()
        }
    } else {
        cwd.to_string()
    };
    cmd.current_dir(&real_cwd);

    // CRITICAL: prevent "nested Claude Code session" error
    cmd.env_remove("CLAUDECODE");

    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());
    cmd.stdin(std::process::Stdio::piped());

    let mut child = cmd
        .spawn()
        .with_context(|| format!("Failed to spawn claude at `{}`", claude_bin))?;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("Failed to capture claude stdin"))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow!("Failed to capture claude stdout"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("Failed to capture claude stderr"))?;

    // Send initialization control request
    let init_msg = json!({
        "type": "control_request",
        "request_id": "init_1",
        "request": {
            "subtype": "initialize",
            "hooks": null,
            "agents": null
        }
    });
    stdin
        .write_all(format!("{}\n", init_msg).as_bytes())
        .await?;
    stdin.flush().await?;

    // Send user message
    let content = build_content_blocks(prompt, attachments);
    let user_msg = json!({
        "type": "user",
        "session_id": session_id.unwrap_or(""),
        "message": {
            "role": "user",
            "content": content
        },
        "parent_tool_use_id": null
    });
    stdin
        .write_all(format!("{}\n", user_msg).as_bytes())
        .await?;
    stdin.flush().await?;

    let (tx, rx) = mpsc::unbounded_channel();

    // Task 1: parse stdout NDJSON → ClaudeEvents
    let tx_stdout = tx.clone();
    tokio::spawn(async move {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        while let Ok(Some(line)) = lines.next_line().await {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            debug!("claude stdout: {}", trimmed);

            match serde_json::from_str::<StreamLine>(trimmed) {
                Ok(parsed) => {
                    for ev in translate(parsed) {
                        if tx_stdout.send(ev).is_err() {
                            return;
                        }
                    }
                }
                Err(e) => {
                    debug!("NDJSON parse skip: {} — line: {}", e, trimmed);
                }
            }
        }
    });

    // Task 2: collect stderr, then send ProcessExited
    let tx_stderr = tx;
    tokio::spawn(async move {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();
        let mut buf = String::new();
        while let Ok(Some(line)) = lines.next_line().await {
            if !buf.is_empty() {
                buf.push('\n');
            }
            buf.push_str(&line);
        }
        let success = buf.is_empty();
        let error = if success { None } else { Some(buf) };
        let _ = tx_stderr.send(ClaudeEvent::ProcessExited {
            _success: success,
            error,
        });
    });

    Ok((
        ClaudeProcess {
            child,
            stdin: Some(stdin),
        },
        rx,
    ))
}

// ---------------------------------------------------------------------------
// translate — convert a parsed StreamLine into ClaudeEvents
// ---------------------------------------------------------------------------

fn translate(line: StreamLine) -> Vec<ClaudeEvent> {
    match line {
        StreamLine::StreamEvent { event, .. } => match event {
            StreamInnerEvent::ContentBlockStart {
                content_block: ContentBlock::ToolUse { id, name, .. },
                ..
            } => {
                vec![ClaudeEvent::ToolUseStart { _id: id, name }]
            }
            StreamInnerEvent::ContentBlockStart { .. } => vec![],
            StreamInnerEvent::ContentBlockDelta { delta, .. } => match delta {
                Delta::Text { text } => vec![ClaudeEvent::TextDelta(text)],
                Delta::Thinking { thinking } => vec![ClaudeEvent::ThinkingDelta(thinking)],
                Delta::InputJson { partial_json } => {
                    vec![ClaudeEvent::ToolInputDelta(partial_json)]
                }
                Delta::Unknown => vec![],
            },
            StreamInnerEvent::ContentBlockStop { index } => {
                vec![ClaudeEvent::ContentBlockStop { _index: index }]
            }
            StreamInnerEvent::MessageStop => vec![ClaudeEvent::MessageStop],
            _ => vec![],
        },
        StreamLine::ResultLine {
            total_cost_usd,
            duration_ms,
            session_id,
            usage,
            ..
        } => {
            vec![ClaudeEvent::Result {
                cost_usd: total_cost_usd.unwrap_or(0.0),
                _duration_ms: duration_ms.unwrap_or(0),
                input_tokens: usage.as_ref().and_then(|u| u.input_tokens).unwrap_or(0),
                output_tokens: usage.as_ref().and_then(|u| u.output_tokens).unwrap_or(0),
                session_id: session_id.unwrap_or_default(),
            }]
        }
        StreamLine::ControlRequest {
            request_id,
            request,
        } => match request {
            ControlRequestBody::CanUseTool {
                tool_name, input, ..
            } => {
                vec![ClaudeEvent::PermissionRequest {
                    request_id,
                    _tool_name: tool_name,
                    _input: input,
                }]
            }
            ControlRequestBody::Unknown => vec![],
        },
        StreamLine::Assistant { message } => {
            // Complete assistant snapshot — extract all text and emit as
            // AssistantSnapshot to replace prior streaming content.
            let mut full_text = String::new();
            if let Some(content) = message.get("content").and_then(|c| c.as_array()) {
                for block in content {
                    if block.get("type").and_then(|t| t.as_str()) == Some("text") {
                        if let Some(text) = block.get("text").and_then(|t| t.as_str()) {
                            if !text.is_empty() {
                                full_text.push_str(text);
                            }
                        }
                    }
                }
            }
            if full_text.is_empty() {
                vec![]
            } else {
                vec![ClaudeEvent::AssistantSnapshot { content: full_text }]
            }
        }
        _ => vec![],
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Find the largest byte index <= `i` that is a valid char boundary.
/// Equivalent to `str::floor_char_boundary` (stable since 1.91).
pub fn floor_char_boundary(s: &str, i: usize) -> usize {
    if i >= s.len() {
        s.len()
    } else {
        let mut pos = i;
        while pos > 0 && !s.is_char_boundary(pos) {
            pos -= 1;
        }
        pos
    }
}

/// Build a JSON control_response for a permission request.
pub fn build_permission_response(request_id: &str, allow: bool) -> String {
    if allow {
        json!({
            "type": "control_response",
            "response": {
                "subtype": "success",
                "request_id": request_id,
                "response": {
                    "behavior": "allow"
                }
            }
        })
        .to_string()
    } else {
        json!({
            "type": "control_response",
            "response": {
                "subtype": "success",
                "request_id": request_id,
                "response": {
                    "behavior": "deny",
                    "message": "User denied this action"
                }
            }
        })
        .to_string()
    }
}

/// Map full model IDs (from the TUI picker) to short names accepted by claude CLI.
fn map_model_name(model: &str) -> &str {
    if model.contains("opus") {
        "opus"
    } else if model.contains("haiku") {
        "haiku"
    } else if model.contains("sonnet") {
        "sonnet"
    } else {
        model
    }
}

/// Try to pretty-format partial JSON for tool params display.
pub fn format_tool_params(raw: &str) -> String {
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(val) => {
            if let Some(obj) = val.as_object() {
                obj.iter()
                    .map(|(k, v)| {
                        let v_str = match v {
                            serde_json::Value::String(s) => {
                                if s.len() > 120 {
                                    let end = floor_char_boundary(s, 120);
                                    format!("{}…", &s[..end])
                                } else {
                                    s.clone()
                                }
                            }
                            other => {
                                let s = other.to_string();
                                if s.len() > 120 {
                                    let end = floor_char_boundary(&s, 120);
                                    format!("{}…", &s[..end])
                                } else {
                                    s
                                }
                            }
                        };
                        format!("{}: {}", k, v_str)
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                raw.to_string()
            }
        }
        Err(_) => raw.to_string(),
    }
}
