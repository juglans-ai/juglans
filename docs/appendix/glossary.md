# Glossary

Core terminology used throughout Juglans documentation.

---

**Agent** -- An AI persona defined as an inline JSON map node in a `.jg` file, specifying model, system prompt, tools, and behavior parameters. Referenced by node ID in `chat()` calls (e.g., `chat(agent=my_agent, ...)`). For cross-workflow reuse, agents can be defined in a library file and imported via `libs:`.

**Bot adapter** -- A server-side integration that connects a chat platform (Telegram, Feishu, WeChat, Discord) to a Juglans workflow, translating inbound messages into workflow runs and replies back to the platform. Started via `juglans bot` or the unified `juglans serve`. Each adapter sets `input.chat_id` using a platform-specific key (`discord:{channel_id}:{agent_slug}` for Discord, `telegram:{chat_id}:{agent_slug}` for Telegram, etc.) so conversation-history buckets stay isolated per thread.

**Builtin** -- A tool implemented directly in the Juglans Rust runtime (e.g., `chat`, `fetch`, `notify`), available without any external configuration.

**CLI** -- The `juglans` command-line interface, the primary way to execute workflows, manage resources, and run development tools.

**`chat_id`** -- The routing key that identifies a single conversation thread in the history store. Resolved per `chat()` call in priority order: explicit `chat_id=` → `reply.chat_id` (chained within a run) → `input.chat_id` (auto-injected by bot adapters as `"{platform}:{user}:{agent}"`) → stateless. Used as the primary key in the JSONL / SQLite history backends.

**Class** -- A named, typed schema declared inside a `.jg` file. Classes define the shape of structured data so the type checker can validate how values flow between nodes.

**Client Bridge** -- A mechanism where unresolved tool calls are forwarded to the frontend via SSE events, allowing the UI to handle tools that require user interaction.

**Context** -- The shared state (`WorkflowContext`) that accumulates data during workflow execution, accessible as bare identifiers (e.g., `count`, `user.name`).

**Conversation History** -- Persistent storage of `chat()` turns keyed by `chat_id`. Configured via the `[history]` section in `juglans.toml`; backends include JSONL (default, one file per thread), SQLite (indexed, for concurrent writers), and in-memory (tests). Auto-loads the tail of a thread before each `chat()` call and appends the turn afterwards; honors the `state=` parameter.

**Cron** -- A built-in scheduler (`juglans cron`) that runs workflows on recurring schedules. Cron entries live alongside workflows and are served by `juglans serve`.

**DAG** -- Directed Acyclic Graph. The underlying data structure of a workflow -- nodes connected by directed edges with no cycles.

**Decorator** -- A `@fn(args)` annotation placed above a `[node]` definition, expanded at compile time by the decorator macro system. Used by adapters and web routing (`@get("/path")`, `@post("/path")`) to attach metadata without imperative wiring.

**Devtools** -- Developer tools (`read_file`, `write_file`, `edit_file`, `glob`, `grep`, `bash`) available as builtins for agent use in code-editing scenarios.

**Doctest** -- Inline test assertions embedded in `.jg` files, executed by `juglans doctest` to verify workflows behave as documented.

**Edge** -- A directed connection between two nodes, defining execution order. Can be unconditional (`[A] -> [B]`), conditional (`[A] if x -> [B]`), or error-handling (`[A] on error -> [B]`).

**Entry Node** -- Determined automatically by topological sort -- the node(s) with in-degree 0 (no incoming edges).

**Exit Node** -- Terminal node(s) with no outgoing edges. The output of terminal nodes becomes the workflow's final result.

**Expression** -- A Python-like expression evaluated at runtime (e.g., `output.count > 10`, `len(items)`). Supports arithmetic, comparison, logical operators, and 30+ built-in functions.

**Flow Import** -- Compile-time merging of another `.jg` file's nodes into the current workflow via `flows: { alias: "path.jg" }`. Imported nodes are namespaced with the alias prefix.

**Function Definition** -- A reusable parameterized block defined with `[name(params)]: { steps }`. Stored separately from the main DAG and invoked like a tool call.

**Instance Arena** -- The runtime store that holds live `class` instances during workflow execution. Nodes hand around stable references into the arena rather than copying the objects themselves, which keeps large structured values cheap to pass between steps.

