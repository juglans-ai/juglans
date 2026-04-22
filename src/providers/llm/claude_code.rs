// src/providers/llm/claude_code.rs
//
// Claude Code CLI as LLM provider.
// No tools: --print mode. With tools: SDK stream + HTTP MCP.
// MCP tools/call → yield SSE tool_call → wait for tool_result → return to Claude.

use super::mcp_types::{self, McpSession, PendingToolCall};
use super::{ChatStreamChunk, LlmProvider, Message, MessagePart, TokenUsage, ToolCallChunk};
use anyhow::Result;
use async_trait::async_trait;
use dashmap::DashMap;
use futures::Stream;
use serde::Deserialize;
use serde_json::{json, Value};
use std::pin::Pin;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;

// NDJSON types
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum StreamLine {
    #[serde(rename = "stream_event")]
    StreamEvent { event: StreamInnerEvent },
    #[serde(rename = "assistant")]
    Assistant { message: Value },
    #[serde(rename = "result")]
    ResultLine {
        #[serde(default)]
        usage: Option<UsagePayload>,
    },
    #[serde(rename = "system")]
    System {
        #[serde(default)]
        mcp_servers: Option<Value>,
        #[serde(default)]
        tools: Option<Value>,
    },
    #[serde(rename = "control_request")]
    ControlRequest {
        request_id: String,
        request: ControlRequestBody,
    },
    #[serde(other)]
    Unknown,
}
#[derive(Debug, Deserialize)]
#[serde(tag = "subtype")]
enum ControlRequestBody {
    #[serde(rename = "can_use_tool")]
    CanUseTool {
        #[allow(dead_code)]
        tool_name: String,
    },
    #[serde(other)]
    Unknown,
}
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum StreamInnerEvent {
    #[serde(rename = "content_block_delta")]
    ContentBlockDelta { delta: Delta },
    #[serde(rename = "message_stop")]
    MessageStop,
    #[serde(other)]
    Unknown,
}
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
enum Delta {
    #[serde(rename = "text_delta")]
    Text { text: String },
    #[serde(other)]
    Unknown,
}
#[derive(Debug, Deserialize)]
struct UsagePayload {
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
}

enum InternalEvent {
    TextDelta(String),
    /// Full text from Assistant message snapshot (--print mode only, skipped if deltas received)
    AssistantText(String),
    PermissionRequest {
        request_id: String,
    },
    Result {
        input_tokens: u64,
        output_tokens: u64,
    },
    Error(String),
    Done,
}

// Provider
pub struct ClaudeCodeProvider {
    claude_bin: String,
    cwd: String,
    tool_sessions: Arc<DashMap<String, McpSession>>,
    server_port: u16,
}

