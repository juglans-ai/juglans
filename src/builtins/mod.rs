// src/builtins/mod.rs
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tracing::debug;

use crate::core::context::WorkflowContext;
use crate::core::tool_loader::ToolResource;
use crate::services::local_runtime::LocalRuntime;
use crate::services::prompt_loader::PromptRegistry;
use crate::services::tool_registry::ToolRegistry;

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;

    /// Return tool schema in OpenAI function calling format
    /// Tools implementing this method can be auto-discovered and invoked by the LLM
    fn schema(&self) -> Option<Value> {
        None
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        context: &WorkflowContext,
    ) -> Result<Option<Value>>;
}

/// Central registry of builtin tools.
///
/// # Circular dependency pattern
///
/// `WorkflowExecutor` owns `Arc<BuiltinRegistry>`, while `BuiltinRegistry` holds
/// a `Weak<WorkflowExecutor>` back-reference for nested workflow execution
/// and tool registry resolution. This bidirectional relationship is required
/// because builtin tools (Chat, ExecuteWorkflow) need executor capabilities at runtime.
///
/// The `Weak` reference breaks the ownership cycle and is set post-construction via
/// `set_executor()`. Similarly, `Chat` and `ExecuteWorkflow` hold `Weak<BuiltinRegistry>`
/// to access these capabilities without preventing deallocation.
///
/// **Future improvement**: Extract the executor capabilities (`get_tool_registry`,
/// `execute_graph`) into a trait. The blocker is that `execute_graph`
/// uses `self: Arc<Self>` for `tokio::spawn`, which isn't compatible with `#[async_trait]`
/// trait objects. A redesign of the parallel execution model would be needed first.
pub struct BuiltinRegistry {
    tools: RwLock<HashMap<String, Arc<Box<dyn Tool>>>>,
    /// Back-reference to WorkflowExecutor for nested execution.
    /// Set post-construction via `set_executor()`. See struct-level docs for rationale.
    executor: RwLock<Option<std::sync::Weak<crate::core::executor::WorkflowExecutor>>>,
    _prompt_registry: Arc<PromptRegistry>,
}

impl BuiltinRegistry {
    pub fn new(prompts: Arc<PromptRegistry>, runtime: Arc<LocalRuntime>) -> Arc<Self> {
        let mut tool_map: HashMap<String, Arc<Box<dyn Tool>>> = HashMap::new();

        macro_rules! reg {
            ($t:expr) => {
                tool_map.insert($t.name().to_string(), Arc::new(Box::new($t)));
            };
        }

        reg!(network::FetchUrl);
        reg!(network::Fetch);
        reg!(http_client::HttpRequest);
        reg!(oauth::OAuthToken);
        reg!(system::Timer);
        reg!(system::Notify);
        reg!(system::Print);
        reg!(system::Reply::new());
        reg!(system::SetContext);
        reg!(system::FeishuWebhook);
        reg!(system::FeishuSend);
        reg!(system::Return);

        // HTTP backend
        // Serve is registered post-construction (needs Weak<BuiltinRegistry>)
        reg!(http::HttpResponse);

        // Devtools (Claude Code style)
        reg!(devtools::ReadFile);
        reg!(devtools::WriteFile);
        reg!(devtools::EditFile);
        reg!(devtools::GlobSearch);
        reg!(devtools::GrepSearch);
        reg!(devtools::Bash);
        // "sh" alias: backward compatible with old sh(cmd=...) syntax
        tool_map.insert("sh".to_string(), Arc::new(Box::new(devtools::Bash)));
        reg!(ai::Prompt::new(prompts.clone()));

        // Testing tools
        reg!(testing::Config);
        // Mock is registered post-construction (needs Weak<BuiltinRegistry>)

        // Device control (requires "device" feature — skipped on headless ARM64)
        #[cfg(feature = "device")]
        {
            reg!(device::KeyTap);
            reg!(device::KeyCombo);
            reg!(device::TypeText);
            reg!(device::MouseMove);
            reg!(device::MouseClick);
            reg!(device::MouseScroll);
            reg!(device::MousePosition);
            reg!(device::MouseDrag);
            reg!(device::ScreenSize);
            reg!(device::Screenshot);
        }

        // Database ORM
        reg!(database::DbConnect);
        reg!(database::DbDisconnect);
        reg!(database::DbQuery);
        reg!(database::DbExec);
        reg!(database::DbFind);
        reg!(database::DbFindOne);
        reg!(database::DbCreate);
        reg!(database::DbCreateMany);
        reg!(database::DbUpsert);
        reg!(database::DbUpdate);
        reg!(database::DbDelete);
        reg!(database::DbCount);
        reg!(database::DbAggregate);
        reg!(database::DbBegin);
        reg!(database::DbCommit);
        reg!(database::DbRollback);
        reg!(database::DbCreateTable);
        reg!(database::DbDropTable);
        reg!(database::DbAlterTable);
        reg!(database::DbTables);
        reg!(database::DbColumns);

        // Conversation history
        reg!(history::HistoryLoad);
        reg!(history::HistoryAppend);
        reg!(history::HistoryReplace);
        reg!(history::HistoryTrim);
        reg!(history::HistoryClear);
        reg!(history::HistoryStats);
        reg!(history::HistoryListChats);

        let registry_arc = Arc::new(Self {
            tools: RwLock::new(tool_map),
            executor: RwLock::new(None),
            _prompt_registry: prompts.clone(),
        });

        let mut chat_tool = ai::Chat::new(prompts, runtime);
        chat_tool.set_registry(Arc::downgrade(&registry_arc));

        let mut exec_wf_tool = ai::ExecuteWorkflow::new();
        exec_wf_tool.set_registry(Arc::downgrade(&registry_arc));

        let mut mock_tool = testing::Mock::new();
        mock_tool.set_registry(Arc::downgrade(&registry_arc));

        let mut call_tool = system::Call::new();
        call_tool.set_registry(Arc::downgrade(&registry_arc));

        let mut serve_tool = http::Serve::new();
        serve_tool.set_registry(Arc::downgrade(&registry_arc));

        #[cfg(feature = "device")]
        let key_listen_tool = {
            let mut t = device::KeyListen::new();
            t.set_registry(Arc::downgrade(&registry_arc));
            t
        };

        #[cfg(feature = "device")]
        let mouse_listen_tool = {
            let mut t = device::MouseListen::new();
            t.set_registry(Arc::downgrade(&registry_arc));
            t
        };

        {
            let mut guard = registry_arc.tools.write().expect("Lock poisoned");
            guard.insert("chat".to_string(), Arc::new(Box::new(chat_tool)));
            guard.insert(
                "execute_workflow".to_string(),
                Arc::new(Box::new(exec_wf_tool)),
            );
            guard.insert("mock".to_string(), Arc::new(Box::new(mock_tool)));
            guard.insert("call".to_string(), Arc::new(Box::new(call_tool)));
            guard.insert("serve".to_string(), Arc::new(Box::new(serve_tool)));
            #[cfg(feature = "device")]
            {
                guard.insert(
                    "key_listen".to_string(),
                    Arc::new(Box::new(key_listen_tool)),
                );
                guard.insert(
                    "mouse_listen".to_string(),
                    Arc::new(Box::new(mouse_listen_tool)),
                );
            }
        }

        registry_arc
    }

