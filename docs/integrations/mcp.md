# MCP Tool Integration

This guide explains how to integrate Model Context Protocol (MCP) tool servers in Juglans.

## What is MCP

MCP (Model Context Protocol) is an open protocol for exposing external tool capabilities to AI systems.

```
┌─────────────────┐  HTTP   ┌─────────────────┐
│    Juglans      │◀───────▶│   MCP Server    │
│                 │ JSON-RPC│                 │
│  Workflow       │         │  - Filesystem   │
│  Executor       │         │  - GitHub       │
│                 │         │  - Database     │
└─────────────────┘         └─────────────────┘
```

## Configure MCP Servers

### juglans.toml Configuration

**Important:** Juglans connects to MCP servers via HTTP/JSON-RPC. You need to start the MCP server first (either in jug0 or as a standalone service), then configure the HTTP connection.

#### HTTP Connection Method

```toml
# Filesystem tools
[[mcp_servers]]
name = "filesystem"
base_url = "http://localhost:3001/mcp/filesystem"
alias = "fs"

# GitHub tools
[[mcp_servers]]
name = "github"
base_url = "http://localhost:3001/mcp/github"
token = "${GITHUB_TOKEN}"

# Custom MCP server
[[mcp_servers]]
name = "my-tools"
base_url = "http://localhost:5000/mcp"
token = "optional_token"

# Cloud MCP service
[[mcp_servers]]
name = "cloud-service"
base_url = "https://mcp.example.com/v1"
token = "${MCP_API_KEY}"
```

**Configuration details:**

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | Yes | Server name (used to generate tool names) |
| `base_url` | string | Yes | MCP server HTTP address |
| `alias` | string | No | Alias |
| `token` | string | No | Authentication token (Bearer token) |

### Starting MCP Servers

Juglans does not automatically start MCP servers. You need to start them manually or use jug0's integrated MCP service:

**Option 1: Use jug0's MCP Integration**

jug0 can host MCP servers and expose them via HTTP:

```bash
# Configure and start MCP service in jug0
cd jug0
cargo run -- --mcp-enabled
```

**Option 2: Start an Independent MCP Server**

Use an HTTP-to-MCP bridge tool:

```bash
# Example: Start filesystem MCP service
npx @anthropic/mcp-filesystem --http --port 3001
```

**Option 3: Custom HTTP MCP Server**

Implement an HTTP server following the MCP JSON-RPC protocol (see below)

## Using MCP Tools in Workflows

### Tool Naming Convention

MCP tools are used in workflows with the `<namespace>.<tool_name>` format:

**Namespace source:**
- If `alias` is configured, use the alias
- Otherwise use the `name`

**Example configuration:**

```toml
[[mcp_servers]]
name = "filesystem"
base_url = "http://localhost:3001/mcp/filesystem"
alias = "fs"  # Optional alias
```

**Calling in workflows:**

```yaml
# Use alias (if configured)
[read]: fs.read_file(path="/data/input.txt")

# Or use name (if no alias)
[read]: filesystem.read_file(path="/data/input.txt")

# GitHub tool example
[issue]: github.create_issue(
  repo="owner/repo",
  title="Bug Report",
  body=$ctx.report
)
```

**Naming format:** `namespace.tool_name` (separated by a dot, not an underscore)

### Complete Example

```yaml
name: "Code Review Workflow"

entry: [fetch_pr]
exit: [done]

# Fetch PR from GitHub
[fetch_pr]: github.get_pull_request(
  repo=$input.repo,
  number=$input.pr_number
)

# Save PR info
[save_pr]: set_context(pr=$output)

# Get changed files
[get_files]: github.list_pr_files(
  repo=$input.repo,
  number=$input.pr_number
)

# AI code review
[review]: chat(
  agent="code-reviewer",
  message="Review these changes:\n" + json($output)
)

# Post comment
[comment]: github.create_review_comment(
  repo=$input.repo,
  number=$input.pr_number,
  body=$output
)

[done]: notify(status="Review completed")

[fetch_pr] -> [save_pr] -> [get_files] -> [review] -> [comment] -> [done]
```

## Common MCP Servers

### @anthropic/mcp-filesystem

Filesystem operations (requires starting the HTTP service first):

```toml
[[mcp_servers]]
name = "filesystem"
base_url = "http://localhost:3001/mcp/filesystem"
```

Start the server (assuming an HTTP bridge is available):

```bash
# Requires HTTP-to-stdio bridge tool
npx @anthropic/mcp-filesystem --http --port 3001
```

Available tools:

| Tool | Description |
|------|-------------|
| `read_file` | Read file contents |
| `write_file` | Write to a file |
| `list_directory` | List a directory |
| `create_directory` | Create a directory |
| `delete_file` | Delete a file |
| `move_file` | Move/rename a file |
| `search_files` | Search for files |

```yaml
# Read a file
[read]: filesystem.read_file(path="data/config.json")

# Write a file
[write]: filesystem.write_file(
  path="output/result.txt",
  content=$ctx.result
)

# List a directory
[list]: filesystem.list_directory(path="src/")
```

### @anthropic/mcp-github

GitHub operations (requires starting the HTTP service first):

```toml
[[mcp_servers]]
name = "github"
base_url = "http://localhost:3001/mcp/github"
token = "${GITHUB_TOKEN}"
```

Start the server (assuming an HTTP bridge is available):

```bash
export GITHUB_TOKEN="ghp_..."
npx @anthropic/mcp-github --http --port 3001
```

Available tools:

