# Juglans

**Juglans** is the first AI orchestration language that treats **graph topology as a first-class programming primitive**.

> Others write code to draw graphs. We write graphs as code.

In traditional programming, execution is fundamentally sequential — even async/await is syntactic sugar over linear control flow. In traditional workflow tools, the graph is a scheduling artifact generated from code. **Juglans inverts this: the graph topology IS the program.**

## Why Topology Matters

In the era of AI agents, the **structure of how agents interact** — who talks to whom, in what order, with what branching — is often more important than any individual agent's capability.

Juglans makes this structure **explicit, composable, and verifiable**:

```yaml
[classify] -> switch $output.intent {
    "question": [answer]
    "task": [execute]
    default: [fallback]
}
[answer] -> [review]
[execute] -> [review]
[review] if $output.quality < 0.8 -> [refine]
[refine] -> [review]
```

The topology of this code — branching, convergence, cycles — **IS** the architecture diagram. No separate drawing needed.

## What Makes Juglans Different

### Node = Function

A node is simultaneously a **graph vertex** (with edges, topological position) and a **callable function** (with parameters, a body, reusable). Function calls don't flatten the topology — sub-graphs are expanded in place, preserving structure.

```yaml
[deploy(env, version)]: {
  bash(command="docker build -t app:" + $version + " .")
  bash(command="kubectl apply --namespace=" + $env)
}

[staging]: deploy(env="staging", version="1.2.0")
[production]: deploy(env="production", version="1.2.0")
[staging] -> [production]
```

### Topology-Preserving Composition

`flows:` imports perform **graph merging** — the sub-workflow's complete topology (nodes, edges, branches) is embedded into the parent graph with namespace prefixes. This is an **embedding**, not a projection. No structural information is lost.

```yaml
flows: {
  auth: "./auth.jg"
  trading: "./trading.jg"
}
[route] if $ctx.need_auth -> [auth.start]
[auth.done] -> [trading.begin]
```

Compare: Python function calls flatten the call stack. Microservice calls hide network topology. Juglans sub-graphs remain **fully visible, reasonnable, and optimizable** within the parent graph.

### Computation Topology = API Topology

With `serve()`, the routing topology of your HTTP API and the execution topology of your computation are **the same graph**:

```yaml
[request]: serve()
[request] -> switch $input.route {
  "GET /api/hello": [hello]
  "POST /api/data": [process]
  default: [not_found]
}
```

### How We Compare

| Paradigm | Role of Graph | Limitation |
|----------|--------------|------------|
| Airflow / Prefect | Python code **generates** DAG; graph is a scheduling artifact | Graph is a second-class citizen |
| Terraform | Declarative dependency graph | No control flow, no functions, no runtime branching |
| BPMN / workflow engines | Visual XML graphs | Verbose, no function abstraction, not composable |
| LangGraph / CrewAI | State machines between agents | State machines are not graphs — no true topological composition |
| **Juglans** | **Graph topology IS the program** | — |

## Features

- **Declarative DSL** — Three file formats: `.jg` (workflows), `.jgprompt` (templates), `.jgagent` (agents)
- **Graph Execution Engine** — DAG traversal with conditionals, switch routing, loops (`while`, `foreach`), error handling
- **Function Definitions** — `[name(params)]: { steps }` — reusable parameterized node blocks
- **Topology-Preserving Composition** — `flows:` merges sub-workflow graphs with namespace isolation
- **Template System** — Jinja-style prompt templates with variable interpolation
- **Expression Language** — Python-like expressions with 30+ built-in functions
- **HTTP Backend** — `serve()` + `response()` turn workflows into HTTP APIs
- **Multi-Agent Collaboration** — Declarative agent interaction topology
- **MCP Integration** — Model Context Protocol for extensible tool capabilities
- **Client Tool Bridge** — Unresolved tool calls forwarded to frontend via SSE
- **Jug0 Backend** — Seamless integration with Jug0 AI platform
- **Cross-Platform** — Native + WebAssembly

## Installation

### Quick Install (macOS & Linux)

```bash
curl -fsSL https://juglans.ai/get-sdk | sh
```

Or use the GitHub direct link:

```bash
curl -fsSL https://raw.githubusercontent.com/juglans-ai/juglans/main/install.sh | sh
```

### macOS (Homebrew)

```bash
# Add tap
brew tap juglans-ai/tap

# Install
brew install juglans

# Verify installation
juglans --version
```

### Linux

```bash
# Use install script (recommended)
curl -fsSL https://juglans.ai/get-sdk | sh

# Or download from Releases
wget https://github.com/juglans-ai/juglans/releases/latest/download/juglans-linux-x64.tar.gz
tar -xzf juglans-linux-x64.tar.gz
sudo mv juglans /usr/local/bin/
```

### Windows