**LocalRuntime** -- The juglans runtime. Calls LLM providers (OpenAI, Anthropic, DeepSeek, Gemini, Qwen, ByteDance Ark, xAI) directly via their APIs, plus two specialty providers: `claude-code` (dispatches through the local `claude` CLI) and `juglans` (routes through the juglans-wallet proxy so agents don't hold upstream credentials). The runtime itself is local-first; no inbound connection to a Juglans service is required.

**Message State** -- The `state=` argument on `chat()`, which controls a message's lifecycle on two axes: whether it persists into conversation history (`context`) and whether it streams to the UI via SSE (`display`). Canonical values: `context_visible` (both), `context_hidden` (history only), `display_only` (UI only), `silent` (neither).

**LSP** -- Language Server Protocol. `juglans lsp` implements an LSP server for `.jg`/`.jgx` files, providing diagnostics, hover, and completion in any LSP-compatible editor.

**Lib Import** -- Importing function-only library files via `libs: ["./lib.jg"]`. Only function definitions are imported, not the main workflow graph.

**MCP** -- Model Context Protocol. A standard for connecting external tool servers. Juglans connects to MCP servers via HTTP/JSON-RPC.

**Manifest Parser** -- The component that reads a package's `juglans.toml` manifest and resolves its metadata, dependencies, and entry points. Used by `juglans pack`, `publish`, `add`, and `install`.

**Metadata** -- The header section of a `.jg` file containing resource import declarations. Valid keys: `libs`, `flows`, `prompts`, `tools`, `python`. Package-level fields like `slug` / `name` / `version` live in the `.jgflow` manifest instead.

**Node** -- The fundamental unit of a workflow. Each node has an ID (in brackets) and executes a single tool call: `[node_id]: tool(params)`.

**Platform Messaging** -- The family of dotted builtins `{telegram, discord, wechat, feishu}.{send_message, typing, edit_message, react, send_image, send_webhook}` that push messages to a chat platform from any workflow node. Each `*.send_message` auto-resolves its target from `input.platform_chat_id` (set by the adapter on inbound messages), so a bot reply branch is one line: `[reply]: telegram.send_message(text="hi")`. From cron / error / non-bot contexts, pass an explicit `chat_id` / `channel_id` / `user_id`.

**Prompt** -- A Jinja-style template defined in a `.jgx` file, rendered with the `p()` builtin. Supports variables, conditionals, and loops.

**Registry** -- The package registry (`jgr.juglans.ai`) where Juglans packages are published and installed via `juglans publish` / `juglans add`.

**Serve** -- The unified server subcommand (`juglans serve`) that hosts the web API, bot adapters, and cron triggers together in a single process, replacing the older standalone `juglans web` / `juglans bot` pattern.

**Skill** -- A packaged, reusable capability (a workflow plus its prompts, agents, and tools) that can be listed and invoked via `juglans skills`. Skills are the primary unit of sharing on the registry.

**Slug** -- A URL-safe identifier for a resource. Used to reference prompts and workflows. For agents, the node ID serves as the identifier.

**Switch** -- Multi-branch routing that executes exactly one matching path: `[node] -> switch var { "a": [x], default: [y] }`.

**Tool** -- Any callable operation in a workflow node. Resolution order: builtin, function definition, Python module, MCP server, client bridge.

**Type checker** -- The static analysis pass (part of `juglans check`) that validates class definitions, node inputs/outputs, and cross-node data flow before execution, catching wiring mistakes that would otherwise surface at runtime.

**Variable** -- A runtime reference to data: `input` (CLI/adapter input, including `input.chat_id` for history routing and `input.platform_chat_id` as the raw target id used by `*.send_message`), `output` (previous node result), `reply.*` (agent response metadata, including `reply.chat_id` for chaining), `error` (`on error` paths), `config` (parsed `juglans.toml`), `response` (HTTP handlers), plus loop-scoped `loop.index` / `loop.item` inside `foreach` / `while` blocks. Reserved names cannot be used as user variables.

**Workflow** -- A complete execution graph defined in a `.jg` file, consisting of metadata, nodes, edges, and optional function definitions.
