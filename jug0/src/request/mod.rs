// src/request/mod.rs
//
// Unified request types for all API endpoints

pub mod agents;
pub mod api_keys;
pub mod auth;
pub mod chats;
pub mod deploys;
pub mod embeddings;
pub mod memories;
pub mod models;
pub mod organizations;
pub mod prompts;
pub mod users;
pub mod vectors;
pub mod workflows;

// Re-export all types at module root for convenience
pub use agents::{CreateAgentRequest, UpdateAgentRequest};
pub use api_keys::CreateApiKeyRequest;
pub use auth::{LoginRequest, RegisterRequest};
pub use chats::{
    AgentConfig, BranchRequest, ChatIdInput, ChatRequest, ContextQuery, CreateMessageRequest,
    ListChatsQuery, MessagePart, RegenerateRequest, StopRequest, ToolResultPayload,
    ToolResultRequest, UpdateMessageRequest,
};
pub use deploys::{CreateDeployRequest, UpdateDeployRequest};
pub use embeddings::EmbeddingRequest;
pub use memories::{ListMemoryQuery, SearchMemoryRequest};
pub use models::ModelsQuery;
pub use organizations::SetPublicKeyRequest;
pub use prompts::{CreatePromptRequest, PromptFilter, RenderPromptRequest, UpdatePromptRequest};
pub use users::{BatchSyncRequest, SyncUserRequest};
pub use workflows::{CreateWorkflowRequest, ExecuteWorkflowRequest, UpdateWorkflowRequest};