impl ClaudeCodeProvider {
    pub fn new() -> Self {
        Self {
            claude_bin: std::env::var("CLAUDE_BIN").unwrap_or_else(|_| "claude".to_string()),
            cwd: std::env::var("CLAUDE_CODE_CWD").unwrap_or_else(|_| "/tmp".to_string()),
            tool_sessions: Arc::new(DashMap::new()),
            server_port: std::env::var("PORT")
                .unwrap_or_else(|_| "3000".to_string())
                .parse()
                .unwrap_or(3000),
        }
    }
    pub fn set_tool_sessions(&mut self, s: Arc<DashMap<String, McpSession>>) {
        self.tool_sessions = s;
    }
    fn map_model(model: &str) -> &str {
        let m = model.to_lowercase();
        if m.contains("opus") {
            "opus"
        } else if m.contains("haiku") {
            "haiku"
        } else {
            "sonnet"
        }
    }
    fn build_prompt(history: &[Message]) -> String {
        let mut parts: Vec<(String, String)> = Vec::new();
        for msg in history {
            let content = serde_json::from_value::<Vec<MessagePart>>(msg.parts.clone())
                .map(|ps| {
                    ps.iter()
                        .filter_map(|p| p.content.clone())
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .unwrap_or_default();
            if !content.is_empty() {
                parts.push((msg.role.clone(), content));
            }
        }
        if parts.is_empty() {
            return String::new();
        }
        if parts.len() == 1 {
            return parts[0].1.clone();
        }
        parts
            .iter()
            .map(|(r, c)| {
                let l = match r.as_str() {
                    "user" => "User",
                    "assistant" => "Assistant",
                    "tool" => "Tool Result",
                    _ => "System",
                };
                format!("[{}]\n{}\n\n", l, c)
            })
            .collect()
    }
    fn resolve_cwd(&self) -> String {
        if let Some(s) = self.cwd.strip_prefix("~/") {
            if let Some(h) = std::env::var_os("HOME") {
                return format!("{}/{}", h.to_string_lossy(), s);
            }
        }
        self.cwd.clone()
    }
}

#[async_trait]
impl LlmProvider for ClaudeCodeProvider {
    async fn stream_chat(
        &self,
        model: &str,
        system_prompt: Option<String>,
        history: Vec<Message>,
        tools: Option<Vec<Value>>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ChatStreamChunk>> + Send>>> {
        let prompt = Self::build_prompt(&history);
        let real_cwd = self.resolve_cwd();
        let cli_model = Self::map_model(model);
        let has_tools = tools.as_ref().map(|t| !t.is_empty()).unwrap_or(false);

        // Register MCP session with channels
        let session_id = uuid::Uuid::new_v4().to_string();
        let mut tool_call_rx: Option<mpsc::UnboundedReceiver<PendingToolCall>> = None;
        let mcp_config_path = if has_tools {
            let mcp_tools = mcp_types::openai_tools_to_mcp(tools.as_ref().unwrap());
            let (tc_tx, tc_rx) = mpsc::unbounded_channel::<PendingToolCall>();
            tool_call_rx = Some(tc_rx);
            let result_senders = Arc::new(DashMap::new());

            tracing::info!(
                "[claude-code] MCP session {} with {} tools",
                session_id,
                mcp_tools.len()
            );
            self.tool_sessions.insert(
                session_id.clone(),
                McpSession {
                    tools: mcp_tools,
                    tool_call_tx: tc_tx,
                    result_senders,
                },
            );

            let path = format!("/tmp/juglans_mcp_{}.json", session_id);
            let cfg = json!({"mcpServers": {"juglans": {"type": "http", "url": format!("http://127.0.0.1:{}/mcp/{}", self.server_port, session_id)}}});
            tokio::fs::write(&path, serde_json::to_string(&cfg)?).await?;
            Some(path)
        } else {
            None
        };

        let mut cmd = Command::new(&self.claude_bin);
        if has_tools {
            cmd.arg("--output-format")
                .arg("stream-json")
                .arg("--input-format")
                .arg("stream-json")
                .arg("--verbose")
                .arg("--include-partial-messages")
                .arg("--dangerously-skip-permissions")
                .arg("--model")
                .arg(cli_model)
                .arg("--tools=")
                .arg("--mcp-config")
                .arg(mcp_config_path.as_ref().unwrap())
                .arg("--allowed-tools")
                .arg("mcp__juglans__*");
        } else {
            cmd.arg("--print")
                .arg("--output-format")
                .arg("stream-json")
                .arg("--verbose")
                .arg("--include-partial-messages")
                .arg("--dangerously-skip-permissions")
                .arg("--model")
                .arg(cli_model);
        }
        if let Some(ref sp) = system_prompt {
            if !sp.trim().is_empty() {
                cmd.arg("--system-prompt").arg(sp);
            }
        }
        if !has_tools {
            cmd.arg(&prompt);
        }
        cmd.current_dir(&real_cwd)
            .env_remove("CLAUDECODE")
            .env_remove("ANTHROPIC_API_KEY")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        if has_tools {
            cmd.stdin(std::process::Stdio::piped());
        } else {
            cmd.stdin(std::process::Stdio::null());
        }

        tracing::info!(
            "[claude-code] Spawning (tools={}) prompt_len={}",
            has_tools,
            prompt.len()
        );

        let mut child = cmd
            .spawn()
            .map_err(|e| anyhow::anyhow!("Failed to spawn claude: {}", e))?;

        // SDK mode: send user message, keep stdin open until signaled
        let (stdin_close_tx, mut stdin_close_rx) = mpsc::channel::<()>(1);
        if has_tools {
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| anyhow::anyhow!("No stdin"))?;
            let p = prompt.clone();
            tokio::spawn(async move {
                let msg = json!({"type": "user", "message": {"role": "user", "content": p}});
                let _ = stdin.write_all(format!("{}\n", msg).as_bytes()).await;
                let _ = stdin.flush().await;
                // Keep stdin open until close signal (on Result event)
                let _ = stdin_close_rx.recv().await;
                drop(stdin);
            });
        }

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("No stdout"))?;
        let stderr = child
            .stderr
            .take()
            .ok_or_else(|| anyhow::anyhow!("No stderr"))?;

        let (tx, rx) = mpsc::unbounded_channel::<InternalEvent>();
        let tx1 = tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let t = line.trim();
                if t.is_empty() {
                    continue;
                }
                if let Ok(parsed) = serde_json::from_str::<StreamLine>(t) {
                    for ev in translate(parsed) {
                        if tx1.send(ev).is_err() {
                            return;
                        }
                    }
                }
            }
            let _ = tx1.send(InternalEvent::Done);
        });
        let tx2 = tx.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            let mut buf = String::new();
            while let Ok(Some(l)) = lines.next_line().await {
                if !buf.is_empty() {
                    buf.push('\n');
                }
                buf.push_str(&l);
            }
            if !buf.is_empty() {
                let _ = tx2.send(InternalEvent::Error(buf));
            }
        });
        let _tx3 = tx;
        tokio::spawn(async move {
            let _ = child.wait().await;
        });

        // Produce ChatStreamChunk — merge NDJSON events + tool_call notifications
        let (chunk_tx, chunk_rx) = mpsc::unbounded_channel::<Result<ChatStreamChunk>>();
        let cleanup_id = session_id.clone();
        let cleanup_sessions = self.tool_sessions.clone();
        let cleanup_path = mcp_config_path;

        tokio::spawn(async move {
            let mut irx = rx;
            let mut tcrx = tool_call_rx;
            let mut has_streamed_deltas = false;

            loop {
                tokio::select! {
                    // NDJSON events from claude stdout
                    ev = irx.recv() => {
                        match ev {
                            Some(InternalEvent::TextDelta(t)) => {
                                has_streamed_deltas = true;
                                let _ = chunk_tx.send(Ok(ChatStreamChunk { content: Some(t), tool_calls: vec![], usage: None, finish_reason: None }));
                            }
                            Some(InternalEvent::AssistantText(t)) => {
                                // Only emit if no streaming deltas received (--print mode)
                                if !has_streamed_deltas {
                                    let _ = chunk_tx.send(Ok(ChatStreamChunk { content: Some(t), tool_calls: vec![], usage: None, finish_reason: None }));
                                }
                            }
                            Some(InternalEvent::Result { input_tokens, output_tokens }) => {
                                let _ = chunk_tx.send(Ok(ChatStreamChunk {
                                    content: None, tool_calls: vec![],
                                    usage: Some(TokenUsage { input_tokens: input_tokens as i64, output_tokens: output_tokens as i64, total_tokens: (input_tokens + output_tokens) as i64 }),
                                    finish_reason: Some("stop".to_string()),
                                }));
                                // Close stdin to let claude process exit
                                let _ = stdin_close_tx.send(()).await;
                            }
                            Some(InternalEvent::PermissionRequest { .. }) => {}
                            Some(InternalEvent::Error(msg)) => { tracing::warn!("claude-code stderr: {}", msg); }
                            Some(InternalEvent::Done) | None => { break; }
                        }
                    }
                    // Tool call notifications from MCP endpoint
                    tc = async {
                        if let Some(ref mut rx) = tcrx { rx.recv().await } else { std::future::pending().await }
                    } => {
                        if let Some(pending) = tc {
                            tracing::info!("[claude-code] Tool call: {} ({})", pending.name, pending.call_id);
                            let _ = chunk_tx.send(Ok(ChatStreamChunk {
                                content: None,
                                tool_calls: vec![ToolCallChunk {
                                    index: 0,
                                    id: Some(pending.call_id),
                                    name: Some(pending.name),
                                    arguments: Some(pending.arguments),
                                    signature: None,
                                }],
                                usage: None,
                                finish_reason: Some("tool_use".to_string()),
                            }));
                        }
                    }
                }
            }

            // Cleanup
            cleanup_sessions.remove(&cleanup_id);
            if let Some(p) = cleanup_path {
                let _ = tokio::fs::remove_file(p).await;
            }
            drop(chunk_tx);
        });

        Ok(Box::pin(UnboundedReceiverStream::new(chunk_rx)))
    }
}

