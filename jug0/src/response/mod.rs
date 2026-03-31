// src/response/mod.rs
//
// Unified response types for all API endpoints

pub mod admin;
pub mod agents;
pub mod api_keys;
pub mod auth;
pub mod chats;
pub mod common;
pub mod embeddings;
pub mod models;
pub mod organizations;
pub mod prompts;
pub mod resources;
pub mod usage;
pub mod users;
pub mod workflows;

// Re-export common types at module root for convenience
pub use agents::{AgentDetailResponse, AgentWithOwner};
pub use api_keys::CreateApiKeyResponse;
pub use auth::{AuthResponse, MeResponse, UserDto};
pub use chats::{BranchResponse, ChatSyncResponse, ContextResponse, MessageResponse, StreamEvent};
pub use common::{OwnerInfo, PublicUserProfile, SuccessResponse};
pub use embeddings::EmbeddingResponse;
pub use models::ModelsResponse;
pub use organizations::{OrgInfoResponse, SetPublicKeyResponse};
pub use prompts::{PromptWithOwner, RenderPromptResponse};
pub use resources::{ResourceAgent, ResourcePrompt, ResourceResponse, ResourceWorkflow};
pub use usage::{ModelUsage, UsageStats};
pub use users::{BatchSyncResponse, SyncUserResponse};
pub use workflows::{ExecuteWorkflowResponse, WorkflowRunResponse, WorkflowWithOwner};
