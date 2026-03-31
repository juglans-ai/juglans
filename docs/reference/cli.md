# CLI Command Reference

## Usage

```bash
juglans [OPTIONS] <FILE> [ARGS]
juglans <COMMAND> [OPTIONS]
```

**Global Options:**

| Option | Description |
|--------|-------------|
| `--input <JSON>` | Input data (JSON format) |
| `--input-file <FILE>` | Read input from file |
| `--verbose`, `-v` | Verbose output |
| `--dry-run` | Parse only, do not execute |
| `--output <FILE>` | Write result to file |
| `--output-format <FMT>` | Output format: `text` (default), `json` |

## Command Summary

| Command | Description |
|---------|-------------|
| `juglans <file>` | Execute .jg / .jgx file |
| `juglans init` | Create a new project scaffold |
| `juglans install` | Fetch MCP tool schemas |
| `juglans check` | Validate file syntax |
| `juglans web` | Start local development web server |
| `juglans push` | Push resources to Jug0 server |
| `juglans pull` | Pull resources from Jug0 server |
| `juglans list` | List remote resources |
| `juglans delete` | Delete a remote resource |
| `juglans whoami` | Show account and config info |
| `juglans bot` | Start bot adapter (Telegram, Feishu) |
| `juglans chat` | Launch interactive chat TUI |
| `juglans test` | Run tests (test_* nodes in .jg files) |
| `juglans doctest` | Validate code snippets in markdown docs |
| `juglans pack` | Pack a package into .tar.gz |
| `juglans publish` | Publish a package to the registry |
| `juglans add` | Add a package dependency |
| `juglans remove` | Remove a package dependency |
| `juglans deploy` | Deploy project to Docker container |
| `juglans cron` | Run a workflow on a cron schedule |
| `juglans lsp` | Start Language Server Protocol server |
| `juglans skills` | Manage agent skills from GitHub |

---

## Execute File

Run a workflow or prompt file.

```bash
juglans <FILE> [OPTIONS]
```

File type is determined by extension:

| Extension | Behavior |
|-----------|----------|
| `.jg` | Execute workflow DAG |
| `.jgx` | Render prompt template |

**Examples:**

```bash
# Execute a workflow
juglans src/main.jg

# Pass JSON input
juglans src/main.jg --input '{"query": "Hello"}'

# Read input from file
juglans src/main.jg --input-file input.json

# Dry run (parse only)
juglans src/main.jg --dry-run

# JSON output for programmatic use
juglans src/main.jg --output-format json

# Render a prompt template
juglans src/prompts/greeting.jgx --input '{"name": "Alice"}'
```

---

## init

Create a new project scaffold.

```bash
juglans init <PROJECT_NAME>
```

**Example:**

```bash
juglans init my-project
```

Generated structure:

```
my-project/
├── juglans.toml
└── src/
    ├── example.jg
    ├── workflows/
    ├── agents/
    ├── prompts/
    └── tools/
```

---

## install

Fetch MCP tool schemas defined in `juglans.toml`.

```bash
juglans install
```

**Example:**

```bash
juglans install
```

---

## check

Validate syntax of `.jg` and `.jgx` files.

```bash
juglans check [PATH] [OPTIONS]
```

| Option | Default | Description |
|--------|---------|-------------|
| `PATH` | `.` | File or directory to check |
| `--all` | | Show warnings in addition to errors |
| `--format <FMT>` | `text` | Output format: `text`, `json` |

**Exit codes:** `0` = all valid, `1` = errors found.

**Examples:**

```bash
# Check all files in current directory
juglans check

# Check a specific file
juglans check workflow.jg

# Check a directory with warnings
juglans check ./src/ --all

# JSON output (for CI)
juglans check --format json
```

---

## web

Start local development web server with SSE streaming support.

```bash
juglans web [OPTIONS]
```

| Option | Default | Description |
|--------|---------|-------------|
| `--host <HOST>` | `127.0.0.1` | Bind address |
| `--port <PORT>` | `3000` | Port number |

**API Endpoints:**

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/agents` | GET | List agents |
| `/api/agents/:slug` | GET | Get agent |
| `/api/prompts` | GET | List prompts |
| `/api/prompts/:slug/render` | POST | Render prompt |
| `/api/workflows` | GET | List workflows |
| `/api/workflows/:slug/execute` | POST | Execute workflow |
| `/api/chat` | POST | Chat (SSE stream) |
| `/api/chat/tool-result` | POST | Return client tool result |

**Examples:**

```bash
# Default (localhost:3000)
juglans web

# Custom port
juglans web --port 8080

# Allow external access
juglans web --host 0.0.0.0 --port 8080
```

---

## push

Push resources to the Jug0 server.

```bash
juglans push [PATHS...] [OPTIONS]
```

| Option | Description |
|--------|-------------|
| `--force` | Overwrite existing resources |
| `--dry-run` | Preview without pushing |
| `--type <TYPE>`, `-t` | Filter: `workflow`, `agent`, `prompt`, `tool`, `all` |
| `--recursive`, `-r` | Recursively scan directories |
| `--endpoint <URL>` | Override workflow endpoint URL |

When called without paths, uses `[workspace]` glob patterns from `juglans.toml`.

**Examples:**

```bash
# Push a single file
juglans push src/main.jg

# Force overwrite
juglans push src/prompts/greeting.jgx --force

# Push all workspace resources
juglans push

# Preview what will be pushed
juglans push --dry-run

# Push only workflows
juglans push --type workflow

