# Core Concepts

A quick reference to Juglans core concepts. This page provides a concise overview; see the dedicated guides for detailed usage.

## Two File Types

| Type | Extension | Parser | Purpose |
|------|-----------|--------|---------|
| Workflow | `.jg` | GraphParser | Define DAG execution flows, nodes, edges, conditional branches, and inline agent definitions |
| Prompt | `.jgx` | PromptParser | Jinja-style reusable prompt templates |

Agents are defined as **inline JSON map nodes** within `.jg` files -- no separate file type needed.

**Decision tree:**

```
Need orchestration or AI agents?     → .jg (Workflow)
Need reusable/templated prompts?     → .jgx (Prompt)
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
[check]: mode = input.mode
[fast]: print(message="fast path")
[slow]: print(message="slow path")

[check] if mode == "fast" -> [fast]
[check] -> [slow]
```

## Variable System

| Variable | Scope | Description |
|----------|-------|-------------|
| `input.field` | Global | Input data passed via CLI or API |
| `output` | Per-node | Output of the previous node |
| `key` | Global | Custom context variables (via assignment syntax) |
| `reply.output` | Per-chat | Metadata from the agent's response |
| `error` | Error path | Error information object (available in `on error` paths) |

Variables are accessed by path within node parameters:

```juglans
[save]: user = input.name
[greet]: print(message="Hello, " + user)
[save] -> [greet]
```

## Tool Resolution Order

When a node invokes a tool, the engine searches in the following order:

```
1. Builtin         — chat, p, notify, print, fetch, bash...
2. Function        — [name(params)]: { ... } defined in the current workflow
3. Associated Fn   — Type.function() calls on struct definitions
4. Instance Method — instance.method() calls on struct instances
5. Python          — Direct Python module calls (pandas.read_csv(), etc.)
6. Client Bridge   — Unmatched tools forwarded to the frontend via SSE
```

MCP tools are handled at the DSL level via `std/mcps.jg` (see [How to Use MCP Tools](./use-mcp.md)).

Calling builtin tools in a Workflow:

```juglans
[step1]: print(message="start")
[step2]: notify(status="processing")
[step3]: result = "done"
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

Resource referencing: define agents inline, or use `owner/slug` for remote resources:

```juglans
[my_agent]: {
  "model": "gpt-4o",
  "system_prompt": "You are a helpful assistant."
}

[local]: chat(agent=my_agent, message=input.query)
[remote]: chat(agent="juglans/cloud-agent", message=output)
[my_agent] -> [local] -> [remote]
```

## Project Structure

The recommended layout for a Juglans project:

```
src/
├── main.jg                    # Main workflow with inline agent definitions
├── agents.jg                  # Shared agent library (imported via libs:)
├── prompts/
│   └── system.jgx        # Prompt templates
└── tools/
    └── toolbox.json           # Tool definitions
```

Agents are defined as inline JSON map nodes in `.jg` files. For reuse across workflows, define agents in a library file and import with `libs:`:

```juglans
# agents.jg — shared agent library
[assistant]: { "model": "gpt-4o", "system_prompt": "You are helpful." }

# main.jg
libs: ["./agents.jg"]
[ask]: chat(agent=agents.assistant, message=input.query)
```

**Prompt-driven pattern:** System prompts can live in `.jgx` files, rendered at runtime with `p(slug="...", param=value)` inside `chat()` calls. This keeps prompts separate, reusable, and version-controlled.

## Next Steps

- [Workflow Syntax](./workflow-syntax.md) -- Full syntax reference
- [Agent Syntax](../reference/agent-spec.md) -- Inline agent configuration
- [Prompt Syntax](./prompt-syntax.md) -- Template syntax
- [Connect AI](./connect-ai.md) -- Connecting to AI models
- [Debugging](./debugging.md) -- Debugging and troubleshooting
