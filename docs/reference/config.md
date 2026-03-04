# Configuration File Reference

Juglans uses a `juglans.toml` file for configuration.

## File Location

Searched by priority:

1. `./juglans.toml` - Current directory (project configuration)
2. `~/.config/juglans/juglans.toml` - User configuration
3. `/etc/juglans/juglans.toml` - System configuration

Can also be specified via environment variable:

```bash
JUGLANS_CONFIG=/path/to/juglans.toml juglans ...
```

## Complete Configuration Example

```toml
# juglans.toml

# Account configuration
[account]
id = "user_123"
name = "John Doe"
role = "admin"
api_key = "jug0_sk_..."

# Workspace configuration (optional)
[workspace]
id = "workspace_456"
name = "My Workspace"
members = ["user_123", "user_789"]

# Resource paths (supports glob patterns)
agents = ["src/agents/**/*.jgagent", "src/pure-agents/**/*.jgagent"]
workflows = ["src/**/*.jg", "src/workflows/**/*.jgflow"]
prompts = ["src/prompts/**/*.jgprompt"]
tools = ["src/tools/**/*.json"]

# Exclude rules
exclude = ["**/*.backup", "**/.draft", "**/test_*"]

# Jug0 backend configuration
[jug0]
base_url = "http://localhost:3000"

# Web server configuration
[server]
host = "127.0.0.1"
port = 8080

# Environment variables (optional)
[env]
DATABASE_URL = "postgresql://localhost/mydb"
CUSTOM_VAR = "value"

# MCP server configuration (HTTP connection method)
[[mcp_servers]]
name = "filesystem"
base_url = "http://localhost:3001/mcp/filesystem"
alias = "fs"
token = "optional_token"

[[mcp_servers]]
name = "github"
base_url = "http://localhost:3001/mcp/github"
token = "${GITHUB_TOKEN}"
```

## Configuration Sections Explained

### [account] - Account Configuration

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `id` | string | Yes | User ID |
| `name` | string | Yes | User name |
| `role` | string | No | User role (e.g., admin, user) |
| `api_key` | string | No | API key |

```toml
[account]
id = "user_123"
name = "John Doe"
role = "admin"
api_key = "jug0_sk_abcdef123456"
```

**Environment variable override:**

```bash
export JUGLANS_API_KEY="jug0_sk_..."
```

---

### [workspace] - Workspace Configuration

Workspaces are used for multi-user collaboration and batch resource management.

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `id` | string | Yes | Workspace ID |
| `name` | string | Yes | Workspace name |
| `members` | array | No | Member user ID list |
| `agents` | array | No | Agent file path patterns |
| `workflows` | array | No | Workflow file path patterns |
| `prompts` | array | No | Prompt file path patterns |
| `tools` | array | No | Tool definition file path patterns |
| `exclude` | array | No | Exclude file path patterns |

```toml
[workspace]
id = "workspace_456"
name = "My Team Workspace"
members = ["user_123", "user_789", "user_456"]

# Resource path configuration (supports glob patterns)
agents = ["src/agents/**/*.jgagent", "src/pure-agents/**/*.jgagent"]
workflows = ["src/**/*.jg", "src/workflows/**/*.jgflow"]
prompts = ["src/prompts/**/*.jgprompt"]
tools = ["src/tools/**/*.json"]

# Exclude rules
exclude = [
  "**/*.backup",
  "**/.draft",
  "**/test_*",
  "**/private_*"
]
```

#### Resource Path Configuration

Resource paths support **glob patterns**, used for automatic file discovery during batch operations.

**Common patterns:**

- `**/*.jg` - Recursively match all .jg files
- `src/*.jg` - Match only .jg files in the src directory (non-recursive)
- `src/**/*.jgagent` - Recursively match all agents under src

**Use cases:**

```bash
# Batch apply using workspace configuration
juglans push                    # Apply all configured resources
juglans push --type workflow    # Apply only workflows
juglans push --dry-run          # Preview
```

**Exclude rules:**

Use the `exclude` field to ignore specific files:

```toml
[workspace]
exclude = [
  "**/*.backup",        # All backup files
  "**/.draft",          # Draft files
  "**/test_*",          # Test files
  "**/private_*",       # Private files
  "src/experimental/**" # Experimental directory
]
```

---

### [jug0] - Backend Configuration

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `base_url` | string | https://api.jug0.com | API address |

```toml
[jug0]
base_url = "https://api.jug0.com"
```

**Different environment configurations:**

