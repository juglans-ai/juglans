# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.19] - 2026-04-26

### Fixed

- **`juglans <file.jg>` now initializes conversation history.** The most common code path — `juglans my-workflow.jg` from the CLI — previously skipped `history::init_global` because the file-exec branch (`main.rs::handle_file_logic`) constructs its executor inline without going through `runner::RunBuilder`. As a result, even when `[history]` was enabled and a `chat()` node had a resolved `chat_id`, no history was loaded or persisted; only bot adapters, `juglans serve`, and the embedded `runner` SDK were initializing the store. Adding the init call after `JuglansConfig::load()` brings the file-exec path in line with the other three. Discovered while testing the synple agent end-to-end with `deepseek/deepseek-v4-pro`.

## [0.2.18] - 2026-04-25

### Added

- **Discord bot adapter** (`src/adapters/discord.rs`, ~620 LoC). Gateway v10 WebSocket client that receives `MESSAGE_CREATE` events and dispatches them through the existing `run_agent_for_message` pipeline. `juglans bot discord` runs standalone; `juglans serve` auto-starts it when `[bot.discord]` is configured. Conversation history works out of the box — `input.chat_id = "discord:{channel_id}:{agent_slug}"` is injected automatically, isolating DMs and guild channels.
- **`[bot.discord]` config section** with token (via `${DISCORD_BOT_TOKEN}` interpolation), agent slug, and intent names (default: `guilds`, `guild_messages`, `message_content`, `direct_messages`). Optional `intents_bitmask` for copy-pasting from the Discord Developer Portal. `dm_policy` / `group_policy` / `guilds` allowlist fields parsed-but-not-yet-enforced (warned at startup).
- **Discord session resume.** Session id / resume URL / sequence persist at `.juglans/discord/gateway.json`; restarts resume from the stored state instead of re-identifying.
- **Platform messaging builtins** — push text from any workflow node (cron, error handlers, cross-channel alerts, etc.). Eleven new dotted tools:
  - `telegram.send_message`, `telegram.typing`, `telegram.edit_message`
  - `discord.send_message`, `discord.typing`, `discord.edit_message`, `discord.react`
  - `wechat.send_message`
  - `feishu.send_message`, `feishu.send_image`, `feishu.send_webhook`
- Every `*.send_message` auto-resolves its target from `input.platform_chat_id` (set by the adapter on inbound messages), so bot reply branches need zero arguments: `[reply]: telegram.send_message(text = "hi")`. Pass an explicit `chat_id` / `channel_id` / `user_id` for broadcast / cron use.
- WeChat tool reads its token + base URL from the QR-login session file (`.juglans/wechat/*.json`) — no additional config; run `juglans bot wechat` once to log in.
- **Dependencies:** `tokio-tungstenite = "0.24"` (rustls, connect) + `futures-util = "0.3"` for the Discord Gateway WebSocket. ~3 transitive crates added.

### Changed — BREAKING

