// src/providers/mod.rs

#[cfg(feature = "server")]
pub mod cache;
pub mod embedding;
pub mod llm;
#[cfg(feature = "server")]
pub mod memory;
pub mod search;
#[cfg(feature = "server")]
pub mod storage;

// Re-export commonly used types
#[cfg(feature = "server")]
pub use cache::CacheProvider;
pub use embedding::{EmbeddingFactory, EmbeddingProvider};
pub use llm::factory::ProviderFactory;
pub use llm::{ChatStreamChunk, LlmProvider, Message, MessagePart, TokenUsage, ToolCallChunk};
#[cfg(feature = "server")]
pub use memory::MemoryProvider;
pub use search::SearchProvider;
#[cfg(feature = "server")]
pub use storage::StorageProvider;
