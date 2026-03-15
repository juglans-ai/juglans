# Configuration File Reference

## File Location

Searched by priority (first found wins):

1. `./juglans.toml` -- project directory
2. `~/.config/juglans/juglans.toml` -- user configuration
3. `/etc/juglans/juglans.toml` -- system configuration

Override via environment variable:

```bash
JUGLANS_CONFIG=/path/to/juglans.toml juglans ...
```

## Complete Configuration Example

```toml
[account]
id = "user_123"
name = "John Doe"
role = "admin"
api_key = "jug0_sk_..."

[workspace]
id = "ws_default"
name = "My Workspace"
members = ["user_123", "user_789"]
agents = ["src/agents/**/*.jgagent"]
workflows = ["src/**/*.jg", "src/workflows/**/*.jgflow"]
prompts = ["src/prompts/**/*.jgprompt"]
tools = ["src/tools/**/*.json"]
exclude = ["**/*.backup", "**/test_*"]

[jug0]
base_url = "http://localhost:3000"

[server]
host = "127.0.0.1"
port = 3000
endpoint_url = "https://agent.juglans.ai"

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

[[mcp_servers]]
name = "filesystem"
base_url = "http://localhost:3001/mcp/filesystem"
alias = "fs"

[[mcp_servers]]
name = "github"
base_url = "http://localhost:3001/mcp/github"
token = "${GITHUB_TOKEN}"

[bot.telegram]
token = "bot_token_here"
agent = "default"

[bot.feishu]
app_id = "cli_xxx"
app_secret = "secret"
agent = "default"
port = 9000
base_url = "https://open.feishu.cn"

[registry]
url = "https://jgr.juglans.ai"
```

---

## [account]

User account credentials.

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `id` | string | Yes | | User ID |
| `name` | string | Yes | | Display name |
| `role` | string | No | | Role (e.g., `admin`, `user`) |
| `api_key` | string | No | | Jug0 API key (prefix `jug0_sk_`) |

`api_key` can be overridden by `JUGLANS_API_KEY` environment variable.

---

## [workspace]

Workspace for multi-user collaboration and batch resource management.

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `id` | string | Yes | | Workspace ID |
| `name` | string | Yes | | Workspace name |
| `members` | string[] | No | `[]` | Member user IDs |
| `agents` | string[] | No | `[]` | Agent file glob patterns |
| `workflows` | string[] | No | `[]` | Workflow file glob patterns |
| `prompts` | string[] | No | `[]` | Prompt file glob patterns |
| `tools` | string[] | No | `[]` | Tool file glob patterns |
| `exclude` | string[] | No | `[]` | Exclude patterns |

Resource paths support glob: `*` matches filenames, `**` matches directories recursively.

Used by `juglans push` (without arguments) for batch operations.

---

## [jug0]

Jug0 backend server connection.

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `base_url` | string | No | `https://api.jug0.com` | API base URL |

---

## [server]

Local web server configuration (for `juglans web`).

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `host` | string | No | `127.0.0.1` | Bind address |
| `port` | u16 | No | `3000` | Port number |
| `endpoint_url` | string | No | | Public endpoint URL for Jug0 registration |

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

When set to `"."`, `@/agents/foo.jgagent` resolves to `<project_root>/agents/foo.jgagent`.

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

[account]
api_key = "${JUG0_API_KEY}"

[jug0]
base_url = "${API_BASE}"
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

## [[mcp_servers]]

MCP (Model Context Protocol) server connections. Juglans connects via HTTP/JSON-RPC -- you must start the MCP server separately.

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `name` | string | Yes | | Server name (used in tool namespace) |
| `base_url` | string | Yes | | MCP server HTTP address |
| `alias` | string | No | | Short alias |
| `token` | string | No | | Authentication token |

Token values support `${ENV_VAR}` syntax for environment variable interpolation.

Multiple servers use TOML array-of-tables syntax:

```toml
[[mcp_servers]]
name = "filesystem"
base_url = "http://localhost:3001/mcp/filesystem"
alias = "fs"

[[mcp_servers]]
name = "github"
base_url = "http://localhost:3001/mcp/github"
token = "${GITHUB_TOKEN}"
```

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
| `mode` | string | (auto) | `"local"` or `"jug0"` |

---

## [registry]

Package registry configuration.

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `url` | string | `https://jgr.juglans.ai` | Registry URL |
| `port` | u16 | | Server port (when running registry locally) |
| `data_dir` | string | | Server data directory |

---

## Environment Variables

| Variable | Description | Overrides |
|----------|-------------|-----------|
| `JUGLANS_API_KEY` | API key | `account.api_key` |
| `JUGLANS_CONFIG` | Config file path | Search order |
| `JUGLANS_LOG_LEVEL` | Log level: `debug`, `info`, `warn`, `error` | `logging.level` |
| `JUGLANS_JUG0_URL` | Jug0 API address | `jug0.base_url` |

---

## Project vs User Configuration

**Project config** (`./juglans.toml`) -- committed to version control, no secrets:

```toml
[jug0]
base_url = "http://localhost:3000"

[server]
port = 8080

[[mcp_servers]]
name = "filesystem"
base_url = "http://localhost:3001/mcp/filesystem"
```

**User config** (`~/.config/juglans/juglans.toml`) -- personal settings and secrets:

```toml
[account]
id = "my_user_id"
name = "My Name"
api_key = "jug0_sk_secret"
```

Environment variables override both:

```bash
export JUGLANS_API_KEY="jug0_sk_..."
```
