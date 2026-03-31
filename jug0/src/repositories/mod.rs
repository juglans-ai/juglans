// src/repositories/mod.rs
//
// Repository pattern for data access with cache + DB fallback

pub mod agents;
pub mod prompts;

pub use agents::AgentRepository;
pub use prompts::PromptRepository;
