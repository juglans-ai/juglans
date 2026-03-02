# CLI Command Reference

The Juglans CLI provides workflow execution, resource management, and development tools.

## Installation

```bash
# Build from source
git clone https://github.com/juglans-ai/juglans.git
cd juglans
cargo build --release

# Install to system
cargo install --path .

# Or add to PATH
export PATH="$PATH:$(pwd)/target/release"
```

## Basic Usage

```bash
juglans [OPTIONS] <FILE> [ARGS]
juglans <COMMAND> [OPTIONS]
```

## Execution Commands

### Execute Workflow

```bash
juglans path/to/workflow.jg [OPTIONS]
```

**Options:**

| Option | Description |
|------|------|
| `--input <JSON>` | Input data (JSON format) |
| `--input-file <FILE>` | Read input from file |
| `--verbose`, `-v` | Verbose output |
| `--dry-run` | Parse only, do not execute |
| `--output <FILE>` | Output result to file |
| `--output-format <FORMAT>` | Output format (text, json), default text |

**Examples:**

```bash
# Basic execution
juglans src/main.jg

# Pass input
juglans src/main.jg --input '{"query": "Hello"}'

# Read input from file
juglans src/main.jg --input-file input.json

# Verbose mode
juglans src/main.jg -v

# Validate only
juglans src/main.jg --dry-run

# JSON format output (convenient for programmatic processing)
juglans src/main.jg --output-format json
```

**JSON Output Format:**

When using `--output-format json`, structured execution results are output:

```json
{
  "success": true,
  "duration_ms": 1234,
  "nodes_executed": 5,
  "final_output": {
    "status": "completed",
    "result": "..."
  }
}
```

This is very useful for CI/CD integration or programmatic processing of workflow results.

---

### Execute Agent (Interactive Mode)

```bash
juglans path/to/agent.jgagent [OPTIONS]
```

**Options:**

| Option | Description |
|------|------|
| `--message <TEXT>` | Initial message |
| `--verbose`, `-v` | Verbose output |
| `--info` | Show Agent information |

**Examples:**

```bash
# Interactive conversation
juglans src/agents/assistant.jgagent

# Send a single message
juglans src/agents/assistant.jgagent --message "What is Rust?"

# View configuration
juglans src/agents/assistant.jgagent --info
```

**Interactive Commands:**

In interactive mode:
- Type a message to send it to the Agent
- `exit` or `quit` to exit
- `clear` to clear history
- `history` to view conversation history

---

### Render Prompt

```bash
juglans path/to/prompt.jgprompt [OPTIONS]
```

**Options:**

| Option | Description |
|------|------|
| `--input <JSON>` | Template variables |
| `--output <FILE>` | Output to file |

**Examples:**

```bash
# Render with default values
juglans src/prompts/greeting.jgprompt

# Pass variables
juglans src/prompts/greeting.jgprompt --input '{"name": "Alice"}'

# Output to file
juglans src/prompts/greeting.jgprompt --output rendered.txt
```

---

## Project Commands

### init - Initialize Project

```bash
juglans init <PROJECT_NAME> [OPTIONS]
```

**Options:**

| Option | Description |
|------|------|
| `--template <NAME>` | Use template (basic, advanced) |

**Examples:**

```bash
# Create a new project
juglans init my-project

# Use advanced template
juglans init my-project --template advanced
```

**Generated Structure:**

```
my-project/
├── juglans.toml
└── src/
    ├── example.jg
    ├── workflows/
    │   └── example.jgflow
    ├── agents/
    │   └── example.jgagent
    ├── pure-agents/
    ├── prompts/
    │   └── example.jgprompt
    └── tools/
```

---

### install - Install Dependencies

Fetch MCP tool schemas:

```bash
juglans install [OPTIONS]
```

**Options:**

| Option | Description |
|------|------|
| `--force` | Force re-fetch |

**Examples:**

```bash
# Install MCP tools
juglans install

# Force refresh
juglans install --force
```

---

## Resource Management

### apply - Push Resources

Push local resources to the Jug0 backend, supporting single file or batch operations.

```bash
juglans apply [PATHS...] [OPTIONS]
```

**Arguments:**

| Argument | Description |
|------|------|
| `PATHS` | File or directory paths (optional, uses workspace configuration when empty) |

**Options:**

