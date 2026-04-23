# Configuration File Reference

## File Location

Searched by priority (first found wins):

1. `./juglans.toml` -- project directory (or any ancestor)
2. `~/.config/juglans/juglans.toml` -- user configuration
3. `/etc/juglans/juglans.toml` -- system configuration

## Complete Configuration Example

```toml
[account]
id = "user_123"
name = "John Doe"
role = "admin"

[workspace]
id = "ws_default"
name = "My Workspace"
members = ["user_123", "user_789"]
agents = []  # deprecated, agents are now inline in .jg files
workflows = ["src/**/*.jg", "src/workflows/**/*.jgflow"]
prompts = ["src/prompts/**/*.jgx"]
tools = ["src/tools/**/*.json"]
exclude = ["**/*.backup", "**/test_*"]

[server]
host = "127.0.0.1"
port = 3000
endpoint_url = "https://agent.juglans.ai"

# LLM providers — juglans is local-first; configure at least one provider
[ai.providers.openai]
api_key = "${OPENAI_API_KEY}"

[ai.providers.anthropic]
api_key = "${ANTHROPIC_API_KEY}"

[ai.providers.deepseek]
api_key = "${DEEPSEEK_API_KEY}"

[ai.providers.qwen]
api_key = "${QWEN_API_KEY}"

[ai]
default_model = "gpt-4o-mini"

[debug]
show_nodes = false
show_context = false
show_conditions = false
show_variables = false

[limits]
max_loop_iterations = 100
max_execution_depth = 10
http_timeout_secs = 120
python_workers = 1

[paths]
base = "."

[env]
DATABASE_URL = "postgresql://localhost/mydb"
CUSTOM_VAR = "value"

[bot.telegram]
token = "bot_token_here"
agent = "default"

[bot.feishu]
app_id = "cli_xxx"
app_secret = "secret"
agent = "default"
port = 9000
base_url = "https://open.feishu.cn"

[bot.wechat]
agent = "default"

[registry]
url = "https://jgr.juglans.ai"
```

---

## [account]

User account information. Identity slot — future juglans-issued agent IDs will live here.

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `id` | string | Yes | | User ID |
| `name` | string | Yes | | Display name |
| `role` | string | No | | Role (e.g., `admin`, `user`) |

---

## [workspace]

Workspace for multi-user collaboration and batch resource management.

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `id` | string | Yes | | Workspace ID |
| `name` | string | Yes | | Workspace name |
| `members` | string[] | No | `[]` | Member user IDs |
| `agents` | string[] | No | `[]` | _(deprecated — prefer inline agent map nodes in `.jg` files)_ |
| `workflows` | string[] | No | `[]` | Workflow file glob patterns |
| `prompts` | string[] | No | `[]` | Prompt file glob patterns |
| `tools` | string[] | No | `[]` | Tool file glob patterns |
| `exclude` | string[] | No | `[]` | Exclude patterns |

Resource paths support glob: `*` matches filenames, `**` matches directories recursively.

---

## [ai]

LLM provider configuration. juglans calls providers directly using their respective APIs — no remote backend involved. Configure at least one provider here, or set the corresponding `*_API_KEY` environment variable.

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `default_model` | string | No | | Default model used when an agent node does not specify one |

```toml
[ai]
default_model = "gpt-4o-mini"

[ai.providers.<name>]
api_key = "..."
base_url = "..."  # optional, for OpenAI-compatible endpoints
```

Supported provider names: `openai`, `anthropic`, `deepseek`, `gemini`, `qwen`, `byteplus`, `xai`.

You can also configure providers entirely via env vars without a `juglans.toml`:

| Env Var | Provider |
|---|---|
| `OPENAI_API_KEY` | openai |
| `ANTHROPIC_API_KEY` | anthropic |
| `DEEPSEEK_API_KEY` | deepseek |
| `GEMINI_API_KEY` | gemini |
| `QWEN_API_KEY` | qwen |
| `ARK_API_KEY` | byteplus (ByteDance Ark) |
| `ARK_API_BASE` | byteplus base URL override |
| `XAI_API_KEY` | xai |

---

## [server]

