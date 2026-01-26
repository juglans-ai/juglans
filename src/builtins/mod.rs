// src/builtins/mod.rs
use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::core::context::WorkflowContext;
use crate::services::agent_loader::AgentRegistry;
use crate::services::interface::JuglansRuntime; // 【修改】引用 Trait
use crate::services::prompt_loader::PromptRegistry;

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        context: &WorkflowContext,
    ) -> Result<Option<Value>>;
}

pub struct BuiltinRegistry {
    tools: RwLock<HashMap<String, Arc<Box<dyn Tool>>>>,
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
        reg!(system::Timer);
        reg!(system::Notify);
        reg!(system::SetContext);
        reg!(ai::Prompt::new(prompts.clone(), runtime.clone()));
        reg!(ai::MemorySearch::new(runtime.clone()));

        let registry_arc = Arc::new(Self {
            tools: RwLock::new(tool_map),
        });

        let mut chat_tool = ai::Chat::new(agents, prompts, runtime);
        chat_tool.set_registry(Arc::downgrade(&registry_arc));

        {
            let mut guard = registry_arc.tools.write().expect("Lock poisoned");
            guard.insert("chat".to_string(), Arc::new(Box::new(chat_tool)));
        }

        registry_arc
    }

    pub fn get(&self, name: &str) -> Option<Arc<Box<dyn Tool>>> {
        self.tools.read().ok()?.get(name).cloned()
    }
}

pub mod ai;
pub mod network;
pub mod system;