| Option | Description |
|------|------|
| `--force` | Overwrite existing resources |
| `--dry-run` | Preview without executing |
| `--type <TYPE>`, `-t` | Filter resource type (workflow, agent, prompt, tool, all) |
| `--recursive`, `-r` | Recursively scan directories |

#### Basic Usage

```bash
# Push a single file
juglans apply src/prompts/my-prompt.jgprompt
juglans apply src/agents/my-agent.jgagent
juglans apply src/workflows/my-flow.jgflow

# Force overwrite
juglans apply src/prompts/my-prompt.jgprompt --force
```

#### Batch Operations

**Using workspace configuration:**

First configure resource paths in `juglans.toml`:

```toml
[workspace]
agents = ["src/agents/**/*.jgagent", "src/pure-agents/**/*.jgagent"]
workflows = ["src/**/*.jg", "src/workflows/**/*.jgflow"]
prompts = ["src/prompts/**/*.jgprompt"]
tools = ["src/tools/**/*.json"]
exclude = ["**/*.backup", "**/test_*"]
```

Then run apply without arguments:

```bash
# Apply all configured resources
juglans apply

# Preview files that will be applied
juglans apply --dry-run

# Apply only workflows
juglans apply --type workflow

# Apply only agents
juglans apply -t agent
```

**Output example:**

```
📦 Using workspace configuration from juglans.toml

📂 Found resources:
  📄 3 workflow(s)
  👤 5 agent(s)
  📝 8 prompt(s)

📤 Applying resources...

  ✅ workflow: trading-assistant.jg - Applied
  ✅ agent: trader.jgagent - Applied
  ⚠️  agent: assistant.jgagent - Skipped (exists, use --force)
  ✅ prompt: greeting.jgprompt - Applied

📊 Summary:
  ✅ 9 succeeded
  ⚠️  1 skipped
  ❌ 0 failed
```

**Apply specific directories:**

```bash
# Apply an entire directory
juglans apply src/workflows/

# Recursively apply all subdirectories
juglans apply src/ -r

# Apply multiple directories
juglans apply src/agents/ src/prompts/

# Apply a specific type
juglans apply src/ -r --type workflow
```

**Glob patterns:**

```bash
# Apply all workflows
juglans apply "src/**/*.jg"

# Apply files with a specific prefix
juglans apply "src/agents/prod_*.jgagent"
```

**Dry-run mode:**

```bash
# Preview files that will be applied
juglans apply --dry-run

# Preview a specific directory
juglans apply src/workflows/ --dry-run
```

Output:

```
📦 Scanning workspace: src/

📂 Found resources:
  📄 3 workflow(s)
  👤 5 agent(s)

🔍 Dry run mode - preview only:

  ✓ src/trading.jg
  ✓ src/analysis.jg
  ✓ src/pipeline.jg
  ✓ src/agents/trader.jgagent
  ✓ src/agents/assistant.jgagent

📊 Total: 8 file(s)

Run without --dry-run to apply.
```

---

### pull - Pull Resources

Pull resources from the Jug0 backend:

```bash
juglans pull <SLUG> [OPTIONS]
```

**Options:**

| Option | Description |
|------|------|
| `--type <TYPE>` | Resource type (prompt, agent, workflow) |
| `--output <DIR>` | Output directory |

**Examples:**

```bash
# Pull a Prompt
juglans pull my-prompt --type prompt

# Pull to a specific directory
juglans pull my-agent --type agent --output ./src/agents/
```

---

### list - List Remote Resources

List resources on the Jug0 backend.

```bash
juglans list [OPTIONS]
```

**Options:**

| Option | Description |
|------|------|
| `--type <TYPE>`, `-t` | Filter resource type (prompt, agent, workflow), optional |

**Examples:**

```bash
# List all resources
juglans list

# List only Prompts
juglans list --type prompt

# List only Agents (short option)
juglans list -t agent

# List only Workflows
juglans list --type workflow
```

**Output format:**

```
greeting-prompt (prompt)
assistant (agent)
market-analyst (agent)
simple-chat (workflow)
data-pipeline (workflow)
```

Output format is: `slug (resource_type)`, one resource per line.

**Empty results:**

If no resources are found, the following is displayed:
```
No resources found.
```

**Use cases:**

