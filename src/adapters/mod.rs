// src/adapters/mod.rs
#![cfg(not(target_arch = "wasm32"))]

pub mod discord;
pub mod feishu;
pub mod telegram;
pub mod wechat;

use anyhow::{anyhow, Result};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

use crate::core::context::{WorkflowContext, WorkflowEvent};
use crate::core::executor::WorkflowExecutor;
use crate::core::parser::GraphParser;
use crate::services::config::JuglansConfig;
use crate::services::local_runtime::LocalRuntime;
use crate::services::prompt_loader::PromptRegistry;

/// Standardized platform event envelope.
///
/// All platform events (messages, card callbacks, etc.) use a uniform format; workflow routes via $input.event_type.
pub struct PlatformMessage {
    /// Event type: "message" | "card_action" | ...
    pub event_type: String,
    /// Event-specific data (message: {"text": "..."}, card_action: {"action": "confirm", ...})
    pub event_data: Value,
    pub platform_user_id: String,
    pub platform_chat_id: String,
    /// Convenience field: message text (populated for message events, empty for others)
    pub text: String,
    pub username: Option<String>,
    /// Platform identifier: "telegram" | "feishu" | "wechat" | "web"
    pub platform: String,
}

/// Bot reply
pub struct BotReply {
    pub text: String,
}

/// Tool executor trait -- implemented by platform adapters
#[async_trait::async_trait]
pub trait ToolExecutor: Send + Sync {
    async fn execute(&self, tool_name: &str, args: Value) -> Result<String>;
}

/// Pluggable message dispatch. Default behavior is the in-process workflow
/// runtime (see [`LocalDispatcher`]); orchestrator-style hosts (e.g.
/// juglans-wallet) wrap their own dispatcher so a long-running adapter loop
/// can route incoming messages to whichever agent the orchestrator picks.
///
/// Two entry points coexist:
/// - `dispatch(message)` — legacy, "concatenate Token events into BotReply.text"
///   semantics. Kept for callers that don't have a Channel (orchestrator hosts,
///   tests). Channels should not call this directly.
/// - `dispatch_with_origin(message, origin)` — new entry. When `origin` is
///   `Some`, the implementation sets `WorkflowContext::origin` and routes
///   replies through `origin.channel.send(...)` per speech node, returning an
///   empty `BotReply` (channels see empty text and skip the legacy re-send).
///   The default impl delegates to `dispatch`, ignoring origin — old impls
///   keep working but lose the per-node routing benefit.
#[async_trait::async_trait]
pub trait MessageDispatcher: Send + Sync {
    async fn dispatch(&self, message: &PlatformMessage) -> Result<BotReply>;

    async fn dispatch_with_origin(
        &self,
        message: &PlatformMessage,
        _origin: Option<crate::core::context::ChannelOrigin>,
    ) -> Result<BotReply> {
        self.dispatch(message).await
    }
}

/// A named platform endpoint with optional ingress and egress.
///
/// Channels uniformly model every chat-platform integration: WeChat accounts,
/// Telegram bots (polling or webhook), Discord gateways, Feishu event apps,
/// Feishu incoming webhooks. Each channel implements whichever direction(s) it
/// supports — pure-egress channels (e.g. Feishu incoming webhook) leave both
/// ingress methods at their default no-op.
///
/// **Active ingress** — `run()`. Spawn a long-lived loop (long-poll, websocket).
/// **Passive ingress** — `install_routes()`. Mount HTTP routes that the platform POSTs to.
/// **Egress** — `send()` (inherited from `ChannelEgress`). Push a reply to a conversation.
///
/// `Channel` extends `ChannelEgress` (defined in `core::context`) so a
/// `WorkflowContext` can carry an `Arc<dyn ChannelEgress>` without `core`
/// depending on `adapters`. `Arc<dyn Channel>` coerces to `Arc<dyn ChannelEgress>`
/// automatically.
///
/// A single juglans process can run many channels concurrently; the orchestrator
/// spawns each `run()` and merges each `install_routes()` into the shared axum
/// router before serving.
#[async_trait::async_trait]
pub trait Channel: crate::core::context::ChannelEgress {
    /// Stable identifier, e.g. `"wechat:wxid_alpha"` or `"telegram:main"`.
    fn id(&self) -> &str;

    /// Platform family, e.g. `"wechat"`, `"telegram"`, `"discord"`, `"feishu"`.
    fn kind(&self) -> &str;

    /// Active ingress loop. Default: no-op (passive-only or egress-only channels
    /// leave this alone). Implementations should tolerate transient errors
    /// internally and only return `Err` for unrecoverable conditions.
    async fn run(self: Arc<Self>, _dispatcher: Arc<dyn MessageDispatcher>) -> Result<()> {
        Ok(())
    }

