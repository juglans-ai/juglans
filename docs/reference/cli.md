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
| `--output-format <FMT>` | Output format: `text` (default), `json`, `sse` |
| `--chat-id <ID>` | Chat session ID for multi-turn conversation |
| `--info` | Show prompt info without executing |

## Command Summary

| Command | Description |
|---------|-------------|
| `juglans <file>` | Execute .jg / .jgx file |
| `juglans init` | Create a new project scaffold |
| `juglans install` | Install package dependencies from jgpackage.toml |
| `juglans check` | Validate file syntax |
| `juglans web` | Start local development web server |
| `juglans whoami` | Show account and config info |
| `juglans serve` | Start unified server (web API + every configured channel) |
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

Install package dependencies from `jgpackage.toml`.

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

## whoami

Show current account and configuration information.

```bash
juglans whoami [OPTIONS]
```

| Option | Description |
|--------|-------------|
| `--verbose`, `-v` | Show detailed info (resource paths, server config) |

**Examples:**

```bash
juglans whoami
juglans whoami -v
```

---

## serve

The single entry point that runs the HTTP API plus every channel configured under `[channels.*]` — Telegram bots (polling or webhook), WeChat accounts (auto-discovered), Discord gateways, Feishu event subscriptions, and Feishu incoming webhooks — in one process. Active channels (long-poll, websocket) get a tokio task each; passive channels (webhooks) mount their routes on the shared axum router.

```bash
juglans serve [OPTIONS]
```

| Option | Default | Description |
|--------|---------|-------------|
| `--port <PORT>`, `-p` | `3000` | Port for the web server |
| `--host <HOST>` | `127.0.0.1` | Host address to bind |
| `--entry <FILE>` | `main.jg` | Workflow entry file (defaults to `main.jg` in the project root) |

**Examples:**

```bash
juglans serve
juglans serve --port 8080 --host 0.0.0.0
juglans serve --entry src/api.jg
```

A single channel failure (token expired, network down) is logged and that task exits — other channels keep running. Per-channel `agent` lets different bots route to different workflows; the orchestrator caches one dispatcher per unique agent slug. See [`[channels.*]`](./config.md#channels) for instance-level config.

---

## bot (removed)

`juglans bot <platform>` has been removed. Every channel runs through `juglans serve` now. To migrate:

1. Move `[bot.telegram]` / `[bot.feishu]` / `[bot.wechat]` / `[bot.discord]` config to `[channels.<kind>.<id>]` form (see [config docs](./config.md#channels)).
2. Replace `juglans bot <platform>` invocations with `juglans serve`.

The command exits with an error message pointing here.

---

## chat

Launch interactive chat TUI.

```bash
juglans chat [OPTIONS]
```

| Option | Description |
|--------|-------------|
| `--agent <FILE>`, `-a` | Path to an agent file (e.g. `./agents/assistant.jg`) |

**Example:**

```bash
juglans chat --agent ./agents/assistant.jg
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
juglans doctest docs/reference/workflow-spec.md
juglans doctest --format json
```

---

## skills

Manage Agent Skills — packaged capabilities (workflows + prompts + tools) fetched from a GitHub repository and converted to `.jgx` templates.

```bash
juglans skills <SUBCOMMAND>
```

### skills add

Fetch one or more skills from a GitHub repo and save them as `.jgx` files under `./prompts/` (or a custom output directory).

```bash
juglans skills add <REPO> [OPTIONS]
```

| Option | Description |
|--------|-------------|
| `REPO` | GitHub repo in `owner/repo` form, e.g. `anthropics/skills` |
| `--skill <NAME>` | Specific skill to fetch (repeatable) |
| `--all` | Fetch every skill discovered in the repo |
| `--list` | Only list available skill names, don't download |
| `--output <DIR>`, `-o` | Output directory (default: `./prompts`) |

### skills list / skills remove

```bash
juglans skills list                    # list locally-installed skills
juglans skills remove <NAME>           # delete one
```

**Examples:**

```bash
juglans skills add anthropics/skills --list
juglans skills add anthropics/skills --skill pdf
juglans skills add anthropics/skills --all -o ./my-prompts
juglans skills list
juglans skills remove pdf
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

**Arguments:**

| Name | Description |
|------|-------------|
| `PATH` | Project directory (default: current directory) |

**Options:**

| Option | Description |
|--------|-------------|
| `--tag <TAG>` | Custom image tag (default: `juglans-{project}:latest`) |
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
| `--schedule <EXPR>`, `-s` | Cron expression (required for `.jg` files; overrides manifest metadata for `.jgflow`) |

**Example:**

```bash
juglans cron src/daily-report.jg -s "0 9 * * *"
juglans cron my-package.jgflow                # uses `schedule` from the manifest
```

> `.jg` files have no schedule metadata, so `--schedule` is required. `.jgflow` package manifests may embed the schedule.

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
