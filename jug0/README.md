<h1 align="center">jug0</h1>

<p align="center">
  Open AI Runtime Protocol — multi-tenant chat backend with SSE tool call interruption
</p>

<p align="center">
  <a href="https://github.com/juglans-ai/jug0/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-Apache%202.0-blue.svg" alt="License" /></a>
  <img src="https://img.shields.io/badge/rust-1.80+-orange.svg" alt="Rust" />
</p>

---

jug0 is an open-source AI runtime backend written in Rust. It provides a multi-tenant chat API with streaming SSE, multi-provider LLM support, and a unique **tool call interruption protocol** that pauses generation mid-stream, waits for client-side tool execution, and resumes seamlessly.

## Key Features

- **SSE Tool Call Interruption** — Stream pauses at tool calls, resumes after client returns results. Supports multi-round tool chains.
- **Multi-Provider LLM** — OpenAI, DeepSeek, Gemini, Qwen out of the box. Pluggable `LlmProvider` trait.
- **Message State System** — `context_visible | context_hidden | display_only | silent` for fine-grained control over what the LLM sees vs what the user sees.
- **Vector Memory** — Automatic fact extraction + semantic search via Qdrant. Pluggable `MemoryProvider` trait.
- **Multi-Tenancy** — Organizations, users, API keys, JWT + HMAC chain authentication.
- **MCP Integration** — Model Context Protocol for external tool discovery and execution.
- **Pluggable Architecture** — Provider traits for LLM, Embedding, Memory, Storage, and Cache.

## Architecture

```
Client (SSE)
  │
  ├── POST /api/chat ──────────────► jug0 (Axum)
  │   ◄── event: meta                  │
  │   ◄── event: content               ├── LlmProvider (OpenAI/DeepSeek/Gemini/Qwen)
  │   ◄── event: tool_call  ◄── PAUSE  ├── MemoryProvider (Qdrant)
  │                                     ├── StorageProvider (PostgreSQL)
  ├── POST /api/chat/tool-result ──►    ├── CacheProvider (Redis)
  │   ◄── event: content    ◄── RESUME └── EmbeddingProvider (OpenAI/Qwen)
  │   ◄── event: done
  │
```

## Quick Start

```bash
# Clone
git clone https://github.com/juglans-ai/jug0.git
cd jug0

# Start dependencies
docker compose up -d postgres redis qdrant

# Configure
cp .env.example .env
# Edit .env with your LLM API keys

# Run database migrations
cargo run -p migration -- up

# Start jug0
cargo run --release
```

## Provider Traits

jug0 uses trait-based abstractions so you can swap implementations:

```rust
// LLM Provider — bring your own model
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn stream_chat(
        &self, model: &str, system_prompt: Option<String>,
        history: Vec<Message>, tools: Option<Vec<Value>>,
    ) -> Result<Pin<Box<dyn Stream<Item = Result<ChatStreamChunk>> + Send>>>;
}

// Memory Provider — bring your own vector DB
#[async_trait]
pub trait MemoryProvider: Send + Sync {
    async fn ensure_collection(&self, name: &str, dim: u64) -> Result<()>;
    async fn upsert(&self, collection: &str, points: Vec<MemoryPoint>) -> Result<()>;
    async fn search(&self, collection: &str, vector: Vec<f32>, limit: u64, filter: Option<MemoryFilter>) -> Result<Vec<MemoryResult>>;
    async fn delete(&self, collection: &str, ids: Vec<Uuid>) -> Result<()>;
}

// Cache Provider — bring your own cache
#[async_trait]
pub trait CacheProvider: Send + Sync {
    async fn get_raw(&self, key: &str) -> Option<String>;
    async fn set_raw(&self, key: &str, value: &str, ttl_secs: u64) -> Result<()>;
    async fn del(&self, key: &str) -> Result<()>;
}

// Storage Provider — bring your own database
#[async_trait]
pub trait StorageProvider: Send + Sync {
    async fn ping(&self) -> Result<()>;
    fn connection(&self) -> &DatabaseConnection;
}
```

## SSE Protocol

The tool call interruption protocol works as follows:

```
1. Client POSTs to /api/chat (Accept: text/event-stream)
2. Server streams SSE events:
   - event: meta      → { chat_id, message_id }
   - event: content   → { delta: "Hello..." }
   - event: tool_call → { id, name, arguments }  ← STREAM PAUSES
3. Client executes tool locally
4. Client POSTs result to /api/chat/tool-result
   - { tool_call_id, result: "..." }
5. Server resumes streaming:
   - event: content   → { delta: "Based on the result..." }
   - event: done      → { usage: { input_tokens, output_tokens } }
```

This enables LLM agents to use client-side tools (file system, browser, custom APIs) without the server needing direct access.

## API Overview

| Category | Endpoints |
|----------|-----------|
| **Chat** | `POST /api/chat`, `POST /api/chat/stop`, `POST /api/chat/tool-result` |
| **History** | `GET /api/chats`, `GET /api/chat/:id`, `DELETE /api/chat/:id` |
| **Messages** | `GET/POST/PATCH/DELETE /api/chats/:id/messages` |
| **Agents** | `GET/POST/PATCH/DELETE /api/agents` |
| **Prompts** | `GET/POST/PATCH/DELETE /api/prompts`, `POST /api/prompts/:key/render` |
| **Memory** | `POST /api/memories/search`, `GET/DELETE /api/memories` |
| **Auth** | `POST /api/auth/login`, `POST /api/auth/register`, `POST /api/keys` |
| **Models** | `GET /api/models` |

## Configuration

All configuration is via environment variables. See [`.env.example`](.env.example) for the full list.

Key variables:

| Variable | Description | Default |
|----------|-------------|---------|
| `DATABASE_URL` | PostgreSQL connection string | required |
| `REDIS_URL` | Redis connection string | `redis://127.0.0.1:6379` |
| `QDRANT_URL` | Qdrant vector DB URL | `http://localhost:6334` |
| `OPENAI_API_KEY` | OpenAI API key | - |
| `JWT_SECRET` | JWT signing secret | required |
| `HOST` | Server bind address | `0.0.0.0` |
| `PORT` | Server port | `3000` |

## Tech Stack

- **Rust** + **Axum** 0.7 — async HTTP with SSE streaming
- **SeaORM** — database-agnostic ORM (PostgreSQL, MySQL, SQLite)
- **Qdrant** — vector similarity search
- **Redis** — caching and session storage
- **async-openai** — OpenAI-compatible client
- **DashMap** — concurrent state for active tool call channels

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup and guidelines.

## License

[Apache 2.0](LICENSE)