    /// Passive ingress route registration. Default: returns the router untouched
    /// (active or egress-only channels leave this alone). Called once at server
    /// boot, before `axum::serve`. The same `dispatcher` the channel will hand
    /// to its workflow runs is passed in so handlers can capture it.
    fn install_routes(
        self: Arc<Self>,
        router: axum::Router,
        _dispatcher: Arc<dyn MessageDispatcher>,
    ) -> axum::Router {
        router
    }
}

// Note: the egress `send` method lives on `ChannelEgress` in `core::context`,
// not on `Channel` directly. Each channel impl provides an
// `impl ChannelEgress for Foo { async fn send(...) {...} }` block alongside
// its `impl Channel for Foo { ... }` block.

/// Wraps another `MessageDispatcher` to auto-inject a `ChannelOrigin` derived
/// from the inbound message's `platform_chat_id`. Channel ingress code wraps
/// its supplied dispatcher with this, so the underlying message loop doesn't
/// need to know about origin — it just calls `dispatch` like before, and
/// origin gets plumbed transparently.
pub struct OriginAwareDispatcher {
    inner: Arc<dyn MessageDispatcher>,
    channel: Arc<dyn crate::core::context::ChannelEgress>,
}

impl OriginAwareDispatcher {
    pub fn new(
        channel: Arc<dyn crate::core::context::ChannelEgress>,
        inner: Arc<dyn MessageDispatcher>,
    ) -> Self {
        Self { inner, channel }
    }
}

#[async_trait::async_trait]
impl MessageDispatcher for OriginAwareDispatcher {
    async fn dispatch(&self, message: &PlatformMessage) -> Result<BotReply> {
        let origin = crate::core::context::ChannelOrigin {
            channel: self.channel.clone(),
            conversation: message.platform_chat_id.clone(),
        };
        self.inner
            .dispatch_with_origin(message, Some(origin))
            .await
    }

    async fn dispatch_with_origin(
        &self,
        message: &PlatformMessage,
        origin: Option<crate::core::context::ChannelOrigin>,
    ) -> Result<BotReply> {
        // If caller supplied an origin we honor that; otherwise synthesize
        // one from this wrapper's channel + the message's chat_id.
        let origin = origin.or_else(|| {
            Some(crate::core::context::ChannelOrigin {
                channel: self.channel.clone(),
                conversation: message.platform_chat_id.clone(),
            })
        });
        self.inner.dispatch_with_origin(message, origin).await
    }
}

/// Default dispatcher: load the workflow file from disk and run it in-process.
/// CLI-mode adapters use this; juglans-wallet supplies its own.
pub struct LocalDispatcher {
    pub config: JuglansConfig,
    pub project_root: std::path::PathBuf,
    pub agent_slug: String,
}

#[async_trait::async_trait]
impl MessageDispatcher for LocalDispatcher {
    async fn dispatch(&self, message: &PlatformMessage) -> Result<BotReply> {
        run_agent_for_message(
            &self.config,
            &self.project_root,
            &self.agent_slug,
            message,
            None,
            None,
        )
        .await
    }

    async fn dispatch_with_origin(
        &self,
        message: &PlatformMessage,
        origin: Option<crate::core::context::ChannelOrigin>,
    ) -> Result<BotReply> {
        run_agent_for_message(
            &self.config,
            &self.project_root,
            &self.agent_slug,
            message,
            None,
            origin,
        )
        .await
    }
}

