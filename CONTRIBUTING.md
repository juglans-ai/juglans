# Contributing to Juglans

Thank you for your interest in contributing to Juglans!

## Development Setup

### Prerequisites

- [Rust](https://rustup.rs/) (1.80+)
- Git

### Build & Test

```bash
git clone https://github.com/juglans-ai/juglans.git
cd juglans
cargo build
cargo test
```

### Code Style

```bash
cargo fmt          # Format code
cargo clippy       # Lint
```

All PRs must pass `cargo fmt --check` and `cargo clippy -- -D warnings`.

## Pull Request Process

1. Fork the repo and create a branch from `main`
2. Make your changes with clear, focused commits
3. Add tests for new functionality
4. Ensure all tests pass: `cargo test`
5. Open a PR against `main`

### Commit Messages

Follow [Conventional Commits](https://www.conventionalcommits.org/):

```
feat: add new builtin tool for X
fix: resolve panic on CJK characters in TUI
docs: update workflow syntax guide
refactor: simplify expression evaluator
test: add parser edge case tests
```

## Project Structure

```
src/
├── core/       # Parser, executor, validator, expression evaluator
├── builtins/   # Built-in tools (ai, system, devtools, http, network)
├── services/   # Config, web server, MCP client, deploy
├── registry/   # Package ecosystem
├── runtime/    # Python integration
├── adapters/   # Bot adapters (Telegram, Feishu)
└── ui/         # Terminal REPL & TUI
```

## Reporting Issues

Use [GitHub Issues](https://github.com/juglans-ai/juglans/issues) with the provided templates.

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