| Tool | Description |
|------|-------------|
| `get_repo` | Get repository information |
| `list_issues` | List Issues |
| `create_issue` | Create an Issue |
| `get_pull_request` | Get a PR |
| `create_pull_request` | Create a PR |
| `list_pr_files` | List PR files |
| `search_code` | Search code |

```yaml
# Search code
[search]: github.search_code(
  query="TODO in:file language:rust",
  repo=$input.repo
)

# Create an Issue
[create]: github.create_issue(
  repo=$input.repo,
  title="Found TODOs",
  body="Found " + len($output.items) + " TODOs"
)
```

### @anthropic/mcp-postgres

PostgreSQL database (requires starting the HTTP service first):

```toml
[[mcp_servers]]
name = "postgres"
base_url = "http://localhost:3001/mcp/postgres"
```

Start the server (assuming an HTTP bridge is available):

```bash
export DATABASE_URL="postgresql://..."
npx @anthropic/mcp-postgres --http --port 3001
```

Available tools:

| Tool | Description |
|------|-------------|
| `query` | Execute a SQL query |
| `execute` | Execute a SQL command |
| `list_tables` | List tables |
| `describe_table` | Describe table structure |

```yaml
# Query data
[query]: postgres.query(
  sql="SELECT * FROM users WHERE active = true LIMIT 10"
)

# Get table structure
[schema]: postgres.describe_table(table="users")
```

## Custom MCP Servers

### Creating an HTTP MCP Server

Implement the HTTP + JSON-RPC protocol in any language:

```python
# my_mcp_server.py
from flask import Flask, request, jsonify

app = Flask(__name__)

@app.route('/messages', methods=['POST'])
def handle_request():
    req = request.json
    method = req.get("method")

    if method == "tools/list":
        return jsonify({
            "jsonrpc": "2.0",
            "id": req.get("id"),
            "result": {
                "tools": [
                    {
                        "name": "my_tool",
                        "description": "My custom tool",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "input": {"type": "string"}
                            },
                            "required": ["input"]
                        }
                    }
                ]
            }
        })

    elif method == "tools/call":
        tool_name = req["params"]["name"]
        arguments = req["params"]["arguments"]

        if tool_name == "my_tool":
            result = process(arguments["input"])
            return jsonify({
                "jsonrpc": "2.0",
                "id": req.get("id"),
                "result": {
                    "content": [{"type": "text", "text": result}]
                }
            })

    return jsonify({"error": "Unknown method"}), 400

if __name__ == '__main__':
    app.run(port=5000)
```

### Configure the Custom Server

Start the server first:

```bash
python ./tools/my_mcp_server.py
```

Then configure Juglans:

```toml
[[mcp_servers]]
name = "my-tools"
base_url = "http://localhost:5000"
```

### Use in Workflows

```yaml
[custom]: my-tools.my_tool(input=$ctx.data)
```

## Tool Discovery

### List Available Tools

```bash
# List all MCP tools
juglans tools --list

# List tools from a specific server
juglans tools --list --server filesystem

# Show tool details
juglans tools --describe filesystem.read_file
```

### Tool Discovery Process

When a workflow loads an Agent, Juglans will:

1. Read the `[[mcp_servers]]` configuration from `juglans.toml`
2. Send a `tools/list` JSON-RPC request to each server
3. Retrieve tool definitions and cache them in memory
4. Register the tools as callable built-in functions

## Error Handling

### MCP Tool Errors

```yaml
[api_call]: github.get_repo(repo=$input.repo)
[api_call] -> [process]
[api_call] on error -> [handle_error]

[handle_error]: notify(status="GitHub API error, repo may not exist")
[fallback]: set_context(repo_info=null)

[handle_error] -> [fallback]
```

### Timeout Handling

Set timeouts in configuration:

```toml
[mcp.slow-service]
url = "http://slow-api.example.com/mcp"
timeout = 120  # Seconds
```

## Best Practices

### 1. Environment Variable Management

Do not hardcode secrets in configuration files:

```toml
# Good
[mcp.github]
env = { GITHUB_TOKEN = "${GITHUB_TOKEN}" }

# Bad
[mcp.github]
env = { GITHUB_TOKEN = "ghp_xxxx..." }
```

### 2. Tool Permissions

Restrict access scope in the MCP server implementation, or use a proxy layer to control permissions.

Juglans only connects via HTTP; permission control should be implemented on the MCP server side.

### 3. Error Recovery

Add error handling for MCP calls:

```yaml
[fetch]: github.get_repo(repo=$input.repo)
[fetch] -> [process]
[fetch] on error -> [retry_or_fallback]
```

### 4. Logging

Enable debug logging to troubleshoot issues:

```toml
[logging]
level = "debug"
```

### 5. Service Health Checks

Ensure MCP servers are running before Juglans starts:

```bash
# Check if MCP server is reachable
curl http://localhost:3001/mcp/filesystem/messages -d '{"jsonrpc":"2.0","method":"tools/list","id":"1"}'
```

## Troubleshooting

### Q: Tool Not Found

Make sure the MCP server is running and accessible:

```bash
# Test MCP server connection
curl http://localhost:3001/mcp/filesystem/messages \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"tools/list","id":"1"}'
```

### Q: MCP Server Connection Failed

Check the server address and status:

```bash
# Check if the server is running
curl http://localhost:3001/health

# Check if the configured base_url is correct
```

### Q: Authentication Failed

Verify environment variables:

```bash
echo $GITHUB_TOKEN
```

### Q: Timeout Error

The MCP client has a default timeout of 30 seconds. Check the network or MCP server performance.

To adjust the timeout, you need to modify `src/services/mcp.rs` in the Juglans source code.