/// Reuse core logic from web_server handle_chat, without the SSE/HTTP parts:
/// 1. Load agent -> create executor
/// 2. Create WorkflowContext, set $input.message
/// 3. Execute workflow or direct chat
/// 4. Collect all Token events -> concatenate into reply text
pub async fn run_agent_for_message(
    config: &JuglansConfig,
    project_root: &Path,
    agent_slug: &str,
    message: &PlatformMessage,
    tool_executor: Option<&dyn ToolExecutor>,
    origin: Option<crate::core::context::ChannelOrigin>,
) -> Result<BotReply> {
    // 1. Find workflow file by slug (agent_slug is now a workflow name)
    let wf_path = {
        let jg_pattern = project_root
            .join(format!("**/{}.jg", agent_slug))
            .to_string_lossy()
            .to_string();
        glob::glob(&jg_pattern)
            .ok()
            .and_then(|mut paths| paths.find_map(|p| p.ok()))
            .ok_or_else(|| anyhow!("Workflow '{}' not found in workspace", agent_slug))?
    };

    let wf_content = fs::read_to_string(&wf_path)
        .map_err(|e| anyhow!("Workflow File Error: {} (tried {:?})", e, wf_path))?;

    // 2. Create runtime + executor
    let runtime: Arc<LocalRuntime> = Arc::new(LocalRuntime::new_with_config(&config.ai));

    let mut prompt_registry = PromptRegistry::new();
    let _ = prompt_registry.load_from_paths(&[
        project_root.join("**/*.jgx").to_string_lossy().to_string(),
        project_root
            .join("**/*.jgprompt")
            .to_string_lossy()
            .to_string(),
    ]);

    let mut executor =
        WorkflowExecutor::new_with_debug(Arc::new(prompt_registry), runtime, config.debug.clone())
            .await;

    // Load tool definitions
    {
        use crate::core::tool_loader::ToolLoader;
        use crate::services::tool_registry::ToolRegistry;
        let tool_pattern = project_root.join("**/*.json").to_string_lossy().to_string();
        if let Ok(tools) = ToolLoader::load_from_glob(&tool_pattern, project_root) {
            if !tools.is_empty() {
                let mut registry = ToolRegistry::new();
                registry.register_all(tools);
                executor.set_tool_registry(Arc::new(registry));
            }
        }
    }

    // Parse workflow + expand decorators
    let mut wf_graph = GraphParser::parse(&wf_content)?;
    crate::core::macro_expand::expand_decorators(&mut wf_graph)?;
    let parsed_workflow = Some(Arc::new(wf_graph));

    // Initialize Python runtime + load workflow tools (requires &mut self, must be before Arc)
    if let Some(ref wf) = parsed_workflow {
        executor.load_tools(wf).await;
        executor.apply_limits(&config.limits);
        if let Err(e) = executor.init_python_runtime(wf, config.limits.python_workers) {
            warn!("Failed to initialize Python runtime: {}", e);
        }
    }

    let executor = Arc::new(executor);
    executor
        .get_registry()
        .set_executor(Arc::downgrade(&executor));

    // Initialize the global conversation-history store from config (idempotent).
    if let Err(e) = crate::services::history::init_global(&config.history) {
        warn!("[history] init_global failed: {}", e);
    }

    // 3. Create context + event channel (for collecting tokens)
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<WorkflowEvent>();
    let ctx = WorkflowContext::with_sender(tx.clone());

    // If a channel origin is set, hand it to the context so reply()/chat()
    // can find their way back, and turn on per-node events so the egress
    // driver can react to NodeComplete (only chat/reply nodes are sent).
    if let Some(ref org) = origin {
        ctx.set_origin(org.clone());
        ctx.set_stream_node_events(true);
    }

    // Derive a namespaced chat_id for history storage. Keeps different
    // platforms / users / agents on separate threads so a telegram chat
    // and a wechat chat for the same slug never collide.
    let derived_chat_id = format!(
        "{}:{}:{}",
        message.platform, message.platform_chat_id, agent_slug
    );

    // Set standardized event input
    ctx.set("input.platform".into(), json!(message.platform))
        .ok();
    ctx.set("input.event_type".into(), json!(message.event_type))
        .ok();
    ctx.set("input.event_data".into(), message.event_data.clone())
        .ok();
    ctx.set("input.user_id".into(), json!(message.platform_user_id))
        .ok();
    ctx.set("input.chat_id".into(), json!(derived_chat_id)).ok();
    ctx.set("input.text".into(), json!(message.text)).ok();
    ctx.set("input.message".into(), json!(message.text)).ok(); // backward compat
    ctx.set(
        "input.platform_chat_id".into(),
        json!(message.platform_chat_id),
    )
    .ok();
    ctx.set(
        "input.platform_user_id".into(),
        json!(message.platform_user_id),
    )
    .ok();
    if let Some(ref username) = message.username {
        ctx.set("input.username".into(), json!(username)).ok();
    }

    // Inject juglans.toml config into $config
    if let Ok(config_value) = serde_json::to_value(config) {
        ctx.set("config".to_string(), config_value).ok();
    }

    // Try to parse message as JSON
    if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&message.text) {
        if let Some(obj) = parsed.as_object() {
            for (k, v) in obj {
                ctx.set(format!("input.{}", k), v.clone()).ok();
            }
        }
    }

    // 4. Execute workflow asynchronously
    let executor_clone = executor.clone();
    let agent_slug_owned = agent_slug.to_string();

    let exec_handle = tokio::spawn(async move {
        let result = if let Some(workflow) = parsed_workflow {
            executor_clone.execute_graph(workflow, &ctx).await
        } else {
            // Direct chat mode (remote agent slug)
            let mut params = HashMap::new();
            params.insert("agent".to_string(), agent_slug_owned);
            params.insert("message".to_string(), "$input.message".to_string());

            executor_clone
                .execute_tool_internal("chat", &params, &ctx)
                .await
                .map(|_| ())
        };

        if let Err(e) = result {
            error!("Bot execution error: {}", e);
            let _ = tx.send(WorkflowEvent::Error(e.to_string()));
        }
    });

    // 5. Drain events.
    //
    // Two paths:
    // - With `origin`: speech-node lifecycle drives channel egress. Per-node
    //   visibility (state=visible/display) and streaming (stream=true AND
    //   channel supports it) is decided at NodeStart. Tokens push live for
    //   streaming nodes; finalize fires on NodeComplete. Non-streaming
    //   visible nodes batch on NodeComplete via `send`. Hidden/silent nodes
    //   skip channel egress entirely.
    // - Without `origin` (back-compat orchestrator hosts): collect all Token
    //   events into one BotReply.text and return.
    let mut reply_text = String::new();
    let conversation = origin.as_ref().map(|o| o.conversation.clone());
    let egress_channel = origin.as_ref().map(|o| o.channel.clone());

    // Per-node egress state. Populated on NodeStart for chat/reply when
    // origin is set; cleared on NodeComplete. `visible` controls whether
    // we send at all; `stream_handle` is Some when streaming was negotiated.
    use crate::core::context::StreamHandle;
    struct NodeEgress {
        visible: bool,
        stream_handle: Option<Box<dyn StreamHandle>>,
    }
    let mut node_egress: HashMap<String, NodeEgress> = HashMap::new();
    // The single currently-streaming node id, if any. Token events route
    // here. Assumes serial chat/reply (the >99% common case); parallel
    // chat across nodes will fall back to batch on the second one.
    let mut streaming_node_id: Option<String> = None;

    fn parse_visibility(params: &HashMap<String, String>) -> bool {
        let raw = params.get("state").map(|s| s.as_str()).unwrap_or("context_visible");
        let output_state = match raw.split_once(':') {
            Some((_, o)) => o,
            None => raw,
        };
        matches!(output_state, "context_visible" | "display_only")
    }
    fn parse_stream_flag(params: &HashMap<String, String>) -> bool {
        params
            .get("stream")
            .map(|s| s.as_str())
            .map(|s| s != "false" && s != "0" && s != "no")
            .unwrap_or(true)
    }

    while let Some(event) = rx.recv().await {
        match event {
            WorkflowEvent::Token(t) => {
                if egress_channel.is_none() {
                    // Legacy path: tokens accumulate into BotReply.text.
                    reply_text.push_str(&t);
                } else if let Some(node_id) = &streaming_node_id {
                    if let Some(state) = node_egress.get_mut(node_id) {
                        if let Some(handle) = state.stream_handle.as_mut() {
                            if let Err(e) = handle.push_token(&t).await {
                                warn!(
                                    "[channel egress] stream push failed on [{}]: {:#}",
                                    node_id, e
                                );
                            }
                        }
                    }
                }
            }
            WorkflowEvent::Status(s) => {
                debug!("[Bot Status] {}", s);
            }
            WorkflowEvent::Error(e) => {
                if egress_channel.is_none() && reply_text.is_empty() {
                    reply_text = format!("Error: {}", e);
                }
                // With origin: workflow errors don't auto-reply to channel.
                // Workflow author can `reply(text="error: ...")` if they want
                // the user to see something.
            }
            WorkflowEvent::ToolCall {
                tools, result_tx, ..
            } => {
                if let Some(executor) = tool_executor {
                    let mut results = vec![];
                    for tool in &tools {
                        let tool_name = tool["name"]
                            .as_str()
                            .or_else(|| tool.pointer("/function/name").and_then(|v| v.as_str()))
                            .unwrap_or("unknown");
                        let args_str = tool["arguments"]
                            .as_str()
                            .or_else(|| {
                                tool.pointer("/function/arguments").and_then(|v| v.as_str())
                            })
                            .unwrap_or("{}");
                        let tool_call_id = tool["id"].as_str().unwrap_or("").to_string();
                        let args: Value = serde_json::from_str(args_str).unwrap_or(json!({}));

                        info!("🔧 [Bot] Executing tool: {}({})", tool_name, args_str);
                        let content = match executor.execute(tool_name, args).await {
                            Ok(result) => result,
                            Err(e) => {
                                error!("🔧 [Bot] Tool {} failed: {}", tool_name, e);
                                format!("Error: {}", e)
                            }
                        };
                        results.push(crate::core::context::ToolResultPayload {
                            tool_call_id,
                            content,
                        });
                    }
                    let _ = result_tx.send((results, None));
                } else {
                    warn!("[Bot] Client tool call received but no executor available, skipping");
                    let _ = result_tx.send((vec![], None));
                }
            }
            WorkflowEvent::NodeStart(evt) => {
                // Speech-node setup: only chat/reply matter for egress.
                if let (Some(channel), Some(conv)) = (&egress_channel, &conversation) {
                    if matches!(evt.tool.as_str(), "chat" | "reply") {
                        let visible = parse_visibility(&evt.params);
                        let want_stream = visible && parse_stream_flag(&evt.params);
                        let mut handle: Option<Box<dyn StreamHandle>> = None;
                        if want_stream && streaming_node_id.is_none() {
                            // Try to negotiate a streaming session. Failure
                            // is fine — we'll batch on NodeComplete.
                            match channel.start_stream(conv).await {
                                Ok(h) => {
                                    handle = Some(h);
                                    streaming_node_id = Some(evt.node_id.clone());
                                }
                                Err(_) => {
                                    // Channel doesn't support streaming, or
                                    // a transient failure. Either way: batch.
                                }
                            }
                        }
                        node_egress.insert(
                            evt.node_id.clone(),
                            NodeEgress {
                                visible,
                                stream_handle: handle,
                            },
                        );
                    }
                }
            }
            WorkflowEvent::NodeComplete(evt) => {
                if let (Some(channel), Some(conv)) = (&egress_channel, &conversation) {
                    if let Some(state) = node_egress.remove(&evt.node_id) {
                        // Visible? Either finalize stream or send batch.
                        // Hidden/silent? Skip — workflow author chose silence.
                        if state.visible {
                            if let Some(handle) = state.stream_handle {
                                if Some(&evt.node_id) == streaming_node_id.as_ref() {
                                    streaming_node_id = None;
                                }
                                if let Err(e) = handle.finalize().await {
                                    warn!(
                                        "[channel egress] stream finalize failed on [{}]: {:#}",
                                        evt.node_id, e
                                    );
                                }
                            } else if let Some(text) = extract_speech_text(&evt.result) {
                                if !text.is_empty() {
                                    if let Err(e) = channel.send(conv, &text).await {
                                        error!(
                                            "[channel egress] {} send failed for [{}]: {:#}",
                                            evt.tool, evt.node_id, e
                                        );
                                    }
                                }
                            }
                        } else {
                            // Hidden node had a stream handle? Defensive
                            // finalize so resources don't leak.
                            if let Some(handle) = state.stream_handle {
                                let _ = handle.finalize().await;
                            }
                        }
                    }
                }
            }
            WorkflowEvent::Meta(_)
            | WorkflowEvent::Yield(_)
            | WorkflowEvent::ToolStart(_)
            | WorkflowEvent::ToolComplete(_) => {
                // Bot mode ignores these — interactive UIs use them for
                // status display but channels just want the final text.
            }
        }
    }

    // Wait for execution to finish
    let _ = exec_handle.await;

    if egress_channel.is_some() {
        // Origin-routed reply: nothing to return — channel.send / start_stream
        // already delivered per node. Empty text signals "no need to re-send"
        // to the caller's legacy guard.
        return Ok(BotReply { text: String::new() });
    }

    // No-origin path: callers (juglans-wallet–style external orchestrators)
    // expect the concatenated Token text. Fall back to "(No response)" so the
    // calling channel sends *something* on a workflow that never spoke.
    //
    // (An earlier version tried to read `reply.output` from a freshly-built
    // empty `WorkflowContext::new()` here as a last resort — that always
    // returned None, so it was dead code. Removed.)
    if reply_text.is_empty() {
        reply_text = "(No response)".to_string();
    }

    Ok(BotReply { text: reply_text })
}

/// Pull a string reply out of a chat/reply NodeComplete result.
///
/// `chat()` returns `{ content: "...", ... }` (sometimes with `tokens`/`role`
/// fields); `reply()` returns `{ content: "...", status: "sent" }`. Either way,
/// the visible message is the `content` field. Falls back to direct string
/// values for forward-compat with future builtins that just return text.
fn extract_speech_text(result: &Option<Value>) -> Option<String> {
    let v = result.as_ref()?;
    if let Some(s) = v.as_str() {
        return Some(s.to_string());
    }
    if let Some(s) = v.get("content").and_then(|c| c.as_str()) {
        return Some(s.to_string());
    }
    None
}