    pub fn get(&self, name: &str) -> Option<Arc<Box<dyn Tool>>> {
        self.tools.read().ok()?.get(name).cloned()
    }

    /// Collect OpenAI-format schemas from all builtin tools that implement schema()
    pub fn list_schemas(&self) -> Vec<Value> {
        let guard = self.tools.read().unwrap();
        guard.values().filter_map(|tool| tool.schema()).collect()
    }

    /// Register builtin devtools schemas into ToolRegistry
    /// Makes devtools discoverable via "devtools" slug through resolve_tools()
    pub fn register_devtools_to_registry(&self, tool_registry: &mut ToolRegistry) {
        let schemas = self.list_schemas();
        if !schemas.is_empty() {
            let resource = ToolResource {
                slug: "devtools".to_string(),
                name: "Built-in Developer Tools".to_string(),
                description: Some("Read, Write, Edit, Glob, Grep, Bash".to_string()),
                tools: schemas,
            };
            tool_registry.register(resource);
        }
    }

    /// Inject WorkflowExecutor reference (avoids circular dependency)
    pub fn set_executor(&self, executor: std::sync::Weak<crate::core::executor::WorkflowExecutor>) {
        if let Ok(mut guard) = self.executor.write() {
            *guard = Some(executor);
        }
    }

    /// Get WorkflowExecutor reference (for accessing ToolRegistry)
    pub fn get_executor(&self) -> Option<Arc<crate::core::executor::WorkflowExecutor>> {
        self.executor.read().ok()?.as_ref()?.upgrade()
    }

    /// Execute nested workflow (called by Chat tool)
    pub async fn execute_nested_workflow(
        &self,
        workflow_path: &str,
        base_dir: &std::path::Path,
        context: &crate::core::context::WorkflowContext,
        identifier: String,
    ) -> Result<Value> {
        use crate::core::parser::GraphParser;
        use std::fs;
        use std::sync::Arc;

        // 1. Check recursion (enter execution stack)
        context.enter_execution(identifier.clone())?;

        // 2. Load workflow
        let abs_workflow_path = if std::path::Path::new(workflow_path).is_absolute() {
            std::path::PathBuf::from(workflow_path)
        } else {
            base_dir.join(workflow_path)
        };

        let workflow_content = fs::read_to_string(&abs_workflow_path)
            .with_context(|| format!("Failed to load nested workflow: {:?}", abs_workflow_path))?;

        let workflow_graph = GraphParser::parse(&workflow_content)?;

        // 3. Load prompts and agents required by the workflow
        let _workflow_base_dir = abs_workflow_path
            .parent()
            .unwrap_or(std::path::Path::new("."));

        // Parse and load resources
        if !workflow_graph.prompt_patterns.is_empty() {
            // TODO: Simplified handling; a better resource isolation strategy is needed
            // Consider creating a temporary registry or merging into the current one
        }

        // 4. Get executor and run
        let executor_weak = {
            let guard = self
                .executor
                .read()
                .map_err(|_| anyhow::anyhow!("Failed to acquire executor lock"))?;
            guard
                .clone()
                .ok_or_else(|| anyhow::anyhow!("WorkflowExecutor not initialized"))?
        };

        let executor = executor_weak
            .upgrade()
            .ok_or_else(|| anyhow::anyhow!("WorkflowExecutor has been dropped"))?;

        debug!("│   ├─ Executing nested workflow: {}", workflow_path);

        // Execute workflow
        executor
            .execute_graph(Arc::new(workflow_graph), context)
            .await?;

        // 5. Exit execution stack
        context.exit_execution()?;

        debug!("│   └─ Nested workflow completed");

        // Get output from context
        let output = context
            .resolve_path("reply.output")?
            .unwrap_or(serde_json::json!(""));

        Ok(output)
    }
}

pub mod ai;
pub mod database;
#[cfg(feature = "device")]
pub mod device;
pub mod devtools;
pub mod history;
pub mod http;
pub mod http_client;
pub mod network;
pub mod oauth;
pub mod system;
pub mod testing;