# Recursive directory push
juglans push src/ -r
```

---

## pull

Pull a resource from the Jug0 server.

```bash
juglans pull <SLUG> --type <TYPE> [OPTIONS]
```

| Option | Description |
|--------|-------------|
| `--type <TYPE>`, `-t` | Resource type: `prompt`, `agent`, `workflow` |
| `--output <DIR>`, `-o` | Output directory |

**Examples:**

```bash
juglans pull my-agent --type agent
juglans pull my-prompt -t prompt --output ./src/prompts/
```

---

## list

List resources on the Jug0 server.

```bash
juglans list [OPTIONS]
```

| Option | Description |
|--------|-------------|
| `--type <TYPE>`, `-t` | Filter: `prompt`, `agent`, `workflow` |

**Examples:**

```bash
juglans list
juglans list -t agent
```

---

## delete

Delete a resource from the Jug0 server.

```bash
juglans delete <SLUG> --type <TYPE>
```

| Option | Description |
|--------|-------------|
| `--type <TYPE>`, `-t` | Resource type: `prompt`, `agent`, `workflow` |

**Examples:**

```bash
juglans delete old-prompt --type prompt
juglans delete my-agent -t agent
```

---

## whoami

Show current account and configuration information.

```bash
juglans whoami [OPTIONS]
```

| Option | Description |
|--------|-------------|
| `--verbose`, `-v` | Show detailed info (MCP servers, resource paths) |
| `--check-connection` | Test connection to Jug0 server |

**Examples:**

```bash
juglans whoami
juglans whoami -v --check-connection
```

---

## bot

Start a bot adapter for messaging platforms.

```bash
juglans bot <PLATFORM> [OPTIONS]
```

| Option | Description |
|--------|-------------|
| `PLATFORM` | `telegram` or `feishu` |
| `--agent <SLUG>` | Agent slug (overrides config default) |
| `--port <PORT>` | Webhook port (Feishu) |

**Examples:**

```bash
juglans bot telegram
juglans bot feishu --agent trader --port 9000
```

---

## chat

Launch interactive chat TUI.

```bash
juglans chat [OPTIONS]
```

| Option | Description |
|--------|-------------|
| `--agent <SLUG>`, `-a` | Agent slug to use |

**Example:**

```bash
juglans chat --agent assistant
```

---

## test

Discover and execute `test_*` nodes in .jg files.

```bash
juglans test [PATH] [OPTIONS]
```

| Option | Default | Description |
|--------|---------|-------------|
| `PATH` | `./tests/` | File or directory to test |
| `--filter <NAME>` | | Filter tests by name substring |
| `--format <FMT>` | `text` | Output format: `text`, `json`, `junit` |

**Examples:**

```bash
juglans test
juglans test ./tests/ --filter auth
juglans test --format junit
```

---

## doctest

Validate ` ```juglans ` code blocks in markdown files through the parser.

```bash
juglans doctest [PATH] [OPTIONS]
```

| Option | Default | Description |
|--------|---------|-------------|
| `PATH` | `./docs/` | File or directory |
| `--format <FMT>` | `text` | Output format: `text`, `json` |

**Examples:**

```bash
juglans doctest
juglans doctest docs/guide/workflow-syntax.md
juglans doctest --format json
```

---

## pack

Pack a package directory into a `.tar.gz` archive.

```bash
juglans pack [PATH] [OPTIONS]
```

| Option | Description |
|--------|-------------|
| `PATH` | Package directory (default: `.`) |
| `--output <DIR>`, `-o` | Output directory |

**Example:**

```bash
juglans pack ./my-package -o ./dist
```

---

## publish

Publish a package to the registry.

```bash
juglans publish [PATH]
```

| Option | Description |
|--------|-------------|
| `PATH` | Package directory (default: `.`) |

**Example:**

```bash
juglans publish
```

---

## add / remove

Manage package dependencies.

```bash
juglans add <PACKAGE>
juglans remove <PACKAGE>
```

Package format: `name` or `name@version` (e.g., `sqlite-tools@^1.2.0`).

**Examples:**

```bash
juglans add sqlite-tools
juglans add sqlite-tools@^1.2.0
juglans remove sqlite-tools
```

---

## deploy

Deploy project to a Docker container.

```bash
juglans deploy [PATH] [OPTIONS]
```

| Option | Description |
|--------|-------------|
| `--tag <TAG>` | Custom image tag |
| `--port <PORT>`, `-p` | Host port (default: 8080) |
| `--build-only` | Build image without starting container |
| `--push` | Push image to registry after build |
| `--stop` | Stop and remove running container |
| `--status` | Show container status |
| `--env <KEY=VAL>`, `-e` | Environment variables (repeatable) |

**Examples:**

```bash
juglans deploy
juglans deploy --port 3000 -e API_KEY=xxx
juglans deploy --build-only --push
juglans deploy --stop
juglans deploy --status
```

---

## cron

Run a workflow on a cron schedule (local dev scheduler).

```bash
juglans cron <FILE> [OPTIONS]
```

| Option | Description |
|--------|-------------|
| `--schedule <EXPR>`, `-s` | Cron expression (overrides file metadata) |

**Example:**

```bash
juglans cron src/daily-report.jg -s "0 9 * * *"
```

---

## lsp

Start the Language Server Protocol server for editor integration.

```bash
juglans lsp
```

---

## Exit Codes

| Code | Description |
|------|-------------|
| 0 | Success |
| 1 | General error |
| 2 | Parse error |
| 3 | Execution error |
| 4 | Configuration error |
| 5 | Network error |