Local web server configuration (for `juglans web`).

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `host` | string | No | `127.0.0.1` | Bind address |
| `port` | u16 | No | `3000` | Port number |
| `endpoint_url` | string | No | | Public endpoint URL for this server |

---

## [debug]

Debug output control.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `show_nodes` | bool | `false` | Show node execution info |
| `show_context` | bool | `false` | Show context variables |
| `show_conditions` | bool | `false` | Show condition evaluation details |
| `show_variables` | bool | `false` | Show variable resolution process |

---

## [limits]

Runtime limits to prevent runaway execution.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `max_loop_iterations` | usize | `100` | Maximum loop iterations |
| `max_execution_depth` | usize | `10` | Maximum nested execution depth |
| `http_timeout_secs` | u64 | `120` | HTTP request timeout (seconds) |
| `python_workers` | usize | `1` | Python worker pool size |

---

## [paths]

Path alias configuration.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `base` | string | (disabled) | Base directory for `@` path alias, relative to project root |

When set to `"."`, `@/prompts/foo.jgx` resolves to `<project_root>/prompts/foo.jgx`.

---

## env_file

Specify `.env` files to load (pydantic-settings style). Files are loaded in order; later files override earlier ones. Default: `[".env"]`.

```toml
env_file = [".env", ".env.local", ".env.deploy"]
```

After loading, all variables are available via the `env()` expression function:

```juglans
[step]: api_key = env("API_KEY")
```

---

## `${VAR}` Interpolation

All string values in `juglans.toml` support `${VAR_NAME}` syntax. Variables are resolved from the process environment (including `.env` files loaded via `env_file`).

```toml
env_file = [".env"]

[ai.providers.openai]
api_key = "${OPENAI_API_KEY}"
```

If a variable is not set, it is replaced with an empty string.

---

## [env]

Custom environment variables available during workflow execution. Supports `${VAR}` interpolation.

```toml
[env]
DATABASE_URL = "postgresql://localhost/mydb"
CLIENT_ID = "${CLIENT_ID}"
CLIENT_SECRET = "${CLIENT_SECRET}"
```

Accessible in workflows via `config.env.DATABASE_URL` or directly via `env("DATABASE_URL")`.

---

## MCP Servers

MCP (Model Context Protocol) servers are **not** configured in `juglans.toml` — they are declared inline on a `chat()` call via the `mcp=` parameter, so each agent can opt into the exact set of tools it needs:

```juglans
[reply]: chat(
  agent = my_agent,
  message = input.text,
  mcp = {
    "filesystem": "http://localhost:3001/mcp/filesystem",
    "github": {
      "url": "http://localhost:3001/mcp/github",
      "token": env("GITHUB_TOKEN")
    }
  }
)
```

See [How to Use MCP Tools](../guide/use-mcp.md) for the full flow, including the `std/mcps.jg` helper library.

---

## [bot]

Bot adapter configuration for messaging platforms.

### [bot.telegram]

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `token` | string | | Telegram bot token |
| `agent` | string | `"default"` | Agent slug to use |

### [bot.feishu]

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `app_id` | string | | Feishu app ID (event subscription mode) |
| `app_secret` | string | | Feishu app secret |
| `webhook_url` | string | | Webhook URL (one-way push mode) |
| `agent` | string | `"default"` | Agent slug to use |
| `port` | u16 | `9000` | Webhook listener port |
| `base_url` | string | `https://open.feishu.cn` | API base (`https://open.larksuite.com` for Lark) |
| `approvers` | string[] | `[]` | Approver open_ids |

### [bot.wechat]

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `agent` | string | `"default"` | Agent slug to use |

---

## [history]

Conversation history storage for `chat()`. When enabled, each `chat()` node with a resolved `chat_id` automatically loads the tail of the thread before the LLM call and appends the user/assistant turn afterwards. `chat_id` is resolved in this priority order: explicit `chat_id=` parameter → `reply.chat_id` (chained within a run) → `input.chat_id` (adapter-injected, e.g. `telegram:12345:my_agent`).

Persistence honors the `state` parameter — `state="silent"` or `state="display_only"` skips storage.