- View resources already on the server
- Confirm whether resources have been successfully applied
- Confirm resource existence before pulling

**Notes:**

- Requires a valid API key to be configured
- Only displays resources accessible by the current account
- Sorted by resource type and name

---

### check - Validate File Syntax

Validate the syntax correctness of `.jg`, `.jgagent`, `.jgprompt` files (similar to `cargo check`).

```bash
juglans check [PATH] [OPTIONS]
```

**Arguments:**

| Argument | Description |
|------|------|
| `PATH` | File or directory path to check (optional, defaults to current directory) |

**Options:**

| Option | Description |
|------|------|
| `--all` | Show all issues including warnings |
| `--format <FORMAT>` | Output format (text, json), default text |

**Examples:**

```bash
# Check all files in current directory
juglans check

# Check a specific directory
juglans check ./src/

# Check a single file
juglans check workflow.jg

# Show all warnings
juglans check --all

# JSON format output
juglans check --format json
```

**Output example (text format):**

```
    Checking juglans files in "."

    error[workflow]: src/main.jg (1 error(s), 0 warning(s))
      --> [E001] Entry node 'start' not defined

    warning[workflow]: src/test.jg (1 warning(s))
      --> [W001] Unused node 'debug'

    Finished checking 3 workflow(s), 2 agent(s), 1 prompt(s) - 2 valid with warnings

error: could not validate 1 file(s) due to 1 previous error(s)
```

**Output example (JSON format):**

```json
{
  "total": 6,
  "valid": 5,
  "errors": 1,
  "warnings": 1,
  "by_type": {
    "workflows": 3,
    "agents": 2,
    "prompts": 1
  },
  "results": [
    {
      "file": "src/main.jg",
      "type": "workflow",
      "slug": "main",
      "valid": false,
      "errors": [
        {"code": "E001", "message": "Entry node 'start' not defined"}
      ],
      "warnings": []
    }
  ]
}
```

**Exit codes:**

- `0` - All files validated successfully
- `1` - Syntax errors found

**Use cases:**

- Syntax validation in CI/CD pipelines
- Local checks before committing
- Batch validation of all workflow files in a project

---

### delete - Delete Remote Resources

Delete resources from the Jug0 backend.

```bash
juglans delete <SLUG> --type <TYPE>
```

**Arguments:**

| Argument | Description |
|------|------|
| `SLUG` | Resource slug to delete |

**Options:**

| Option | Description |
|------|------|
| `--type <TYPE>`, `-t` | Resource type (prompt, agent, workflow) |

**Examples:**

```bash
# Delete a Prompt
juglans delete my-prompt --type prompt

# Delete an Agent (short option)
juglans delete my-agent -t agent

# Delete a Workflow
juglans delete chat-flow --type workflow
```

**Notes:**

- Requires a valid API key (via `juglans.toml` or environment variable)
- Deletion is irreversible, use with caution
- Can only delete resources owned by the current account
- A confirmation message is displayed on successful deletion: `Deleted <slug> (<type>)`

**Error Handling:**

- If the resource does not exist, an error is returned
- If there is no permission to delete, an authentication error is returned
- Network errors display corresponding error messages

---

### whoami - Show Account Information

Display current user and workspace configuration information.

```bash
juglans whoami [OPTIONS]
```

**Options:**

| Option | Description |
|------|------|
| `--verbose`, `-v` | Show detailed information |
| `--check-connection` | Test connection to the Jug0 server |

**Basic Usage:**

```bash
# Show account information
juglans whoami

# Show detailed information
juglans whoami --verbose

# Test connection
juglans whoami --check-connection

# Verbose mode + connection test
juglans whoami -v --check-connection
```

**Output example (basic):**

```
📋 Account Information

User ID:       u_demo
Name:          Demo User
Role:          admin
API Key:       jug0_sk_***...***def (configured)

Workspace:     ws_default (My Workspace)
Members:       2 user(s)

Jug0 Server:   http://localhost:3000

Config:        ./juglans.toml
```

**Output example (verbose mode):**

