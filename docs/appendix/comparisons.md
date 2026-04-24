# Juglans vs Other Tools

The AI orchestration landscape in 2026 spans low-code visual builders (n8n, Dify, Flowise), code-first graph frameworks (LangGraph, CrewAI), the legacy Python ecosystem (LangChain, LlamaIndex), and heavy workflow engines (Temporal, Airflow). Juglans carves out a specific niche: a declarative DSL with first-class AI, bot adapters, and persistence, shipped as a single binary.

## Comparison Matrix

| Dimension | Juglans | LangGraph | CrewAI | LangChain | n8n / Dify | Temporal | Airflow |
|---|---|---|---|---|---|---|---|
| **Primary form** | Declarative `.jg` DSL | Python graph class | Python role graph | Python LCEL / classes | Visual no-code | Durable code | Task DAG |
| **AI as 1st-class** | ✓ (`chat`, agents inline) | ✓ (LangChain runnables) | ✓ (role-centric) | ✓ | ✓ (integrations) | ✗ (plumbing) | ✗ |
| **Runtime** | Single Rust binary | Python runtime | Python runtime | Python runtime | Node.js server | Go + worker pool | Python + scheduler |
| **Python required?** | Only for Python tools | Always | Always | Always | Workflows yes | Optional | Always |
| **State between runs** | Built-in history (JSONL / SQLite) | Checkpointer (Postgres/Memory) | In-process | Memory classes | Per-flow DB | Durable event log | XCom / DB |
| **Bot adapters built-in** | Telegram / Discord / Feishu / WeChat | None | None | None | Many (no-code) | None | None |
| **Cross-platform push from any node** | ✓ (`<platform>.send_message` etc., 10+ dotted tools) | Manual SDK call | Manual SDK call | Manual SDK call | UI-driven | Manual | Manual |
| **Streaming to UI** | SSE, server + client bridge | Native (async) | Limited | Callbacks | Webhook-driven | Not direct | Not direct |
| **MCP client** | Built-in (HTTP) | Via external libs | Via external libs | Via external libs | Via integrations | N/A | N/A |
| **Static validation** | `juglans check` (graph + types) | Runtime | Runtime | Runtime | Limited | Runtime | DAG parse |
| **Learning curve** | Low (3 concepts) | Medium (Python + graph API) | Medium | High | Very low (UI) | High | Medium-high |
| **Best for** | AI agents, bots, API glue | Complex Python AI graphs | Multi-agent teams | RAG + LLM pipelines | Business automations | Durable long-running work | Batch ETL |

## What Juglans Does Differently

**Declarative over imperative.** Define *what* happens, not *how* to wire it:

```juglans
[input]: query = input.question
[search]: fetch(url="https://api.example.com/search?q=" + query)
[respond]: chat(agent=assistant, message=output)

[input] -> [search] -> [respond]
```

An equivalent Python pipeline (LangChain LCEL, LangGraph, or hand-rolled) needs type imports, chain/runnable wiring, HTTP client setup, and explicit state plumbing between steps. No-code tools (n8n, Dify) express this shape easily but require a UI — the workflow isn't reviewable as a file diff.

**Static analysis before execution.** `juglans check` validates the entire DAG before running — missing nodes, broken edges, unreachable paths, unknown builtins, type drift. LangGraph / LangChain / CrewAI only surface these at runtime.

**Built-in routing without code.** Conditional edges and `switch` are part of the DSL, not helper classes:

```juglans
[classify] -> switch output.intent {
    "question": [answer]
    "task": [execute]
    default: [fallback]
}
```

LangGraph expresses this via `add_conditional_edges` with a routing function. CrewAI doesn't branch at the graph level. n8n uses a visual Switch node.

**Shipped bot adapters, history, MCP, and platform messaging.** Multi-turn Telegram / Discord / Feishu / WeChat bots are one TOML block away — `chat_id` auto-derives per platform, history auto-persists, MCP tools attach inline. **Outbound push is also first-class**: `telegram.send_message`, `discord.send_message`, `wechat.send_message`, `feishu.send_message` (plus `typing` / `edit_message` / `react`) work from any node — cron jobs, error handlers, cross-channel alerts. No other framework in the matrix bundles these in the runtime itself.

## Juglans-Only Differentiators

- **Compiles to WebAssembly** — the parser, resolver, expression evaluator, and executor core can be embedded in a browser for static analysis and workflow visualization. (Note: builtins that need TLS, subprocess, or disk do not run in the browser; use the server in that case.)
- **First-class type system** — `class` definitions + a static checker validate structured state flow between nodes before execution, catching wiring mistakes that runtime-only tools miss.
- **LSP server** — `juglans lsp` provides diagnostics, hover, and completion for `.jg` / `.jgx` in any LSP-aware editor.
- **Conversation history as a builtin** — `chat_id` + `[history]` + `history.*` tools give multi-turn memory without threading arrays by hand or adopting an external memory class.
- **Unified `juglans serve`** — one process hosts the web API, every configured bot adapter, and cron triggers.

## When to Choose Juglans

**Pick Juglans when:**

- You're building AI agent workflows and want them in a reviewable, diff-able file
- You need multi-turn chat with durable history across runs
- You want a single binary you can drop on a VM or in a container — no Python toolchain
- You need bot adapters (Telegram / Discord / Feishu / WeChat) or SSE streaming out of the box
- You want static validation catching wiring bugs before they hit production

**Pick something else when:**

- **LangGraph / LangChain** — you're deep in the Python ecosystem, need LlamaIndex-style RAG pipelines, or want LCEL composability with Python control flow
- **CrewAI** — you're modeling teams of agents with role-based collaboration patterns as the primary abstraction
- **n8n / Dify / Flowise** — non-developers author the workflows in a visual editor; reviewability as text is not a priority
- **Temporal** — you need durable execution with retries, backoff, human-in-the-loop signals, and multi-day / multi-week runs
- **Airflow** — traditional batch ETL with scheduling, retries, and cluster workers is the core workload, not AI