```powershell
# PowerShell install
irm https://juglans.ai/get-sdk.ps1 | iex

# Or download manually
# https://github.com/juglans-ai/juglans/releases/latest
```

### Build from Source

```bash
# Prerequisites: Rust 1.70+
git clone https://github.com/juglans-ai/juglans.git
cd juglans
cargo build --release

# Binary location: target/release/juglans
```

## Quick Start

### Create Your First Agent

```yaml
# my-agent.jgagent
slug: "my-assistant"
name: "My Assistant"
model: "deepseek-chat"
temperature: 0.7
system_prompt: "You are a helpful assistant."
```

Run:
```bash
juglans my-agent.jgagent
```

### Build from Source

```bash
# Prerequisites: Rust 1.70+
git clone https://github.com/juglans-ai/juglans.git
cd juglans
cargo build --release

# Binary location: target/release/juglans
```

## Quick Start

### Create a Prompt Template

```yaml
# greeting.jgprompt
---
slug: "greeting"
name: "Greeting Prompt"
inputs: {name: "World"}
---
Hello, {{ name }}! How can I help you today?
```

### Create a Workflow

```yaml
# chat-flow.jg
name: "Simple Chat"
version: "0.1.0"

prompts: ["./prompts/*.jgprompt"]
agents: ["./agents/*.jgagent"]

entry: [start]
exit: [end]

[start]: notify(status="Starting chat...")
[chat]: chat(agent="my-assistant", message=$input.question)
[end]: notify(status="Done")

[start] -> [chat] -> [end]
```

Run:
```bash
juglans chat-flow.jg --input '{"question": "What is Juglans?"}'
```

## Documentation

- [Quick Start](./getting-started/quickstart.md) - 5-minute getting started tutorial
- [Core Concepts](./guide/concepts.md) - Introduction to Agent, Prompt, and Workflow
- [DSL Syntax](./guide/workflow-syntax.md) - Complete syntax reference
- [CLI Reference](./reference/cli.md) - Command-line tools
- [Built-in Tools](./reference/builtins.md) - chat, p, notify, and more

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│                      Juglans CLI                        │
├─────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐     │
│  │  .jg    │  │  .jgprompt  │  │  .jgagent   │     │
│  │  Parser     │  │  Parser     │  │  Parser     │     │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘     │
│         │                │                │             │
│         ▼                ▼                ▼             │
│  ┌─────────────────────────────────────────────────┐   │
│  │              Workflow Executor                   │   │
│  │         (DAG Traversal + Context)               │   │
│  └──────────────────────┬──────────────────────────┘   │
│                         │                               │
│         ┌───────────────┼───────────────┐              │
│         ▼               ▼               ▼              │
│  ┌───────────┐   ┌───────────┐   ┌───────────┐        │
│  │  Builtins │   │  Jug0     │   │   MCP     │        │
│  │  (chat,p) │   │  Client   │   │  Client   │        │
│  └───────────┘   └───────────┘   └───────────┘        │
└─────────────────────────────────────────────────────────┘
```

## File Formats

| Extension | Purpose | Description |
|-----------|---------|-------------|
| `.jg` | Workflow | Defines nodes, edges, and execution logic |
| `.jgprompt` | Prompt Template | Reusable prompt templates |
| `.jgagent` | Agent Configuration | Model, temperature, system prompt |
| `juglans.toml` | Project Configuration | API keys, server settings |

## Configuration File

Create `juglans.toml` to configure your project:

```toml
# Account configuration
[account]
id = "user_123"
name = "Your Name"
role = "admin"
api_key = "jug0_sk_your_api_key_here"

# Workspace configuration (optional)
[workspace]
id = "workspace_456"
name = "My Workspace"
members = ["user_123"]

# Jug0 backend configuration
[jug0]
base_url = "http://localhost:3000"  # Local development
# base_url = "https://api.jug0.com"  # Production

# Web server configuration
[server]
host = "127.0.0.1"
port = 8080

# Environment variables (optional)
[env]
DATABASE_URL = "postgresql://localhost/mydb"
API_KEY = "your_api_key"

# MCP server configuration (optional)
[mcp.filesystem]
command = "npx"
args = ["-y", "@anthropic/mcp-filesystem"]
env = { ROOT_DIR = "/workspace" }

[mcp.web-browser]
url = "http://localhost:3001/mcp"
api_key = "mcp_key_..."
```

For detailed configuration instructions, see [Configuration Reference](./reference/config.md).

## Examples

See the [examples/](../examples/) directory for more examples:

- `examples/prompts/` - Prompt template examples
- `examples/agents/` - Agent configuration examples
- `examples/workflows/` - Workflow examples

## Tech Stack

- **Parser**: Pest (PEG grammar)
- **Script Engine**: Rhai (expression evaluation)
- **Graph Structure**: Petgraph (DAG)
- **Async Runtime**: Tokio
- **Web Framework**: Axum
- **Serialization**: Serde

## License

MIT License