```
📋 Account Information

User ID:       u_demo
Name:          Demo User
Role:          admin
API Key:       jug0_sk_***...***def (configured)

Workspace:     ws_default (My Workspace)
Members:       2 user(s)

Resource Paths:
  Agents:      src/agents/**/*.jgagent, src/pure-agents/**/*.jgagent
  Workflows:   src/**/*.jg, src/workflows/**/*.jgflow
  Prompts:     src/prompts/**/*.jgprompt
  Tools:       src/tools/**/*.json

Exclude:       **/*.backup, **/.draft, **/test_*

Jug0 Server:   http://localhost:3000
Status:        ✅ Connected

Web Server:    127.0.0.1:3000

MCP Servers:   2 configured
  - filesystem (alias: fs): http://localhost:3001/mcp/filesystem
  - github: http://localhost:3001/mcp/github

Config:        ./juglans.toml
```

**Status Indicators:**

- `Connected` - Server connection is normal
- `Server unreachable` - Cannot connect to the server
- `Error: ...` - Connection error
- `Not configured` - API Key not configured

**Use cases:**

- Confirm the current account in use
- Check if the configuration is correct
- Debug connection issues
- View workspace settings
- Verify whether the API Key is configured

---

## Development Server

### web - Start Web Server

```bash
juglans web [OPTIONS]
```

**Options:**

| Option | Default | Description |
|------|--------|------|
| `--host <HOST>` | 127.0.0.1 | Bind address |
| `--port <PORT>` | 8080 | Port number |

**Examples:**

```bash
# Default configuration
juglans web

# Custom port
juglans web --port 3000

# Allow external access
juglans web --host 0.0.0.0 --port 8080
```

**API Endpoints:**

| Endpoint | Method | Description |
|------|------|------|
| `/api/agents` | GET | List Agents |
| `/api/agents/:slug` | GET | Get Agent |
| `/api/prompts` | GET | List Prompts |
| `/api/prompts/:slug` | GET | Get Prompt |
| `/api/prompts/:slug/render` | POST | Render Prompt |
| `/api/workflows` | GET | List Workflows |
| `/api/workflows/:slug/execute` | POST | Execute Workflow |
| `/api/chat` | POST | Chat (SSE) |

---

## Configuration

### Configuration File Location

Searched in order of priority:

1. `./juglans.toml` (current directory)
2. `~/.config/juglans/juglans.toml` (user configuration)
3. `/etc/juglans/juglans.toml` (system configuration)

### Configuration File Format

```toml
# juglans.toml

[account]
id = "user_id"
api_key = "your_api_key"

[jug0]
base_url = "http://localhost:3000"

[server]
host = "127.0.0.1"
port = 8080

[mcp.filesystem]
command = "npx"
args = ["-y", "@anthropic/mcp-filesystem"]
env = { ROOT_DIR = "/workspace" }
```

### Environment Variables

| Variable | Description |
|------|------|
| `JUGLANS_API_KEY` | API key (overrides configuration file) |
| `JUGLANS_CONFIG` | Configuration file path |
| `JUGLANS_LOG_LEVEL` | Log level (debug, info, warn, error) |

---

## Output Formats

### Default Output

```
[node_id] Status message...
[node_id] Result: ...
```

### Verbose Mode (-v)

```
[DEBUG] Loading workflow: main.jg
[DEBUG] Parsed 5 nodes, 4 edges
[INFO] [init] Starting...
[DEBUG] [init] Output: null
[INFO] [chat] Calling agent: assistant
[DEBUG] [chat] Request: {"message": "..."}
[INFO] [chat] Response received (234 tokens)
...
```

### JSON Output

```bash
juglans workflow.jg --output-format json
```

```json
{
  "success": true,
  "duration_ms": 1234,
  "nodes_executed": 5,
  "final_output": { ... }
}
```

---

## Exit Codes

| Code | Description |
|----|------|
| 0 | Success |
| 1 | General error |
| 2 | Parse error |
| 3 | Execution error |
| 4 | Configuration error |
| 5 | Network error |

---

## Troubleshooting

### Common Issues

**Q: Configuration file not found**
```bash
juglans --config /path/to/juglans.toml workflow.jg
```

**Q: API connection failed**
```bash
# Check connection
curl http://localhost:3000/health

# View detailed logs
JUGLANS_LOG_LEVEL=debug juglans workflow.jg
```

**Q: MCP tools unavailable**
```bash
# Reinstall
juglans install --force
```

**Q: Out of memory**
```bash
# For large workflows, increase stack size
RUST_MIN_STACK=8388608 juglans workflow.jg
```
