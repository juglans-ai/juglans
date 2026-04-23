// src/providers/llm/factory.rs
use super::{
    anthropic::AnthropicProvider, byteplus::BytePlusProvider, chatgpt::ChatGPTProvider,
    deepseek::DeepSeekProvider, gemini::GeminiProvider, juglans::JuglansProvider,
    qwen::QwenProvider, xai::XaiProvider, LlmProvider,
};
use dashmap::DashMap;
use std::collections::HashMap;
use std::env;
use std::sync::Arc;

use super::claude_code::ClaudeCodeProvider;
use super::mcp_types::McpSession;

/// Per-provider configuration (api_key, base_url).
#[derive(Debug, Clone, Default)]
pub struct LlmProviderConfig {
    pub api_key: Option<String>,
    pub base_url: Option<String>,
}

#[derive(Clone)]
pub struct ProviderFactory {
    anthropic: Arc<AnthropicProvider>,
    byteplus: Arc<BytePlusProvider>,
    chatgpt: Arc<ChatGPTProvider>,
    deepseek: Arc<DeepSeekProvider>,
    gemini: Arc<GeminiProvider>,
    juglans: Arc<JuglansProvider>,
    qwen: Arc<QwenProvider>,
    xai: Arc<XaiProvider>,
    /// Extra providers registered at runtime (e.g. claude_code in server mode)
    extra: Arc<DashMap<String, Arc<dyn LlmProvider>>>,
}

impl ProviderFactory {
    /// Create a new ProviderFactory with all built-in providers (no server deps).
    pub fn new() -> Self {
        Self {
            anthropic: Arc::new(AnthropicProvider::new()),
            byteplus: Arc::new(BytePlusProvider::new()),
            chatgpt: Arc::new(ChatGPTProvider::new()),
            deepseek: Arc::new(DeepSeekProvider::new()),
            gemini: Arc::new(GeminiProvider::new()),
            juglans: Arc::new(JuglansProvider::new(&LlmProviderConfig::default())),
            qwen: Arc::new(QwenProvider::new()),
            xai: Arc::new(XaiProvider::new()),
            extra: Arc::new(DashMap::new()),
        }
    }

    /// Create a ProviderFactory with configuration from juglans.toml [ai.providers].
    ///
    /// For historical reasons most providers are still env-driven: we shove
    /// `[ai.providers.<name>]` values into `std::env` here so the per-provider
    /// `::new()` constructors (which read env) pick them up. That env magic is
    /// a wart — difficult to test, leaks across processes, etc. — and will be
    /// migrated to the explicit-config pattern one provider at a time.
    ///
    /// `juglans` is the first to opt out: its constructor takes
    /// `&LlmProviderConfig` directly, so its entry is built explicitly below
    /// rather than through `apply(...)`.
    pub fn new_with_config(configs: &HashMap<String, LlmProviderConfig>) -> Self {
        let apply = |name: &str, key_env: &str, url_env: Option<&str>| {
            if let Some(cfg) = configs.get(name) {
                if let Some(key) = &cfg.api_key {
                    if !key.is_empty() {
                        std::env::set_var(key_env, key);
                    }
                }
                if let Some(url) = &cfg.base_url {
                    if let Some(url_env) = url_env {
                        if !url.is_empty() {
                            std::env::set_var(url_env, url);
                        }
                    }
                }
            }
        };

        apply("openai", "OPENAI_API_KEY", Some("OPENAI_API_BASE"));
        apply("anthropic", "ANTHROPIC_API_KEY", Some("ANTHROPIC_BASE_URL"));
        apply("deepseek", "DEEPSEEK_API_KEY", None);
        apply("gemini", "GEMINI_API_KEY", None);
        apply("qwen", "QWEN_API_KEY", None);
        apply("byteplus", "ARK_API_KEY", Some("ARK_API_BASE"));
        apply("xai", "XAI_API_KEY", None);

        let juglans_cfg = configs.get("juglans").cloned().unwrap_or_default();

        let mut s = Self::new();
        s.juglans = Arc::new(JuglansProvider::new(&juglans_cfg));
        s
    }

    /// Create factory with claude_code provider and MCP tool sessions.
    pub fn new_with_mcp(tool_sessions: Arc<DashMap<String, McpSession>>) -> Self {
        let mut factory = Self::new();
        let mut cc = ClaudeCodeProvider::new();
        cc.set_tool_sessions(tool_sessions);
        factory
            .extra
            .insert("claude-code".to_string(), Arc::new(cc));
        factory
    }

    /// Register an additional provider at runtime.
    pub fn register_provider(&self, name: &str, provider: Arc<dyn LlmProvider>) {
        self.extra.insert(name.to_string(), provider);
    }

    /// Returns (provider, actual_model_name).
    /// Supports `provider/model` format (e.g. `byteplus/deepseek-v3`).
    /// Without prefix, falls back to substring matching.
    pub fn get_provider(&self, model: &str) -> (Arc<dyn LlmProvider>, String) {
        // Explicit provider/model format
        if let Some((provider_name, actual_model)) = model.split_once('/') {
            let pn = provider_name.to_lowercase();
            // Check extra providers first (e.g. claude-code)
            if let Some(p) = self.extra.get(&pn) {
                return (p.value().clone(), actual_model.to_string());
            }
            let p: Arc<dyn LlmProvider> = match pn.as_str() {
                "openai" | "chatgpt" => self.chatgpt.clone(),
                "anthropic" | "claude" => self.anthropic.clone(),
                "deepseek" => self.deepseek.clone(),
                "qwen" => self.qwen.clone(),
                "gemini" => self.gemini.clone(),
                "byteplus" | "ark" => self.byteplus.clone(),
                "xai" => self.xai.clone(),
                "juglans" => self.juglans.clone(),
                _ => self.chatgpt.clone(),
            };
            return (p, actual_model.to_string());
        }

        // Legacy: substring matching on model name
        let m = model.to_lowercase();
        let default_provider = env::var("DEFAULT_LLM_PROVIDER")
            .unwrap_or_default()
            .to_lowercase();

        // Check extra providers (e.g. claude-code)
        if m.contains("claude-code") || (m == "default" && default_provider == "claude-code") {
            if let Some(p) = self.extra.get("claude-code") {
                return (p.value().clone(), model.to_string());
            }
        }

        if m.contains("claude") || (m == "default" && default_provider == "anthropic") {
            return (self.anthropic.clone(), model.to_string());
        }

        if m.contains("qwen") || (m == "default" && default_provider == "qwen") {
            return (self.qwen.clone(), model.to_string());
        }

        if m.contains("gemini") {
            return (self.gemini.clone(), model.to_string());
        }

        if m.contains("deepseek") {
            return (self.deepseek.clone(), model.to_string());
        }

        if m.contains("grok") {
            return (self.xai.clone(), model.to_string());
        }

        if m.contains("doubao")
            || m.starts_with("ep-")
            || (m == "default" && default_provider == "byteplus")
        {
            return (self.byteplus.clone(), model.to_string());
        }

        (self.chatgpt.clone(), model.to_string())
    }
}
