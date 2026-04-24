# Juglans

**Juglans** is a language where the graph topology IS the program.

> Others write code to draw graphs. We write graphs as code.

```juglans
[classifier]: { "model": "gpt-4o-mini", "temperature": 0.0, "system_prompt": "Classify intent. Return JSON." }
[qa]: { "model": "gpt-4o-mini", "system_prompt": "Answer questions accurately." }
[executor]: { "model": "gpt-4o-mini", "system_prompt": "Execute tasks step by step." }
[reviewer]: { "model": "gpt-4o-mini", "system_prompt": "Review and improve responses." }

[classify]: chat(agent=classifier, format="json")
[answer]: chat(agent=qa, message=input.query)
[execute]: chat(agent=executor, message=input.task)
[fallback]: print(message="Unknown intent")
[review]: chat(agent=reviewer, message=output)

[classify] -> switch output.intent {
    "question": [answer]
    "task": [execute]
    default: [fallback]
}
[answer] -> [review]
[execute] -> [review]
```

This code IS the architecture diagram. The branching, routing, and convergence are explicit in the syntax — no separate drawing needed.

## Why Juglans?

In the era of AI agents, **how agents interact** — who talks to whom, in what order, with what branching — matters more than any individual agent's capability. Traditional tools make this structure implicit:

| Approach | Problem |
|----------|---------|
| Airflow / Prefect | Python code generates the DAG; graph is a second-class artifact |
| LangGraph / CrewAI | State machines between agents; no true topological composition |
| Terraform | Declarative graph, but no control flow or functions |
| BPMN | Verbose XML; not composable |
| **Juglans** | **Graph topology is the program** — composable, verifiable, executable |

## Two File Types

| Extension | Purpose | Example |
|-----------|---------|---------|
| `.jg` | Workflow | Nodes, edges, branching, loops, inline agent definitions |
| `.jgx` | Prompt Template | Jinja-style variable interpolation |

## Key Features

- **Declarative DSL** — Define workflows as graphs, not imperative code
- **Functions as Nodes** — `[name(params)]: { steps }` — reusable parameterized blocks
- **Topology-Preserving Composition** — `flows:` merges sub-graphs without losing structure
- **Expression Language** — Python-like expressions with 100+ built-in functions
- **Built-in AI** — `chat()` for LLM calls, `p()` for prompt rendering
- **Bot adapters** — Telegram, Discord, Feishu, WeChat as one flag (`juglans bot <platform>`); inbound `chat_id` auto-injects for multi-turn memory
- **Platform messaging** — `telegram.send_message`, `discord.send_message`, `wechat.send_message`, `feishu.send_message` (and friends) — push from any node
- **Conversation history** — JSONL / SQLite / memory backends, auto-loaded into `chat()` when `chat_id` is set
- **HTTP Backend** — `serve()` + `response()` (and `@get` / `@post` decorators) turn workflows into APIs
- **MCP Integration** — Extend with any Model Context Protocol tool via inline `chat(mcp={...})`
- **Python Ecosystem** — Call pandas, sklearn, etc. directly from workflows

## Quick Install

```bash
# Prebuilt binary (recommended) — latest GitHub release
curl -fsSL https://raw.githubusercontent.com/juglans-ai/juglans/main/install.sh | sh

# From source (requires git clone first, Rust 1.80+)
git clone https://github.com/juglans-ai/juglans.git && cd juglans
cargo install --path .
```

Verify:
```bash
juglans --version
```

## Hello World

Create `hello.jg`:

```juglans
[greet]: print(message="Hello, Juglans!")
[done]: print(message="Workflow complete.")
[greet] -> [done]
```

Run it:
```bash
juglans hello.jg
```

## Learning Path

| You want to... | Start here |
|----------------|------------|
| **Get running in 5 minutes** | [Quick Start](./getting-started/quickstart.md) |
| **Learn the language step by step** | [Tutorial 1: Hello Workflow](./tutorials/hello-workflow.md) |
| **Look up a specific tool or syntax** | [Reference: Built-in Tools](./reference/builtins.md) |
| **See real-world examples** | [Tutorial 9: Full Project](./tutorials/full-project.md) |
| **Deploy to production** | [Deploy with Docker](./guide/deploy-docker.md) |

## Architecture

```
┌─────────────────────────────────────────────────┐
│                   Juglans CLI                    │
├─────────────────────────────────────────────────┤
│  .jg Parser          .jgx Parser            │
│       │                     │                    │
│       ▼                     ▼                    │
│  ┌─────────────────────────────────────────┐    │
│  │         Workflow Executor (DAG)          │    │
│  └────────────────────┬────────────────────┘    │
│         ┌─────────────┼─────────────┐           │
│         ▼             ▼             ▼           │
│    Builtins      LLM Providers   MCP Tools      │
│  (chat, print,    (OpenAI,      (filesystem,    │
│   bash, etc.)    Anthropic,      browser)       │
│                  DeepSeek...)                   │
└─────────────────────────────────────────────────┘
```

## License

MIT License
