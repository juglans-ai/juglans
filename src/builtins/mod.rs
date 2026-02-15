// src/builtins/mod.rs
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use tracing::debug;

use crate::core::context::WorkflowContext;
use crate::core::tool_loader::ToolResource;
use crate::services::agent_loader::AgentRegistry;
use crate::services::interface::JuglansRuntime;
use crate::services::prompt_loader::PromptRegistry;
use crate::services::tool_registry::ToolRegistry;

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;

    /// 返回 OpenAI function calling 格式的 tool schema
    /// 实现此方法的工具可被 LLM 自动发现和调用
    fn schema(&self) -> Option<Value> {
        None
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        context: &WorkflowContext,
    ) -> Result<Option<Value>>;
}

pub struct BuiltinRegistry {
    tools: RwLock<HashMap<String, Arc<Box<dyn Tool>>>>,
    // 用于执行嵌套 workflow（避免循环依赖）
    executor: RwLock<Option<std::sync::Weak<crate::core::executor::WorkflowExecutor>>>,
    prompt_registry: Arc<PromptRegistry>,
    agent_registry: Arc<AgentRegistry>,
}

impl BuiltinRegistry {
    pub fn new(
        prompts: Arc<PromptRegistry>,
        agents: Arc<AgentRegistry>,
        runtime: Arc<dyn JuglansRuntime>, // 【修改】接收 Trait Object
    ) -> Arc<Self> {
        let mut tool_map: HashMap<String, Arc<Box<dyn Tool>>> = HashMap::new();

        macro_rules! reg {
            ($t:expr) => {
                tool_map.insert($t.name().to_string(), Arc::new(Box::new($t)));
            };
        }

        reg!(network::FetchUrl);
        reg!(network::Fetch);
        reg!(system::Timer);
        reg!(system::Notify);
        reg!(system::Reply::new(runtime.clone()));
        reg!(system::SetContext);
        reg!(system::FeishuWebhook);

        // Devtools (Claude Code 风格)
        reg!(devtools::ReadFile);
        reg!(devtools::WriteFile);
        reg!(devtools::EditFile);
        reg!(devtools::GlobSearch);
        reg!(devtools::GrepSearch);
        reg!(devtools::Bash);
        // "sh" 别名：向后兼容旧 sh(cmd=...) 语法
        tool_map.insert("sh".to_string(), Arc::new(Box::new(devtools::Bash)));
        reg!(ai::Prompt::new(prompts.clone(), runtime.clone()));
        reg!(ai::MemorySearch::new(runtime.clone()));
        reg!(ai::History::new(runtime.clone()));

        let registry_arc = Arc::new(Self {
            tools: RwLock::new(tool_map),
            executor: RwLock::new(None),
            prompt_registry: prompts.clone(),
            agent_registry: agents.clone(),
        });

        let mut chat_tool = ai::Chat::new(agents, prompts, runtime);
        chat_tool.set_registry(Arc::downgrade(&registry_arc));

        let mut exec_wf_tool = ai::ExecuteWorkflow::new();
        exec_wf_tool.set_registry(Arc::downgrade(&registry_arc));

        {
            let mut guard = registry_arc.tools.write().expect("Lock poisoned");
            guard.insert("chat".to_string(), Arc::new(Box::new(chat_tool)));
            guard.insert("execute_workflow".to_string(), Arc::new(Box::new(exec_wf_tool)));
        }

        registry_arc
    }

    pub fn get(&self, name: &str) -> Option<Arc<Box<dyn Tool>>> {
        self.tools.read().ok()?.get(name).cloned()
    }

    /// 收集所有实现了 schema() 的 builtin 工具的 OpenAI 格式 schema
    pub fn list_schemas(&self) -> Vec<Value> {
        let guard = self.tools.read().unwrap();
        guard.values().filter_map(|tool| tool.schema()).collect()
    }

    /// 将内置 devtools 的 schema 注册到 ToolRegistry 中
    /// 使 devtools 可通过 "devtools" slug 被 resolve_tools() 发现
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

    /// 注入 WorkflowExecutor 引用（避免循环依赖）
    pub fn set_executor(&self, executor: std::sync::Weak<crate::core::executor::WorkflowExecutor>) {
        if let Ok(mut guard) = self.executor.write() {
            *guard = Some(executor);
        }
    }

    /// 获取 WorkflowExecutor 引用（用于访问 ToolRegistry）
    pub fn get_executor(&self) -> Option<Arc<crate::core::executor::WorkflowExecutor>> {
        self.executor
            .read()
            .ok()?
            .as_ref()?
            .upgrade()
    }

    /// 执行嵌套 workflow（由 Chat tool 调用）
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

        // 1. 检查递归（进入执行栈）
        context.enter_execution(identifier.clone())?;

        // 2. 加载 workflow
        let abs_workflow_path = if std::path::Path::new(workflow_path).is_absolute() {
            std::path::PathBuf::from(workflow_path)
        } else {
            base_dir.join(workflow_path)
        };

        let workflow_content = fs::read_to_string(&abs_workflow_path)
            .with_context(|| format!("Failed to load nested workflow: {:?}", abs_workflow_path))?;

        let workflow_graph = GraphParser::parse(&workflow_content)?;

        // 3. 加载 workflow 依赖的 prompts 和 agents
        let workflow_base_dir = abs_workflow_path.parent().unwrap_or(std::path::Path::new("."));

        // 解析并加载资源
        if !workflow_graph.prompt_patterns.is_empty() || !workflow_graph.agent_patterns.is_empty() {
            use crate::services::prompt_loader::PromptRegistry;
            use crate::services::agent_loader::AgentRegistry;

            // TODO: 这里简化处理，实际应该有更好的资源隔离策略
            // 可以考虑创建临时 registry 或合并到当前 registry
        }

        // 4. 获取 executor 并执行
        let executor_weak = {
            let guard = self.executor.read()
                .map_err(|_| anyhow::anyhow!("Failed to acquire executor lock"))?;
            guard.clone()
                .ok_or_else(|| anyhow::anyhow!("WorkflowExecutor not initialized"))?
        };

        let executor = executor_weak.upgrade()
            .ok_or_else(|| anyhow::anyhow!("WorkflowExecutor has been dropped"))?;

        debug!("│   ├─ Executing nested workflow: {}", workflow_path);

        // 执行 workflow
        executor.execute_graph(Arc::new(workflow_graph), context).await?;

        // 5. 退出执行栈
        context.exit_execution()?;

        debug!("│   └─ Nested workflow completed");

        // 从 context 获取输出
        let output = context
            .resolve_path("reply.output")?
            .unwrap_or(serde_json::json!(""));

        Ok(output)
    }
}

pub mod ai;
pub mod devtools;
pub mod network;
pub mod system;