```toml
[history]
enabled = true            # master switch
backend = "jsonl"         # jsonl | sqlite | memory | none
dir = ".juglans/history"  # jsonl: one file per chat_id
# path = ".juglans/history.db"  # sqlite path
max_messages = 20         # cap auto-loaded messages per call
max_tokens = 8000         # soft token budget (rough 4-char ≈ 1-token estimate)
retention_days = 30       # eligible-for-GC age (0 disables)
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `enabled` | bool | `true` | When false, `chat_id` still works as a field but nothing is stored or loaded |
| `backend` | string | `"jsonl"` | `jsonl`, `sqlite`, `memory`, `none` |
| `dir` | string | `.juglans/history` | Directory for the JSONL backend (one `.jsonl` per `chat_id`) |
| `path` | string | `.juglans/history.db` | DB file path for the SQLite backend |
| `max_messages` | uint | `20` | Hard upper bound on messages auto-loaded per `chat()` call |
| `max_tokens` | uint | `8000` | Soft token budget (estimate) |
| `retention_days` | uint | `30` | Days after which old messages are eligible for GC (0 = never) |

Environment overrides: `JUGLANS_HISTORY_BACKEND`, `JUGLANS_HISTORY_DIR`, `JUGLANS_HISTORY_PATH`, `JUGLANS_HISTORY_MAX_MESSAGES`, `JUGLANS_HISTORY_MAX_TOKENS`, `JUGLANS_HISTORY_ENABLED`.

Programmatic access from workflows is exposed via the [`history.*` builtins](./builtins.md#conversation-history-history).

---

## [registry]

Package registry configuration used by `juglans publish` / `juglans add`.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `url` | string | `https://jgr.juglans.ai` | Registry URL to publish to / fetch from |

To publish packages, set `JUGLANS_REGISTRY_API_KEY` (or `REGISTRY_API_KEY`) in your environment.

---

## Environment Variables

| Variable | Description |
|----------|-------------|
| `RUST_LOG` | Log level / module filter (e.g. `debug`, `juglans::runtime::python=debug`) |
| `OPENAI_API_KEY` / `ANTHROPIC_API_KEY` / `DEEPSEEK_API_KEY` / `GEMINI_API_KEY` / `QWEN_API_KEY` / `ARK_API_KEY` / `XAI_API_KEY` / `JUGLANS_API_KEY` | LLM provider API keys (alternative to `[ai.providers]`) |
| `OPENAI_API_BASE` / `ANTHROPIC_BASE_URL` / `ARK_API_BASE` / `JUGLANS_API_BASE` | Provider base URL overrides (local proxies, Ollama, Azure OpenAI, etc.) |
| `DEFAULT_LLM_PROVIDER` | Fallback provider when `chat(model="default")` is used and no `ai.default_model` is set (`openai` \| `anthropic` \| `byteplus` \| `qwen` \| ...) |
| `JUGLANS_HISTORY_BACKEND` / `JUGLANS_HISTORY_DIR` / `JUGLANS_HISTORY_PATH` / `JUGLANS_HISTORY_MAX_MESSAGES` / `JUGLANS_HISTORY_MAX_TOKENS` / `JUGLANS_HISTORY_ENABLED` | Override `[history]` section fields |
| `JUGLANS_REGISTRY_API_KEY` / `REGISTRY_API_KEY` | Package registry credential for `juglans publish` |
| `SERVER_HOST` / `SERVER_PORT` | Override `[server]` host/port |
| `TELEGRAM_BOT_TOKEN` / `FEISHU_APP_ID` / `FEISHU_APP_SECRET` | Bot adapter overrides — if `[bot.telegram]` / `[bot.feishu]` is absent from `juglans.toml`, setting these env vars creates those sections automatically (handy for serverless / container deployments) |

---

## Project vs User Configuration

**Project config** (`./juglans.toml`) -- committed to version control, no secrets:

```toml
[server]
port = 8080

[ai.providers.openai]
api_key = "${OPENAI_API_KEY}"
```

**User config** (`~/.config/juglans/juglans.toml`) -- personal settings:

```toml
[account]
id = "my_user_id"
name = "My Name"
role = "developer"
```

Or just set everything in `.env`:

```bash
OPENAI_API_KEY=sk-...
ANTHROPIC_API_KEY=sk-ant-...
```
