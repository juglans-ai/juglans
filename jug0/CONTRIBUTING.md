# Contributing to jug0

## Development Setup

### Prerequisites

- Rust 1.80+ (`rustup update stable`)
- PostgreSQL 15+
- Redis 7+
- Qdrant 1.10+

### Quick Start

```bash
# Start dependencies
docker compose up -d postgres redis qdrant

# Configure
cp .env.example .env

# Run migrations
cargo run -p migration -- up

# Build and run
cargo run

# Run tests
cargo test

# Lint
cargo fmt --check
cargo clippy -- -D warnings
```

## Code Style

- `cargo fmt` for formatting
- `cargo clippy -- -D warnings` for linting
- Follow existing patterns in the codebase

## Pull Request Process

1. Fork the repository
2. Create a feature branch (`git checkout -b feat/my-feature`)
3. Make your changes
4. Run `cargo fmt && cargo clippy -- -D warnings && cargo test`
5. Commit with [Conventional Commits](https://www.conventionalcommits.org/) format
6. Open a Pull Request

## Adding a New LLM Provider

1. Create `src/providers/llm/your_provider.rs`
2. Implement the `LlmProvider` trait
3. Register in `ProviderFactory::get_provider()` match arm
4. Add API key env var to `.env.example`

## Adding a New Memory/Cache/Storage Backend

1. Create `src/providers/{memory|cache|storage}/your_backend.rs`
2. Implement the corresponding Provider trait
3. Wire into `AppState` in `main.rs`