fn translate(line: StreamLine) -> Vec<InternalEvent> {
    match line {
        StreamLine::StreamEvent { event } => match event {
            StreamInnerEvent::ContentBlockDelta { delta } => match delta {
                Delta::Text { text } => vec![InternalEvent::TextDelta(text)],
                Delta::Unknown => vec![],
            },
            _ => vec![],
        },
        StreamLine::ResultLine { usage } => vec![InternalEvent::Result {
            input_tokens: usage.as_ref().and_then(|u| u.input_tokens).unwrap_or(0),
            output_tokens: usage.as_ref().and_then(|u| u.output_tokens).unwrap_or(0),
        }],
        StreamLine::ControlRequest {
            request_id,
            request,
        } => match request {
            ControlRequestBody::CanUseTool { .. } => {
                vec![InternalEvent::PermissionRequest { request_id }]
            }
            _ => vec![],
        },
        StreamLine::Assistant { message } => {
            // In --print mode, text comes via Assistant snapshots (no stream_event deltas).
            // In SDK stream mode, text comes via stream_event content_block_delta AND Assistant.
            // We emit Assistant text here; duplicates are filtered in the consumer.
            let mut text = String::new();
            if let Some(content) = message.get("content").and_then(|c| c.as_array()) {
                for b in content {
                    if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                        if let Some(t) = b.get("text").and_then(|t| t.as_str()) {
                            if !t.is_empty() {
                                text.push_str(t);
                            }
                        }
                    }
                }
            }
            if text.is_empty() {
                vec![]
            } else {
                vec![InternalEvent::AssistantText(text)]
            }
        }
        StreamLine::System {
            mcp_servers, tools, ..
        } => {
            tracing::info!(
                "[claude-code] System: mcp={:?} tools={:?}",
                mcp_servers.as_ref().map(|v| v.to_string()),
                tools.as_ref().map(|v| v.to_string())
            );
            vec![]
        }
        StreamLine::Unknown => vec![],
    }
}
