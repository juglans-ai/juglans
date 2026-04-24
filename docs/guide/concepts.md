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
| `input.field` | Global | Input data passed via CLI or API (includes `input.chat_id` when triggered by a bot adapter) |
| `output` | Per-node | Output of the previous node |
| `key` | Global | Custom context variables (via assignment syntax) |
| `reply.output` / `reply.chat_id` | Per-chat | Metadata from the agent's response; `reply.chat_id` chains history across `chat()` nodes in one run |
| `error` | Error path | Error information object (available in `on error` paths) |
| `config` | Global | Parsed `juglans.toml`, e.g. `config.server.port` |
| `response` | Global | Written by `response()` in `serve()`-backed HTTP handlers |

Reserved top-level names: `input`, `output`, `reply`, `error`, `config`, `response`. Don't use them as variable names you write to.

### Message state and conversation history

`chat()` accepts a `state=` parameter that controls the message lifecycle on two axes: whether the message feeds back into the chat history (`context`) and whether it streams out via SSE (`display`). Four canonical values:

| state | Persist to history? | Stream to user? |
|---|---|---|
| `context_visible` (default) | ✓ | ✓ |
| `context_hidden` | ✓ | ✗ |
| `display_only` | ✗ | ✓ |
| `silent` | ✗ | ✗ |

When a `chat_id` is resolved (explicit `chat_id=`, `reply.chat_id`, or `input.chat_id` injected by bot adapters), Juglans auto-loads the tail of the thread and appends the turn — so bot workflows get multi-turn memory with no extra wiring. See [Conversation History in connect-ai.md](./connect-ai.md#conversation-history) for the full story.

### Bot adapters

Four platforms ship as first-class adapters: **Telegram**, **Discord**, **Feishu / Lark**, and **WeChat**. Each one runs as `juglans bot <platform>` standalone, or auto-starts inside `juglans serve` when its `[bot.<platform>]` section is present in `juglans.toml`. Inbound messages flow through the same `run_agent_for_message` pipeline, populating `input.platform`, `input.platform_chat_id`, `input.platform_user_id`, `input.text`, and the namespaced `input.chat_id` (e.g. `discord:{channel_id}:{agent_slug}`) so conversation history stays per-thread.

Outbound push uses the platform-namespaced builtins — `telegram.send_message`, `discord.send_message`, `wechat.send_message`, `feishu.send_message` (and friends like `*.typing`, `*.edit_message`, `discord.react`, `feishu.send_image`). Targets auto-resolve from `input.platform_chat_id` on the inbound path, or take an explicit `chat_id` / `channel_id` / `user_id` for cron jobs and broadcasts. See [builtins.md → Platform Messaging](../reference/builtins.md#platform-messaging-telegram-discord-wechat-feishu).

Variables are accessed by path within node parameters:

```juglans
[save]: user = input.name
[greet]: print(message="Hello, " + user)
[save] -> [greet]
```

## Tool Resolution Order

When a node invokes a tool, the engine searches in the following order:

```
1. Builtin         — chat, p, notify, print, fetch, bash, history.*, db.*,
                     telegram.*, discord.*, wechat.*, feishu.*
2. Function        — [name(params)]: { ... } defined in the current workflow
3. Struct methods  — Type.fn() / instance.method() on struct / impl blocks
4. Python          — Direct Python module calls (pandas.read_csv(), etc.)
5. MCP             — External tools surfaced via the `mcp=` parameter on chat()
6. Client Bridge   — Unmatched tools forwarded to the frontend via SSE
```

MCP tools are declared inline on `chat(mcp={...})`; see [How to Use MCP Tools](./use-mcp.md).

Calling builtin tools in a Workflow:

```juglans
[step1]: print(message="start")
[step2]: notify(status="processing")
[step3]: result = "done"
[step1] -> [step2] -> [step3]
```

## Local-First Architecture

```
┌──────────────────────────────────────────┐
│              Juglans                     │
│            (Local Engine)                │
│                                          │
│  - DSL parsing (.jg / .jgx files)        │
│  - DAG execution                         │
│  - Tool calls: builtin / Python / MCP    │
│  - Direct LLM provider calls             │
│    (OpenAI, Anthropic, DeepSeek, Qwen,   │
│     Gemini, xAI, Ollama, ...)            │
└──────────────────────────────────────────┘
```

Juglans runs entirely on your machine. It parses workflows, executes the DAG, and calls LLM providers directly using API keys you configure either in `juglans.toml` or via environment variables. There is no remote backend dependency — no cloud server, no proxy, no account required to run a workflow.

Resource referencing: define agents inline (recommended) or import them from a library file via `libs:`:

```juglans
[my_agent]: {
  "model": "gpt-4o-mini",
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
[assistant]: { "model": "gpt-4o-mini", "system_prompt": "You are helpful." }

# main.jg
libs: ["./agents.jg"]
[ask]: chat(agent=agents.assistant, message=input.query)
```

**Prompt-driven pattern:** System prompts can live in `.jgx` files, rendered at runtime with `p(slug="...", param=value)` inside `chat()` calls. This keeps prompts separate, reusable, and version-controlled.

## Next Steps

- [Workflow Syntax](../reference/workflow-spec.md) -- Full syntax reference
- [Agent Syntax](../reference/agent-spec.md) -- Inline agent configuration
- [Prompt Syntax](../reference/prompt-spec.md) -- Template syntax
- [Connect AI](./connect-ai.md) -- Connecting to AI models
- [Debugging](./debugging.md) -- Debugging and troubleshooting
