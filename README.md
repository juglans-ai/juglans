<p align="center">
  <img src="https://juglans.ai/logo.svg" alt="Juglans" width="120" />
</p>

<h1 align="center">Juglans</h1>

<p align="center">
  AI workflow orchestration DSL — compiler, runtime & CLI
</p>

<p align="center">
  <a href="https://github.com/juglans-ai/juglans/actions"><img src="https://github.com/juglans-ai/juglans/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <a href="https://github.com/juglans-ai/juglans/releases"><img src="https://img.shields.io/github/v/release/juglans-ai/juglans" alt="Release" /></a>
  <a href="https://github.com/juglans-ai/juglans/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License" /></a>
</p>

---

Juglans is a declarative workflow language for building and orchestrating AI agent pipelines. Write `.jg` files to define DAG-based workflows, `.jgagent` for agent configs, and `.jgprompt` for reusable prompt templates.

## Features

- **Declarative DAG workflows** — conditional edges, switch routing, loops, error handling, function definitions
- **AI-native builtins** — `chat()`, `p()` (prompt render), memory, history
- **50+ expression functions** — Python-inspired: `len`, `map`, `filter`, `reduce`, `zip`, `sorted`, ...
- **Lambda expressions** — `filter($list, x => x > 10)`
- **Python interop** — call pandas, sklearn, any Python module directly
- **MCP integration** — connect external tools via Model Context Protocol
- **Package ecosystem** — publish & install reusable workflow packages
- **Web server** — built-in Axum server with SSE streaming
- **Docker deployment** — `juglans deploy` for one-command containerization
- **Cross-platform** — macOS, Linux, Windows + WASM support

## Quick Install

```bash
curl -fsSL https://raw.githubusercontent.com/juglans-ai/juglans/main/install.sh | sh
```

## Hello World

```yaml
# hello.jg
[greet]: chat(message="Say hello in 3 languages")
[format]: chat(message="Format as a numbered list: $output")
[greet] -> [format]
```

```bash
juglans hello.jg
```

## Agent Example

```yaml
# analyst.jgagent
name: analyst
model: claude-sonnet-4-20250514
system: You are a data analyst. Answer questions with data-driven insights.
tools: ["devtools"]
```

```bash
juglans analyst.jgagent --message "Analyze the CSV in ./data/"
```

## CLI

```bash
juglans <file>                    # Execute .jg / .jgagent / .jgprompt
juglans check [path]              # Validate syntax
juglans web --port 8080           # Local dev server (SSE)
juglans deploy                    # Docker deployment
juglans pack / publish            # Package management
juglans chat                      # Interactive TUI
```

## Documentation

- [Getting Started](https://docs.juglans.ai/getting-started/)
- [Workflow Guide](https://docs.juglans.ai/guide/)
- [Builtin Reference](https://docs.juglans.ai/reference/)
- [Python Integration](https://docs.juglans.ai/integrations/python.html)
- [MCP Integration](https://docs.juglans.ai/integrations/mcp.html)

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for development setup and guidelines.

## License

[MIT](LICENSE)