```toml
# Development environment
[jug0]
base_url = "http://localhost:3000"

# Production environment
# [jug0]
# base_url = "https://api.jug0.com"
```

---

### [server] - Web Server

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `host` | string | 127.0.0.1 | Bind address |
| `port` | number | 3000 | Port number |

```toml
[server]
host = "0.0.0.0"
port = 8080
```

---

### [env] - Environment Variables

Custom environment variable dictionary, accessible in workflows.

```toml
[env]
DATABASE_URL = "postgresql://localhost/mydb"
API_ENDPOINT = "https://api.example.com"
CUSTOM_SETTING = "value"
```

These environment variables can be accessed during workflow execution via `$env.DATABASE_URL` and similar paths.

**Use cases:**
- Database connection strings
- API endpoint configuration
- Custom configuration items
- Development/production environment switching

---

### [logging] - Logging Configuration

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `level` | string | info | Log level |
| `format` | string | pretty | Output format |

**Log levels:**

- `error` - Errors only
- `warn` - Warnings and errors
- `info` - Info, warnings, and errors
- `debug` - Debug information
- `trace` - Detailed tracing

**Output formats:**

- `pretty` - Colorized readable format
- `json` - JSON format (suitable for log collection)
- `compact` - Compact single-line format

```toml
[logging]
level = "debug"
format = "json"
```

**Environment variable override:**

```bash
export JUGLANS_LOG_LEVEL=debug
```

---

### [[mcp_servers]] - MCP Servers

Configure Model Context Protocol servers to extend tool capabilities.

**Important:** Juglans uses HTTP/JSON-RPC to connect to MCP servers and does not support process launching. You need to start the MCP server first, then connect via HTTP.

#### Configuration Format

```toml
[[mcp_servers]]
name = "filesystem"
base_url = "http://localhost:3001/mcp/filesystem"
alias = "fs"
token = "optional_token"
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | Yes | Server name (used for tool naming) |
| `base_url` | string | Yes | MCP server HTTP address |
| `alias` | string | No | Alias |
| `token` | string | No | Authentication token |

#### Multiple MCP Servers

```toml
[[mcp_servers]]
name = "filesystem"
base_url = "http://localhost:3001/mcp/filesystem"
alias = "fs"

[[mcp_servers]]
name = "github"
base_url = "http://localhost:3001/mcp/github"
token = "${GITHUB_TOKEN}"

[[mcp_servers]]
name = "database"
base_url = "http://localhost:5000/mcp"
token = "db_mcp_key"
```

---

## Environment Variables

| Variable | Description |
|----------|-------------|
| `JUGLANS_API_KEY` | API key |
| `JUGLANS_CONFIG` | Config file path |
| `JUGLANS_LOG_LEVEL` | Log level |
| `JUGLANS_JUG0_URL` | Jug0 API address |

**Referencing environment variables in configuration:**

```toml
[mcp.github]
env = { GITHUB_TOKEN = "${GITHUB_TOKEN}" }
```

---

## Project Configuration vs User Configuration

### Project Configuration (./juglans.toml)

Project-specific settings, should be committed to version control (without sensitive information):

```toml
# Project configuration example
[jug0]
base_url = "http://localhost:3000"

[server]
port = 8080

[[mcp_servers]]
name = "filesystem"
base_url = "http://localhost:3001/mcp/filesystem"
```

### User Configuration (~/.config/juglans/juglans.toml)

Personal settings and sensitive information:

```toml
# User configuration example
[account]
id = "my_user_id"
api_key = "jug0_sk_my_secret_key"

[logging]
level = "debug"
```

---

## Configuration Validation

Check if configuration is valid:

```bash
juglans config --check
```

View active configuration:

```bash
juglans config --show
```

---

## Best Practices

### 1. Separate Sensitive Information

```toml
# juglans.toml (committed to git)
[jug0]
base_url = "http://localhost:3000"

# Use environment variables for sensitive information
# export JUGLANS_API_KEY="..."
```

### 2. Use .env Files

Create a `.env` file (add to .gitignore):

```bash
JUGLANS_API_KEY=jug0_sk_...
GITHUB_TOKEN=ghp_...
```

### 3. Environment-Specific Configuration

```toml
# juglans.dev.toml
[jug0]
base_url = "http://localhost:3000"

# juglans.prod.toml
[jug0]
base_url = "https://api.jug0.com"
```

Usage:

```bash
JUGLANS_CONFIG=juglans.prod.toml juglans ...
```