- **`feishu_send` and `feishu_webhook` removed.** Migrate to the platform-namespaced equivalents:
  - `feishu_send(chat_id=..., message=...)` → `feishu.send_message(chat_id=..., text=...)` — note `message` became `text`
  - `feishu_send(chat_id=..., image=...)` → `feishu.send_image(chat_id=..., image=...)`
  - `feishu_send(chat_id=..., message=..., image=...)` → two separate calls
  - `feishu_webhook(message=..., webhook_url=...)` → `feishu.send_webhook(message=..., webhook_url=...)` (same params, renamed)
  - See [troubleshooting #19](docs/appendix/troubleshooting.md) for a worked migration table.

### Documentation

Full sweep across all docs sections to reflect the Discord adapter, platform messaging, and the conversation-history improvements that landed in 0.2.13–0.2.17:

- **Onboarding** (`README.md`, `docs/README.md`) — bot adapter list now lists Telegram / Discord / Feishu / WeChat (4 platforms); platform messaging pitched as a key feature; install version string future-proofed.
- **Tutorials** — `ai-chat.md` adds **6.7 Conversation History** and **6.8 Message State (`state=`)** sections; `branching.md` and `error-handling.md` corrected on edge semantics ("all matching `if` edges fire" — not "first match wins"); `full-project.md` adds **9.7 Deploying as a Bot** showing `juglans bot telegram` + explicit `telegram.send_message`.
- **Guide** — `concepts.md` enumerates 4 adapters and adds platform messaging to tool resolution order; `use-mcp.md` rewritten to lead with the native `chat(mcp={...})` parameter (legacy `std/mcps.jg` demoted to a Compatibility section); `build-web-api.md` documents `@get`/`@post`/`@put`/`@delete`/`@patch` decorator routing alongside `switch input.route`; `testing.md` adds `assert()` and `mock()` worked examples; port precedence clarified.
- **Reference** — `cli.md` clarifies `serve` auto-starts Telegram / WeChat / Discord (Feishu is webhook-only); `config.md` removes fictional `JUGLANS_API_KEY` / `JUGLANS_API_BASE` env-var rows.
- **Appendix** — `comparisons.md` matrix bot-adapters cell now lists 4 platforms; new "Cross-platform push from any node" row positions platform messaging as a Juglans-only differentiator vs LangGraph/CrewAI/n8n; `glossary.md` adds **Platform Messaging** entry; `troubleshooting.md` adds three new entries: **#19** `feishu_send` / `feishu_webhook` migration, **#20** Discord 4004 / 4014 close codes (most common: privileged `MESSAGE_CONTENT` intent not toggled in the dev portal), **#21** `<platform>.send_message` "no target" / WeChat session-not-found errors.

### Internal

- Extracted `pub(crate)` send helpers in `adapters/{telegram, discord, wechat}.rs` so platform messaging builtins and adapter reply loops share one code path.
- New `adapters::wechat::load_session` — reads `.juglans/wechat/{account}.json` and returns `{token, base_url, account_id, user_id}`.
- Validator `known_tools` cleanup: added `set_context`, `call`, `mock`; removed ghost entries `memory_search`, `history` (singular), and the unimplemented `vector_*` family that were causing false-positive W004 warnings on `juglans check`.

## [0.2.17] - 2026-04-24

### Changed — full documentation sweep

Large cleanup based on a cross-document review. No runtime behavior changed; every edit is in `docs/`.

**Factual fixes**
- `JUGLANS_LOG_LEVEL` → `RUST_LOG` (debugging, cli, config references)
- `BYTEPLUS_API_KEY` → `ARK_API_KEY` (plus `ARK_API_BASE` now listed)
- Install URL standardized on `raw.githubusercontent.com/juglans-ai/juglans/main/install.sh`; Homebrew tap and Windows `.ps1` marked as not-yet-shipped
- Dockerfile `COPY` paths in `deploy-docker.md` now match the actual Dockerfile (`/usr/local/bin/juglans`, `/usr/local/bin/workers/`)
- Stale version strings removed
- Two workflow snippets that would have failed to parse (wrong edge-condition direction in `tutorials/loops.md`; mixed assignment + tool call via comma in `tutorials/full-project.md`) are rewritten

**Removed fictional sections**
- `workflow-spec.md` metadata table — the parser rejects `slug`/`name`/`version`/`entry`/`exit`/`description`/`author`/`source`/`is_public`/`schedule` at the top of `.jg` files; only `libs`, `flows`, `prompts`, `tools`, `python` are valid. Package-level fields belong in `.jgflow` manifests.
- `prompt-spec.md` "Available Filters" and duplicated "Built-in Functions" tables — pipe filters are just the expression-function catalog applied left-to-right, now documented as such with a pointer to `expressions.md`
- `config.md` `[[mcp_servers]]` section — MCP servers are declared inline on `chat(mcp={...})`, not in `juglans.toml`. Example now shows the real shape.
- Deprecated terms purged: `$var` prefix in `expressions.md` examples, `.jgagent` file-extension references in `agent-spec.md` / `glossary.md`, fictional model IDs (`gpt-5.4`, `claude-sonnet-4-6`, `gemini-3.1-pro`) in `ai-chat.md`

**New coverage for 0.2.13–0.2.16 features**
- **Conversation history** — added sections to `guide/concepts.md`, `guide/connect-ai.md` (new "Conversation History" block covering `chat_id` resolution, backends, config), `reference/builtins.md` (`chat()` table now lists `chat_id`, `history`, `input`, `mcp`, `on_token`, `on_result`, `tool_event` with the 4-tier resolution rule), `appendix/glossary.md` (three new entries), `appendix/troubleshooting.md` (three new diagnostic entries)
- **`juglans/` proxy provider + `claude-code` provider** — documented in `connect-ai.md`, `config.md`, `glossary.md`
- **`assert()` builtin** — documented in `reference/builtins.md` testing section
- **Decorators, `yield` node, switch `ok:/err:` cases, explicit function `return`, `@/` path alias** — added to `reference/workflow-spec.md`

**CLI completeness**
- `juglans skills` full section (`add` / `list` / `remove` with all flags)
- `juglans bot` documents `lark` / `weixin` aliases
- `juglans chat` argument is a file path, not a slug (`--agent <FILE>`)
- `juglans cron` clarifies `--schedule` requirement for `.jg` vs `.jgflow`
- `juglans deploy` documents the `PATH` positional

**Polish**
- `troubleshooting.md` renumbered (was missing #4, had duplicate #8 and #15)
- `appendix/comparisons.md` rewritten — matrix now covers LangGraph, CrewAI, LangChain, n8n / Dify, Temporal, Airflow (dropped Terraform; downgraded the "no Python env" and "WASM everywhere" claims to accurate language)

## [0.2.15] - 2026-04-22

### Fixed

- **CI build commands updated for the new `cli` feature.** The Linux ARM64 cross build in `release.yml`, the docker build in `deploy-docker.yml`, and the docs build in `docs.yml` all pass `--no-default-features --features native` to skip device/X11 deps. After 0.2.14 introduced the `cli` feature (required by the `juglans` bin), these commands produced a lib-only build with no binary, causing the ARM64 release upload to fail with `chmod: cannot access 'juglans'`. All three now pass `--features native,cli`. Library content is identical to 0.2.14 on crates.io — this release restores the full binary matrix on GitHub Releases.

## [0.2.14] - 2026-04-22

### Fixed

- **Library consumers can now build from crates.io.** Moved the two `include_dir!` statics in `src/templates.rs` (`PROJECT_TEMPLATE_DIR`, `DOCS_DIR`) behind a new `cli` Cargo feature. `Cargo.toml` excludes `examples/` and `docs/` from the published tarball to keep the crate small, but `include_dir!` runs at compile time and fails on missing directories — so library consumers with `default-features = false, features = ["native"]` could not compile 0.2.13 after downloading from crates.io. The `juglans` binary requires `["native", "cli"]`; default features are `["native", "device", "cli"]` so `cargo build`/`cargo install juglans` still works end-to-end.

## [0.2.13] - 2026-04-22

### Added

- **Conversation history** — new `[history]` config section with JSONL / SQLite / in-memory backends. When enabled, each `chat()` node with a resolved `chat_id` automatically loads the tail of the thread before the LLM call and appends the turn afterwards. Persistence honors the `state` parameter (`silent` / `display_only` skip storage). `chat_id` is resolved in priority: explicit param → `reply.chat_id` (chained within a run) → `input.chat_id` (adapter-injected, e.g. `telegram:12345:agent_slug`) → stateless.
- **`history.*` builtins** — DSL-callable primitives for inspecting and manipulating the store: `history.load`, `history.append`, `history.replace`, `history.trim`, `history.clear`, `history.stats`, `history.list_chats`.
- **Adapter `chat_id` injection** — telegram / feishu / wechat adapters now set `input.chat_id = "{platform}:{user_id}:{agent_slug}"` automatically, so bot workflows get multi-turn memory with no DSL changes.
- **`juglans` provider** — new LLM provider that routes through the juglans-wallet platform proxy (base URL configurable via `JUGLANS_API_BASE`). Useful when agents should run without holding LLM credentials directly.

### Notes

- History storage is enabled by default with the JSONL backend. Existing workflows that called `chat()` without a `chat_id` remain stateless; bot workflows automatically gain memory once they pick up this version. Override or disable with `JUGLANS_HISTORY_ENABLED=false` or `[history].enabled = false`.
- Auto-compaction (summary / rolling / etc.) is deliberately out of scope for this release — the storage primitives are in place so users and future work can layer strategies on top.

## [0.2.12] - 2026-04-15

### Added

- **WeChat bot adapter** — `src/adapters/wechat.rs` plus `[bot.wechat]` config section. `juglans bot wechat` now joins `telegram` and `feishu` as a supported platform.
- **Unified `juglans serve`** — single subcommand that boots the web API and every configured bot adapter from one process (port/host/entry flags; entry defaults to `main.jg`).
- **Decorator macro system** — internal DSL for declaring builtin tools with less boilerplate.

## [0.2.11] - 2026-04-06

### Changed

- **`jug0` moved into this repository.** The `jug0` crate now lives alongside `juglans/` in the monorepo, but the juglans engine no longer has a compile-time or runtime dependency on it. juglans calls LLM providers (OpenAI, Anthropic, DeepSeek, Gemini, Qwen, xAI, ByteDance Ark) directly via `[ai.providers]` in `juglans.toml` or env vars.
- Collapsed the `JuglansRuntime` trait into the concrete `LocalRuntime` struct — `LocalRuntime` is the only runtime.
- `[account]` config section: the `api_key` field has been removed (it was a `jug0_sk_*` token that no longer has a use). The `id`/`name`/`role` fields are kept as the slot for future juglans-issued agent identity.
- CI `cargo audit` step switched to non-blocking while upstream advisories are triaged.

### Removed

- `services/jug0.rs` (Jug0Client HTTP backend, ~1100 lines)
- `services/interface.rs` (the `JuglansRuntime` trait)
- `[jug0]` config section and `JUG0_BASE_URL` / `JUG0_API_KEY` env var overrides
- `juglans push` / `pull` / `list` / `delete` resource sync commands
- `@handle` remote agent invocation
- Thin jug0 wrapper builtins: `memory_search`, `web_search`, `history`, and 6 `vector_*` tools
- `juglans whoami --check-connection` flag

### Migration

- **Chat / workflow execution**: set an LLM provider API key in env or `juglans.toml`, no other changes
- **Vector memory / RAG**: not yet supported in local mode; pin to v0.2.10 or wait for the upcoming local-store release
- **`juglans publish`**: now reads `JUGLANS_REGISTRY_API_KEY` (or `REGISTRY_API_KEY`) instead of `account.api_key`

## [0.2.10] - 2026-03-28

### Added

- **HTTP client** — `http_request` builtin plus `stdlib/http.jg` wrapper exposing `http.get/post/put/patch/delete/head/options` with full options (headers, params, json, data, files, auth, timeout, cookies, follow_redirects).
- **OAuth2 token helper** — `oauth_token` builtin for acquiring access tokens.
- **AI tool utilities** — helpers that let LLM tool-calling flows hand results back into the workflow cleanly.
- **Expression function bridge** — workflow-defined functions are callable directly from expressions, not just as nodes.

### Changed

- Feature-gated the device-control builtins under the `device` cargo feature so headless ARM64 cross-compiles succeed.

## [0.2.9] - 2026-03-20

### Added

- **Embedded stdlib** — `stdlib/*.jg` is compiled into the binary and resolvable without a checkout.
- **`serve()` HTTP backend** — workflows can declare `serve()` as the entry node and run as an axum HTTP handler (fallback route; any URL hits the workflow).
- **Auth token forwarding** — HTTP request headers are threaded through to nested workflow calls.

## [0.2.8] - 2026-03-12

### Added

#### WASM Full Engine
- **Full WASM execution engine** — `JuglansEngine` upgraded from a stub to a fully functional engine capable of parsing and executing workflows with tool bridge support
- Support running .jg workflows directly in the browser
- Tool handler callbacks support JS function injection

#### Type System & Class Definitions
- **Type system** — `type_checker.rs`, `types.rs` for type inference and checking
- **Instance management** — `instance_arena.rs` for object instance lifecycle management
- **Manifest parser** — `manifest_parser.rs` for project manifest parsing

#### Serverless Webhook Handlers
- **Telegram webhook handler** — `TelegramWebhookHandler` embedded in `juglans web` at the `/webhook/telegram` route
- **Feishu webhook handler** — `FeishuWebhookHandler` embedded at the `/webhook/feishu` route
- Both handlers support local / jug0 dual-mode execution
- `NoopToolExecutor` — General-purpose no-op tool executor
- Update/event deduplication

#### Web Server
- **`/health` route** — Health check endpoint, returns `{"status": "ok"}`
- **Environment variable overrides** — `JUG0_BASE_URL`, `JUG0_API_KEY`, `SERVER_HOST`, `SERVER_PORT`, `FEISHU_APP_ID`, `FEISHU_APP_SECRET`, `TELEGRAM_BOT_TOKEN`

### Changed

#### Parser Migration
- **Removed Pest grammars** — `jwl.pest`, `expr.pest` fully removed; switched to hand-written recursive descent parser (RDP)
- **parser.rs slimmed down** — Removed 1200+ lines of legacy Pest code
- **expr_parser.rs** — Added hand-written Pratt precedence expression parser

#### Core Refactoring
- **context.rs** — Replaced `std::sync::Mutex/RwLock` with `parking_lot` for WASM target compatibility
- **executor.rs** — Executor refactored with improved tool call chain
- **Removed MCP module** — `services/mcp.rs` removed (168 lines); MCP functionality consolidated into other modules

#### Config
- `TelegramBotConfig` added `mode` field (`"local"` / `"jug0"`)

### Documentation
- Major rewrite of tutorials and reference documentation
- Example files updated

## [0.2.7] - 2026-03-09

### Added
- **Rust-style error handling** — `return err`, `switch ok/err`, `??` operator, `is_err()`/`is_ok()` functions
- **RDP parser JSON object literals** — Support writing `{"key": value}` directly in expressions

### Fixed
- Eliminated all clippy warnings (strict `-D warnings` mode)

## [0.2.6] - 2026-03-01

### Added
- **Class system** — Class definitions, instantiation, and method calls
- **Database builtins** — Built-in tools for database operations
- **Node events** — Node execution event system
- **TUI improvements** — Terminal UI enhancements

## [0.2.5] - 2026-02-25

### Added
- **`print()` builtin** — Direct output tool
- **Removed mandatory juglans.toml requirement** — Can now run without a config file
- **README rewrite**

## [0.2.4] - 2026-02-20

### Added
- **Expression engine** — Hand-written Pratt precedence parser replacing Pest
- **Endpoint override** — API endpoint override support
- **CI cleanup** — Build pipeline optimization

## [0.2.3] - 2026-02-15

### Added
- **Flow imports** — Cross-workflow graph composition (`flows: { alias: "path.jg" }`)
- Compile-time subgraph merging with namespace prefixes
- Support for recursive imports and circular import detection
- Cross-workflow conditional edges: `[node] if $ctx.x -> [alias.child_node]`

## [0.2.2] - 2026-02-10

### Added

#### Claude Code-style Developer Tools
- **6 built-in developer tools** auto-registered as `"devtools"` slug in ToolRegistry
  - `read_file` - Read files with line numbers, offset/limit support
  - `write_file` - Write/overwrite files, auto-create parent directories
  - `edit_file` - Exact string replacement with uniqueness check
  - `glob` - File pattern matching (e.g., `**/*.rs`)
  - `grep` - Regex content search with context lines
  - `bash` - Shell execution with timeout, replaces old `sh()` tool

- **Tool trait `schema()` method** - Builtin tools can expose OpenAI function calling schemas for LLM discovery
- **`BuiltinRegistry.list_schemas()`** - Collect all builtin tool schemas
- **`BuiltinRegistry.register_devtools_to_registry()`** - Auto-register devtools to ToolRegistry at executor init
- **Builtin fallback in `chat()` tool** - When ToolRegistry lookup fails, falls back to builtin schemas for "devtools" slug

### Changed
- `bash` tool replaces `sh` (old syntax `sh(cmd="...")` still works via alias)
- Tool resolution now includes devtools schemas in ToolRegistry

### Technical Details

**New Files:**
- `src/builtins/devtools.rs` - 6 developer tools with OpenAI function calling schemas

**Modified Files:**
- `src/builtins/mod.rs` - Tool trait `schema()`, `list_schemas()`, `register_devtools_to_registry()`, tool registration
- `src/builtins/system.rs` - Removed old Shell struct
- `src/builtins/ai.rs` - Builtin fallback for slug resolution
- `src/core/executor.rs` - Auto-register devtools schema to ToolRegistry at init

## [0.1.5] - 2026-02-01

### Added

#### SSE Client Tool Bridge
- **Client-side tool execution** - Tools not matched by builtin or MCP are automatically forwarded to the frontend via SSE for execution
  - `WorkflowEvent::ToolCall` event in `context.rs` with oneshot channel for async response
  - `emit_tool_call_and_wait()` method blocks workflow until frontend returns tool results
  - `/api/chat/tool-result` endpoint on web server receives results from frontend
  - `pending_tool_calls` map in WebState manages in-flight tool calls
  - Tool resolution chain: Builtin → MCP → Client Bridge (automatic fallback)

- **Terminal tool detection** - Tools returning `{ executed_on_client: true }` automatically end the LLM loop
  - Prevents duplicate tool calls for terminal actions (e.g., `create_trade_suggestion`)
  - Functional tools (e.g., `get_market_data`) continue the LLM loop normally

#### Message State Control
- **`state` parameter for `chat()` tool** - Fine-grained control over message visibility and persistence

  | state | Context | SSE Output | Description |
  |-------|---------|------------|-------------|
  | `context_visible` | ✅ | ✅ | Default, normal message |
  | `context_hidden` | ✅ | ❌ | AI sees it, user doesn't |
  | `display_only` | ❌ | ✅ | User sees it, AI doesn't |
  | `silent` | ❌ | ❌ | Neither |

  - Backward compatible: `stateless="true"` maps to `state="silent"`
  - Controls `token_sender` (SSE streaming) and `reply.output` (context persistence)

#### Nested Workflow Execution
- `execute_mcp_tool()` exposed as public method on `WorkflowExecutor`
- Enables agents to trigger workflow execution within their tool call loops

### Changed

- Enhanced `WorkflowContext` with event emission system (`emit()`, `subscribe()`)
- Web server SSE handler now processes `ToolCall` events alongside content/notify events
- `chat()` builtin tool call loop handles both server-side (MCP/builtin) and client-side tool execution

### Technical Details

**New/Modified Files:**
- `src/core/context.rs` - `ToolResultPayload`, `WorkflowEvent::ToolCall`, `emit_tool_call_and_wait()`
- `src/builtins/ai.rs` - `state` parameter, terminal tool detection, client bridge fallback
- `src/services/web_server.rs` - `pending_tool_calls`, `handle_tool_result`, SSE ToolCall handling
- `src/core/executor.rs` - Public `execute_mcp_tool()` method

## [0.1.4] - 2026-01-31

### Added

#### Tool Definition Files
- **Tool definition file support** - Store and reuse tool configurations in JSON files
  - Create `tools/*.json` files with OpenAI Function Calling format
  - Import tools in workflows: `tools: ["./tools/*.json"]`
  - Three reference methods:
    - Inline JSON: `tools: [{...}]` (backward compatible)
    - Single reference: `tools: "web-tools"`
    - Multiple references: `tools: ["web-tools", "data-tools"]`
  - Agent default tools: Configure in `.jgagent` files
  - Automatic tool merging and deduplication
  - Priority: Workflow inline > Workflow reference > Agent default

#### Core Infrastructure
- `ToolResource` data structure for tool definitions
- `ToolLoader` for loading tools from JSON files with glob support
- `ToolRegistry` for tool registration, lookup, and merging
- Workflow parser support for `tools:` field in metadata
- Runtime tool reference resolution in `chat()` builtin

#### Examples & Documentation
- Example tool files: `web-tools.json`, `data-tools.json`
- Complete tool usage example workflow
- Agent with default tools example
- Comprehensive tools guide: `docs/guide/tools.md`
  - File format specification
  - Usage patterns and best practices
  - Error handling and debugging

### Fixed

#### MCP Documentation Corrections
- **MCP configuration format** - Fixed documentation to match actual implementation
  - Corrected to HTTP/JSON-RPC connection model (not process spawning)
  - Updated config format to `[[mcp_servers]]` with `base_url`
  - Removed incorrect `command`, `args`, `env` examples
  - Added proper HTTP server setup instructions

- **MCP tool naming** - Fixed tool invocation format
  - Corrected from `mcp_namespace_tool` to `namespace.tool`
  - Updated all examples to use dot notation
  - Clarified namespace resolution (alias > name)

### Changed

- Updated workflow execution to load tools from patterns
- Enhanced AI builtin to resolve tool references at runtime
- Improved error messages for missing tool resources

### Technical Details

**New Files:**
- `src/core/tool_loader.rs` - Tool file loading and validation
- `src/services/tool_registry.rs` - Tool registration and merging
- `examples/tools/*.json` - Example tool definitions
- `docs/guide/tools.md` - Complete documentation

**Modified Files:**
- `src/core/agent.pest` - Added `list` support for tools field
- `src/core/agent_parser.rs` - Parse three tool reference formats
- `src/core/jwl.pest` - Added `tools` to workflow metadata
- `src/core/parser.rs` - Parse tool patterns in workflows
- `src/core/graph.rs` - Added `tool_patterns` field
- `src/core/executor.rs` - Load and provide tool registry
- `src/builtins/ai.rs` - Resolve tool references at runtime
- `src/builtins/mod.rs` - Expose executor for tool access

**Tests:**
- 6 new unit tests for tool loading, registry, and deduplication
- All tests passing

**Lines of Code:**
- 1000+ lines added across 8 new files
- Complete test coverage for core functionality

## [0.1.3] - 2026-01-30

### Added
- Conditional branch OR semantics with unreachable node detection
- Context save/restore for nested workflow execution
- Tools configuration in agent definitions (JSON array support)

### Fixed
- Workflow deadlock on conditional branch convergence
- Context pollution in nested workflows
- Agent tools priority (workflow > agent default)

## [0.1.2] - 2026-01-29

### Added
- Agent workflow association
- Stateless execution mode
- Multi-turn conversation support

## [0.1.1] - 2026-01-28

### Added
- Initial workflow engine
- Agent and prompt management
- Basic builtins (chat, notify, etc.)

[Unreleased]: https://github.com/juglans-ai/juglans/compare/v0.2.12...HEAD
[0.2.12]: https://github.com/juglans-ai/juglans/compare/v0.2.11...v0.2.12
[0.2.11]: https://github.com/juglans-ai/juglans/compare/v0.2.10...v0.2.11
[0.2.10]: https://github.com/juglans-ai/juglans/compare/v0.2.9...v0.2.10
[0.2.9]: https://github.com/juglans-ai/juglans/compare/v0.2.8...v0.2.9
[0.2.8]: https://github.com/juglans-ai/juglans/compare/v0.2.7...v0.2.8
[0.2.7]: https://github.com/juglans-ai/juglans/compare/v0.2.6...v0.2.7
[0.2.6]: https://github.com/juglans-ai/juglans/compare/v0.2.5...v0.2.6
[0.2.5]: https://github.com/juglans-ai/juglans/compare/v0.2.4...v0.2.5
[0.2.4]: https://github.com/juglans-ai/juglans/compare/v0.2.3...v0.2.4
[0.2.3]: https://github.com/juglans-ai/juglans/compare/v0.2.2...v0.2.3
[0.2.2]: https://github.com/juglans-ai/juglans/compare/v0.2.1...v0.2.2
[0.1.5]: https://github.com/juglans-ai/juglans/compare/v0.1.4...v0.1.5
[0.1.4]: https://github.com/juglans-ai/juglans/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/juglans-ai/juglans/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/juglans-ai/juglans/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/juglans-ai/juglans/releases/tag/v0.1.1
