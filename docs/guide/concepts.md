# Core Concepts

A quick reference to Juglans core concepts. This page provides a concise overview; see the dedicated guides for detailed usage.

## Three File Types

| Type | Extension | Parser | Purpose |
|------|-----------|--------|---------|
| Workflow | `.jg` | GraphParser | Define DAG execution flows, connecting nodes, edges, and conditional branches |
| Agent | `.jgagent` | AgentParser | Configure AI model parameters (model, system prompt, tools) |
| Prompt | `.jgprompt` | PromptParser | Jinja-style reusable prompt templates |

**Decision tree:**

```
Need multi-step orchestration?       → .jg (Workflow)
Need to configure AI model behavior? → .jgagent (Agent)
Need reusable/templated prompts?     → .jgprompt (Prompt)
```

Minimal Workflow example:

```juglans
[greet]: print(message="Hello!")
[done]: print(message="Done.")
[greet] -> [done]
```

## DAG Execution Model

A Workflow is internally represented as a **Directed Acyclic Graph (DAG)**. The engine executes nodes in **topological order** -- nodes with no dependencies run first, followed by nodes whose dependencies have been satisfied.

```
     [A]
    /   \
  [B]   [C]
    \   /
     [D]
```

Execution order: A -> B, C (may run in parallel) -> D. The absence of cycles guarantees that execution always terminates.

Conditional edges are evaluated at runtime; branches whose conditions are not met are automatically skipped:

```juglans
[check]: set_context(mode=$input.mode)
[fast]: print(message="fast path")
[slow]: print(message="slow path")

[check] if $ctx.mode == "fast" -> [fast]
[check] -> [slow]
```

## Variable System

| Variable | Scope | Description |
|----------|-------|-------------|
| `$input.field` | Global | Input data passed via CLI or API |
| `$output` | Per-node | Output of the previous node |
| `$ctx.key` | Global | Custom context variables (via `set_context()`) |
| `$reply.output` | Per-chat | Metadata from the agent's response |
| `$error` | Error path | Error information object (available in `on error` paths) |

Variables are accessed by path within node parameters:

```juglans
[save]: set_context(user=$input.name)
[greet]: print(message="Hello, " + $ctx.user)
[save] -> [greet]
```

## Tool Resolution Order

When a node invokes a tool, the engine searches in the following order:

```
1. Builtin    — chat, p, notify, print, set_context, fetch, bash...
2. Function   — [name(params)]: { ... } defined in the current workflow
3. Python     — Direct Python module calls (pandas.read_csv(), etc.)
4. Client Bridge — Unmatched tools forwarded to the frontend via SSE
```

MCP tools are handled at the DSL level via `std/mcps.jg` (see [How to Use MCP Tools](./use-mcp.md)).

Calling builtin tools in a Workflow:

```juglans
[step1]: print(message="start")
[step2]: notify(status="processing")
[step3]: set_context(result="done")
[step1] -> [step2] -> [step3]
```

## Juglans and Jug0

```
┌─────────────────┐     ┌─────────────────┐
│    Juglans      │     │      Jug0       │
│   (Local CLI)   │────>│   (Backend)     │
│                 │     │                 │
│  - DSL parsing  │     │  - LLM calls    │
│  - DAG execution│     │  - Resource     │
│  - Local dev    │     │    storage      │
│                 │     │  - API service  │
└─────────────────┘     └─────────────────┘
```

- **Juglans** is the local engine: parses the DSL, executes the DAG, and manages tool calls
- **Jug0** is the backend platform: provides the LLM API, cloud resource storage, and user management
- During local development you can work offline with local files; for production deployment, resources are managed through Jug0

Resource referencing: use a slug for local resources (e.g., `"my-agent"`), and `owner/slug` for remote resources (e.g., `"juglans/assistant"`):

```juglans
[local]: chat(agent="my-agent", message=$input.query)
[remote]: chat(agent="juglans/cloud-agent", message=$output)
[local] -> [remote]
```

## Project Structure

The recommended layout for a Juglans project:

```
src/
├── main.jg                    # Main workflow (agent entry point)
├── agents/
│   └── assistant.jgagent      # Workflow-bound agent (source: "../main.jg")
├── pure-agents/
│   └── helper.jgagent         # Pure agent (model + system_prompt)
├── prompts/
│   └── system.jgprompt        # Prompt templates
└── tools/
    └── toolbox.json           # Tool definitions
```

**Two types of agents:**

- **Workflow-bound** (`agents/`) — uses `source:` to bind to a `.jg` file. No model or system prompt in the agent. The workflow controls all behavior.
- **Pure** (`pure-agents/`) — defines model, temperature, system_prompt directly. Used inside workflows via `chat(agent="slug")`.

**Prompt-driven pattern:** System prompts live in `.jgprompt` files, rendered at runtime with `p(slug="...", param=value)` inside `chat()` calls. This keeps prompts separate, reusable, and version-controlled.

## Next Steps

- [Workflow Syntax](./workflow-syntax.md) -- Full syntax reference
- [Agent Syntax](./agent-syntax.md) -- Agent configuration
- [Prompt Syntax](./prompt-syntax.md) -- Template syntax
- [Connect AI](./connect-ai.md) -- Connecting to AI models
- [Debugging](./debugging.md) -- Debugging and troubleshooting
