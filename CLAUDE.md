# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

**Juglans** is a Rust-based AI workflow orchestration framework with a declarative DSL for defining and executing complex AI agent workflows. The CLI tool supports native (Linux/macOS/Windows) and WebAssembly targets.

## Core Architecture

### Two File Types, Two Parsers

```
.jg   → GraphParser      → Workflow DAG execution (includes inline agent map nodes)
.jgx → PromptParser     → Jinja-style template rendering
```

### Execution Flow

```
1. Parse file → AST
2. Resolve flow imports (merge subgraph nodes/edges with namespace prefixes)
3. Load imported resources (prompts via glob patterns)
4. Build WorkflowExecutor with:
   - PromptRegistry (from .jgx files)
   - LocalRuntime (LLM provider direct calls)
5. Execute DAG:
   - Traverse graph (petgraph)
   - Resolve variables (input, output, reply, etc.)
   - Call tools (builtin, Python, MCP)
   - Update WorkflowContext
```

### Key Components

**src/core/**
- `parser.rs` - Workflow DSL parser (Pest PEG grammar: `jwl.pest`)
- `executor.rs` - DAG execution engine with Pest-based expression evaluation (ExprEvaluator); `execute_mcp_tool()` for nested workflow tool calls
- `context.rs` - Workflow context with event system (`WorkflowEvent::ToolCall`, `emit_tool_call_and_wait()` for client tool bridge)
- `renderer.rs` - Template variable substitution
- `validator.rs` - Workflow validation (entry/exit nodes, edge consistency)
- `resolver.rs` - Flow import resolver: compile-time subgraph merging for `flows:` declarations
- `expr.pest` - Pest PEG grammar for expression language (Python-like syntax)
- `expr_ast.rs` - Expression AST definitions (Expr, BinOp, UnaryOp enums)
- `expr_eval.rs` - Tree-walking expression evaluator with 30+ built-in functions

**src/services/**
- `local_runtime.rs` - The juglans runtime: calls LLM providers directly via the providers layer. Hosts `LocalRuntime` (the only runtime), `ChatRequest`, `ChatOutput`, `ChatToolHandler`. juglans is local-first; there is no remote backend dependency.
- `mcp.rs` - Model Context Protocol client for tool integration
- `web_server.rs` - Local development server (Axum) with SSE streaming and client tool bridge (`/api/chat/tool-result`)
- `config.rs` - juglans.toml configuration loader
- `prompt_loader.rs` - Resource registry with glob loading

**src/builtins/**
- `ai.rs` - chat() (with `state` parameter, client tool bridge, terminal tool detection), p() (prompt render)
- `system.rs` - notify(), reply(), timer(), feishu_webhook() tools (set_context is internal-only, users write assignment syntax)
- `devtools.rs` - Developer tools: read_file(), write_file(), edit_file(), glob(), grep(), bash() (also aliased as "sh")
- `http.rs` - serve() (HTTP entry point marker), response() (HTTP response builder) for web server backend
- `network.rs` - fetch(), fetch_url() tools

**src/runtime/** (NEW)
- `python/mod.rs` - Python ecosystem runtime manager
- `python/worker.rs` - Python worker process pool
- `python/protocol.rs` - JSON-RPC protocol definitions

### Data Flow

```
CLI Input
   ↓
GraphParser::parse() → WorkflowGraph (DAG)
   ↓
WorkflowExecutor::run()
   ↓
For each node:
  1. Resolve parameters (input.field, user_var, etc.)
  2. execute_node()
     - Builtin tools (chat, p, notify, devtools)
     - Python tools
     - MCP tools (filesystem, web-browser)
     - Client bridge tools
  3. Update WorkflowContext
   ↓
Output final context
```

### Variable Resolution

Juglans uses bare identifiers for variable access (no `$` prefix needed):

- `input.field` - Input data passed via --input
- `output` - Last node's output
- `variable` - Custom context variables (set via assignment syntax: `variable = value`)
- `reply.output` / `reply.status` - Agent response metadata

Reserved words: `input`, `output`, `reply`, `error`, `config`.

Variables are resolved by `WorkflowContext::resolve_path()` using JSON pointer-style access. Legacy `$ctx.var` / `$input.field` syntax is still supported internally but not recommended.

## Common Development Commands

### Build and Test

```bash
# Build (native only, requires feature flag)
cargo build --release

# Run without installing
cargo run -- <file.jg>

# Run tests
cargo test

# Check without building
cargo check

# Format code
cargo fmt

# Lint
cargo clippy
```

### CLI Usage Patterns

```bash
# Execute workflow
juglans workflow.jg --input '{"query": "test"}'
juglans workflow.jg --input-file input.json

# Render prompt
juglans prompt.jgx --input '{"name": "Alice"}'

# Validate all files (like cargo check)
juglans check
juglans check ./workflows/ --all --format json

# Local dev server
juglans web --port 8080
```

### Configuration

**juglans.toml** must exist in project root or ancestor directory:

```toml
[account]
id = "user_id"
name = "Developer"
role = "admin"

[server]
host = "127.0.0.1"
port = 8080

# LLM providers (juglans is local-first — configure at least one provider here
# or set OPENAI_API_KEY / ANTHROPIC_API_KEY / DEEPSEEK_API_KEY / QWEN_API_KEY / etc. in env)
[ai.providers.openai]
api_key = "sk-..."

# Optional MCP servers
[mcp.filesystem]
command = "npx"
args = ["-y", "@anthropic/mcp-filesystem"]
env = { ROOT_DIR = "/workspace" }
```

The CLI searches for config in: `./juglans.toml` → `~/.config/juglans/juglans.toml` → `/etc/juglans/juglans.toml`

## Key Implementation Details

### Parser Architecture

- Uses **Pest** (PEG parser) for `.jg` and `.jgx` grammars
- Each file type has dedicated parser module and `.pest` grammar file
- Workflow parser builds DAG using **petgraph** library

### Workflow Execution

- DAG traversal in topological order
- **Flow imports**: `flows: { alias: "path" }` merges subworkflow nodes with namespace prefixes at compile time (`resolver.rs`). Variables referencing child nodes are auto-prefixed. Supports recursive imports and circular import detection.
- Conditional edges: `[node] if value == "x" -> [next]`
- Cross-workflow edges: `[node] if x -> [alias.child_node]` (target nodes from imported flows)
- Switch routing: `[node] -> switch output.type { "a": [x], default: [y] }` (exclusive branches)
- Error handling edges: `[node] on error -> [fallback]` (sets global `error` variable)
- Loop support: `foreach` and `while` blocks for iteration
- **Function definitions**: `[name(params)]: { steps }` — reusable parameterized node blocks stored in `workflow.functions`, not in main DAG. Executor binds args to context, runs body sub-graph, returns `output`.

### Expression Evaluation

- **Pest-based ExprEvaluator** (`expr.pest` grammar, `expr_ast.rs` AST, `expr_eval.rs` evaluator) replaces Rhai
- Python-like expression language: arithmetic, comparison, logical (`and`/`or`/`not`), membership (`in`/`not in`), pipe/filter chains
- Variable resolution via bare identifiers (`var.path.field`) with JSON pointer-style access; legacy `$var` syntax still supported
- 30+ built-in functions: `len`, `str`, `int`, `float`, `upper`, `lower`, `round`, `join`, `split`, `replace`, `contains`, `keys`, `values`, `default`, `range`, etc.
- Python-like truthiness: `false`, `0`, `""`, `[]`, `{}`, `null` are falsy

### Tool System

1. **Builtin tools** - Direct Rust implementations (chat, p, notify, fetch, reply, serve, response). Assignment syntax (`key = value`) compiles to internal `set_context` tool.
2. **Devtools** - Claude Code-style developer tools (read_file, write_file, edit_file, glob, grep, bash); auto-registered as `"devtools"` slug in ToolRegistry for LLM function calling
3. **Python tools** - Transparent Python ecosystem calls via subprocess worker
4. **MCP tools** - External processes via stdio/SSE (filesystem, web-browser)
5. **Client bridge tools** - Unresolved tools forwarded to frontend via SSE `tool_call` events; results returned via `/api/chat/tool-result`
6. **Custom tools** - Project-provided via the `tools:` glob in workflow metadata

Resolution order: Builtin (including devtools) → Python → MCP → Client Bridge (automatic fallback)

### Python Ecosystem Integration (NEW)

Juglans 2.0 supports direct Python module calls without wrapping:

```yaml
# Import Python modules
python: ["pandas", "sklearn.ensemble", "./utils.py"]

# Transparent function calls
[load]: pandas.read_csv("data.csv")
[stats]: load.describe()
[model]: sklearn.ensemble.RandomForestClassifier()
```

**Key files:**
- `src/runtime/python/mod.rs` - Python runtime manager
- `src/runtime/python/worker.rs` - Worker process lifecycle
- `src/runtime/python/protocol.rs` - JSON-RPC message types
- `src/workers/python_worker.py` - Python subprocess worker

**Object reference system:**
- Non-serializable Python objects (DataFrame, Model) stay in Python memory
- Juglans holds reference IDs (`ref:obj:12345`)
- Method calls routed via reference ID
- Automatic GC on workflow completion

### Switch Routing (NEW)

Multi-branch routing that executes only ONE matching branch:

```yaml
[classify] -> switch output.intent {
    "question": [answer]
    "task": [execute]
    default: [fallback]
}
```

Unlike conditional edges (`if`), switch ensures mutual exclusivity.

### Message State (`chat()` `state` parameter)

| State | Context | SSE | Description |
|-------|---------|-----|-------------|
| `context_visible` | ✅ | ✅ | Default |
| `context_hidden` | ✅ | ❌ | AI-only |
| `display_only` | ❌ | ✅ | User-only |
| `silent` | ❌ | ❌ | Neither |

### WASM Support

- Library compiled to `cdylib` for WASM target
- Native-only features guarded by `#![cfg(not(target_arch = "wasm32"))]`
- WASM dependencies: wasm-bindgen, serde-wasm-bindgen
- Native dependencies: tokio, reqwest, clap, axum

### Cross-Platform Build

- Uses **Cross.toml** for cross-compilation (Linux ARM64)
- rustls-tls instead of native-tls for better cross-platform support
- CI/CD via GitHub Actions (see `.github/workflows/`)

## Important Patterns

### Adding a New Builtin Tool

1. Add function to `src/builtins/<category>.rs`
2. Register in `BuiltinRegistry::new()` or module's registration function
3. Tool receives parameters as `HashMap<String, String>` and `WorkflowContext`
4. Return `Result<Option<Value>>`

### Adding a New Devtool

1. Add struct to `src/builtins/devtools.rs` implementing `Tool` trait
2. Implement `schema()` returning OpenAI function calling JSON format
3. Register in `BuiltinRegistry::new()` via `reg!(devtools::YourTool)`
4. Schema is auto-registered to ToolRegistry as "devtools" slug
5. Tool is available in both workflow nodes and LLM function calling

### Adding a CLI Subcommand

1. Add variant to `Commands` enum in `src/main.rs`
2. Implement handler function (e.g., `handle_<command>()`)
3. Add match arm in `main()` function

### Resource Loading

- Use glob patterns in workflow: `prompts: ["./prompts/*.jgx"]`
- Paths are relative to `.jg` file location
- Registry resolves and caches resources by slug
- Agents are defined as inline JSON map nodes: `[my_agent]: { "model": "gpt-4o", ... }`
- Reference agents by node ID: `chat(agent=my_agent, ...)`

### Error Handling

- Use `anyhow::Result<T>` for fallible operations
- Context via `.with_context(|| format!(...))`
- Validation errors return structured `ValidationResult` with error codes

## Testing Strategy

- Unit tests for parsers (valid/invalid syntax)
- Integration tests for executor (end-to-end workflow runs)
- Example files in `examples/` serve as smoke tests
- Use `juglans check examples/` to validate all examples

## Documentation Structure

```
docs/
├── getting-started/     # Installation, quickstart
├── guide/               # Concepts, syntax guides
├── reference/           # CLI, config, builtins
├── integrations/        # MCP, web server, Python
└── examples/            # Tutorial-style examples

examples/
├── prompts/
├── agents/
└── workflows/
```

When modifying DSL syntax, update both code and `docs/guide/` markdown files.
