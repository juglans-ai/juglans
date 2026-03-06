# Glossary

Core terminology used throughout Juglans documentation.

---

**Agent** -- An AI persona defined in a `.jgagent` file, specifying model, system prompt, tools, and behavior parameters. Referenced by slug in `chat()` calls.

**Builtin** -- A tool implemented directly in the Juglans Rust runtime (e.g., `chat`, `fetch`, `set_context`, `notify`), available without any external configuration.

**CLI** -- The `juglans` command-line interface, the primary way to execute workflows, manage resources, and run development tools.

**Client Bridge** -- A mechanism where unresolved tool calls are forwarded to the frontend via SSE events, allowing the UI to handle tools that require user interaction.

**Context** -- The shared state (`WorkflowContext`) that accumulates data during workflow execution, accessible via `$ctx` variables.

**DAG** -- Directed Acyclic Graph. The underlying data structure of a workflow -- nodes connected by directed edges with no cycles.

**Devtools** -- Developer tools (`read_file`, `write_file`, `edit_file`, `glob`, `grep`, `bash`) available as builtins for agent use in code-editing scenarios.

**Edge** -- A directed connection between two nodes, defining execution order. Can be unconditional (`[A] -> [B]`), conditional (`[A] if $ctx.x -> [B]`), or error-handling (`[A] on error -> [B]`).

**Entry Node** -- The node(s) declared with `entry:` where workflow execution begins. Every workflow must have at least one.

**Exit Node** -- The node(s) declared with `exit:` where workflow execution ends. The output of exit nodes becomes the workflow's final result.

**Expression** -- A Python-like expression evaluated at runtime (e.g., `$output.count > 10`, `len($ctx.items)`). Supports arithmetic, comparison, logical operators, and 30+ built-in functions.

**Flow Import** -- Compile-time merging of another `.jg` file's nodes into the current workflow via `flows: { alias: "path.jg" }`. Imported nodes are namespaced with the alias prefix.

**Function Definition** -- A reusable parameterized block defined with `[name(params)]: { steps }`. Stored separately from the main DAG and invoked like a tool call.

**Jug0** -- The backend server that provides resource storage (push/pull), API key management, and remote execution capabilities.

**Lib Import** -- Importing function-only library files via `libs: ["./lib.jg"]`. Only function definitions are imported, not the main workflow graph.

**MCP** -- Model Context Protocol. A standard for connecting external tool servers. Juglans connects to MCP servers via HTTP/JSON-RPC.

**Metadata** -- The header section of a `.jg` file containing `name`, `version`, `author`, `description`, and resource import declarations.

**Node** -- The fundamental unit of a workflow. Each node has an ID (in brackets) and executes a single tool call: `[node_id]: tool(params)`.

**Prompt** -- A Jinja-style template defined in a `.jgprompt` file, rendered with the `p()` builtin. Supports variables, conditionals, and loops.

**Registry** -- The package registry (`jgr.juglans.ai`) where Juglans packages are published and installed via `juglans publish` / `juglans add`.

**Slug** -- A URL-safe identifier derived from a resource's filename (e.g., `my-agent` from `my-agent.jgagent`). Used to reference agents, prompts, and workflows.

**Switch** -- Multi-branch routing that executes exactly one matching path: `[node] -> switch $var { "a": [x], default: [y] }`.

**Tool** -- Any callable operation in a workflow node. Resolution order: builtin, function definition, Python module, MCP server, client bridge.

**Variable** -- A runtime reference to data, prefixed with `$`: `$input` (CLI input), `$output` (previous node result), `$ctx` (shared context), `$reply` (agent response metadata), `$env` (environment variables).

**Workflow** -- A complete execution graph defined in a `.jg` file, consisting of metadata, nodes, edges, and optional function definitions.
