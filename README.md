<p align="center">
  <img src="https://juglans.ai/logo.svg" alt="Juglans" width="120" />
</p>

<h1 align="center">Juglans</h1>

<p align="center">
  <b>The graph topology <i>is</i> the program.</b><br/>
  A Rust-native DSL for AI agent workflows — where the DAG you draw is the DAG you run.
</p>

<p align="center">
  <a href="https://github.com/juglans-ai/juglans/actions"><img src="https://github.com/juglans-ai/juglans/actions/workflows/ci.yml/badge.svg" alt="CI" /></a>
  <a href="https://github.com/juglans-ai/juglans/releases"><img src="https://img.shields.io/github/v/release/juglans-ai/juglans" alt="Release" /></a>
  <a href="https://github.com/juglans-ai/juglans/blob/main/LICENSE"><img src="https://img.shields.io/badge/license-MIT-blue.svg" alt="License" /></a>
  <img src="https://img.shields.io/badge/rust-1.80%2B-orange.svg" alt="Rust 1.80+" />
</p>

---

Others write code to draw graphs. Juglans writes graphs as code. Your workflow file is a directed acyclic graph of typed nodes and edges — the compiler parses it, validates it, and runs it. No DAG-builder boilerplate, no state-machine glue, no Python harness.

```juglans
# router.jg — classify user input, then branch

[assistant]: { "model": "gpt-4o-mini", "temperature": 0.0, "system_prompt": "Classify user input. Return JSON with key 'intent' set to 'question' or 'task'." }

[classify]: chat(agent=assistant, message=input.query, format="json")
[answer]:   print(message="Answering: " + input.query)
[execute]:  print(message="Executing: " + input.query)
[fallback]: print(message="I did not understand.")

[assistant] -> [classify]

[classify] -> switch output.intent {
    "question": [answer]
    "task":     [execute]
    default:    [fallback]
}
```

```bash
juglans router.jg --input '{"query": "What is a DAG?"}'
```

That file IS the architecture diagram. The branching, routing, and convergence are explicit in the syntax.

## Why Juglans?

| Approach | Problem Juglans solves |
|---|---|
| Airflow / Prefect | Python code generates the DAG; the graph is a second-class artifact. |
| LangGraph / CrewAI | State machines between agents; no true topological composition. |
| Terraform | Declarative graph, but no control flow, no functions, no AI. |
| BPMN / XML | Verbose, not composable, no runtime. |
| **Juglans** | **Graph topology is the program** — composable, verifiable, executable in one step. |

## Features

- **Declarative DAG** — conditional edges, `switch` routing, `foreach` / `while` loops, `on error` handlers, `[name(params)]: { ... }` function definitions
- **Inline agents** — agents are JSON map nodes defined alongside the workflow that uses them, no separate file
- **100+ expression functions** — Python-like syntax: `len`, `map`, `filter`, `reduce`, `sort_by`, `group_by`, `zip`, `regex_*`, `json`, `uuid`, date helpers, lambdas
- **Embedded HTTP backend** — `serve()` turns a workflow into an Axum handler; every URL hits the workflow as an axum fallback
- **Native LLM providers** — OpenAI, Anthropic, DeepSeek, Google Gemini, Qwen, xAI, ByteDance Ark (no broker, no proxy)
- **Python ecosystem bridge** — `python: ["pandas", "sklearn"]` and call modules directly, with object references for non-serializable types
- **MCP integration** — plug in any Model Context Protocol server as a tool source
- **Package registry** — `juglans pack` / `publish` / `add` to share reusable libraries
- **Bot adapters** — Telegram, Discord, Feishu, WeChat — one flag turns a workflow into a chatbot, with auto-injected `chat_id` for multi-turn memory
- **Platform messaging** — push from any node via `telegram.send_message` / `discord.send_message` / `wechat.send_message` / `feishu.send_message` (and friends — see [builtins.md](docs/reference/builtins.md))
- **Cross-platform** — macOS, Linux, Windows, and WASM (full engine runs in the browser)

## Install

```bash
# Prebuilt binary (recommended) — latest GitHub release
curl -fsSL https://raw.githubusercontent.com/juglans-ai/juglans/main/install.sh | sh

# From source — requires Rust 1.80+
git clone https://github.com/juglans-ai/juglans.git
cd juglans && cargo install --path .
```

Verify with `juglans --version`.

## 30-Second Quick Start

```bash
cat > hello.jg <<'EOF'
[greet]: print(message="Hello, " + input.name + "!")
[done]:  print(message="Workflow complete.")
[greet] -> [done]
EOF

juglans hello.jg --input '{"name": "World"}'
```

Next: read the [Quick Start guide](docs/getting-started/quickstart.md) and [Tutorial 1](docs/tutorials/hello-workflow.md).

## CLI

```bash
# Run & validate
juglans <file>              # Execute a .jg or .jgx file
juglans check [path]        # Validate syntax (like cargo check)
juglans test [path]         # Run test_* nodes across the project
juglans doctest [path]      # Validate code blocks in markdown docs

# Dev loop
juglans web       --port 3000      # Local HTTP server with SSE streaming
juglans serve     --port 3000      # Unified web API + all configured bot adapters
juglans chat      --agent path.jg  # Interactive TUI
juglans cron      file --schedule  # Run on a cron schedule
juglans lsp                        # Language Server Protocol
juglans bot       <platform>       # Telegram / Discord / Feishu / WeChat adapter

# Packages
juglans init <name>       # Scaffold a new project
juglans install           # Install jgpackage.toml dependencies
juglans add <pkg>         # Add a package dependency
juglans remove <pkg>      # Remove a package dependency
juglans pack              # Build a .tar.gz archive
juglans publish           # Publish to the registry
juglans skills            # Sync Agent Skills from GitHub

# Deploy & account
juglans deploy    [--tag] [--push]  # Build a Docker image and run it
juglans whoami                      # Show current account info
```

Run `juglans --help` or `juglans <cmd> --help` for every flag.

## Architecture

```
┌────────────────────────────────────────────────────────┐
│                      Juglans CLI                        │
├────────────────────────────────────────────────────────┤
│     .jg Parser                       .jgx Parser        │
│          │                                │             │
│          ▼                                ▼             │
│  ┌──────────────────────────────────────────────┐       │
│  │           Workflow Executor (DAG)             │       │
│  │    cycles check · variable resolve · run      │       │
│  └──────────────────────┬────────────────────────┘       │
│           ┌─────────────┼─────────────┬─────────┐       │
│           ▼             ▼             ▼         ▼       │
│       Builtins    LLM Providers   MCP Tools  Python     │
│      (chat, p,     (OpenAI,      (filesystem, (pandas,  │
│       bash, db,    Anthropic,     github,     sklearn,  │
│       http, ...)   DeepSeek...)   browser)    numpy)    │
└────────────────────────────────────────────────────────┘
```

## Documentation

- **Official docs** — <https://docs.juglans.dev>
- **In-repo mdbook source** — [`docs/SUMMARY.md`](docs/SUMMARY.md)
- **Learning path** — [Getting Started](docs/getting-started/) → [Tutorials](docs/tutorials/) → [Reference](docs/reference/)

## Contributing

Issues, PRs, and discussions are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) for build steps and code conventions.

## License

[MIT](LICENSE)
