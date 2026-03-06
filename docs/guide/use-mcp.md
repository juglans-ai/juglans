# How to Use MCP Tools

MCP (Model Context Protocol) is an open protocol that lets Juglans call external tool servers -- filesystem, GitHub, database, or your own custom services.

## Configure MCP Server

In `juglans.toml`, add `[[mcp_servers]]` entries. Juglans connects via HTTP/JSON-RPC, so you need to start the MCP server first.

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
name = "postgres"
base_url = "http://localhost:3001/mcp/postgres"
```

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `name` | string | Yes | Server name, used as default namespace |
| `base_url` | string | Yes | MCP server HTTP address |
| `alias` | string | No | Short namespace alias |
| `token` | string | No | Bearer token for authentication |

## Call MCP Tools in Workflows

Use `namespace.tool_name(params)` format. If `alias` is set, use alias as the namespace; otherwise use `name`.

```juglans
entry: [read]
exit: [done]

# Read a file via filesystem MCP
[read]: fs.read_file(path="/data/config.json")

# Create a GitHub issue
[issue]: github.create_issue(
  repo="owner/repo",
  title="Bug Report",
  body=$output
)

# Query database
[query]: postgres.query(sql="SELECT * FROM users LIMIT 10")

[done]: notify(status="All done")

[read] -> [issue] -> [query] -> [done]
```

## Common MCP Tools

### filesystem

| Tool | Description |
|------|-------------|
| `read_file` | Read file contents |
| `write_file` | Write to a file |
| `list_directory` | List directory entries |
| `create_directory` | Create a directory |
| `delete_file` | Delete a file |
| `move_file` | Move or rename a file |
| `search_files` | Search for files |

### github

| Tool | Description |
|------|-------------|
| `get_repo` | Get repository info |
| `list_issues` | List issues |
| `create_issue` | Create an issue |
| `get_pull_request` | Get a pull request |
| `create_pull_request` | Create a pull request |
| `list_pr_files` | List changed files in a PR |
| `search_code` | Search code |

### postgres

| Tool | Description |
|------|-------------|
| `query` | Execute a SQL query |
| `execute` | Execute a SQL command |
| `list_tables` | List all tables |
| `describe_table` | Describe table structure |

## Error Handling

Use `on error` edges to handle MCP call failures (network errors, server down, invalid params, etc.):

```juglans
name: "MCP with Error Handling"

entry: [fetch_repo]
exit: [done]

[fetch_repo]: github.get_repo(repo=$input.repo)
[process]: chat(agent="reviewer", message="Review: " + json($output))
[handle_error]: notify(status="GitHub API error: " + $error.message)
[fallback]: set_context(repo_info=null)
[done]: notify(status="Complete")

[fetch_repo] -> [process] -> [done]
[fetch_repo] on error -> [handle_error]
[handle_error] -> [fallback] -> [done]
```

## Complete Example: Code Review Workflow

```juglans
name: "Code Review"

entry: [fetch_pr]
exit: [done]

[fetch_pr]: github.get_pull_request(
  repo=$input.repo,
  number=$input.pr_number
)

[get_files]: github.list_pr_files(
  repo=$input.repo,
  number=$input.pr_number
)

[review]: chat(
  agent="code-reviewer",
  message="Review these changes:\n" + json($output)
)

[comment]: github.create_review_comment(
  repo=$input.repo,
  number=$input.pr_number,
  body=$output
)

[done]: notify(status="Review completed")

[fetch_pr] -> [get_files] -> [review] -> [comment] -> [done]
[fetch_pr] on error -> [done]
```

## Build a Custom MCP Server

Implement an HTTP server that handles JSON-RPC requests with two methods:

- `tools/list` -- returns available tool definitions
- `tools/call` -- executes a tool and returns results

Example in Python (Flask):

```python
from flask import Flask, request, jsonify

app = Flask(__name__)

@app.route('/messages', methods=['POST'])
def handle():
    req = request.json
    method = req.get("method")

    if method == "tools/list":
        return jsonify({
            "jsonrpc": "2.0",
            "id": req["id"],
            "result": {
                "tools": [{
                    "name": "my_tool",
                    "description": "My custom tool",
                    "inputSchema": {
                        "type": "object",
                        "properties": {"input": {"type": "string"}},
                        "required": ["input"]
                    }
                }]
            }
        })

    elif method == "tools/call":
        name = req["params"]["name"]
        args = req["params"]["arguments"]
        result = do_work(name, args)
        return jsonify({
            "jsonrpc": "2.0",
            "id": req["id"],
            "result": {"content": [{"type": "text", "text": result}]}
        })

app.run(port=5000)
```

Then configure and use it:

```toml
[[mcp_servers]]
name = "my-tools"
base_url = "http://localhost:5000"
```

```juglans
[result]: my_tools.my_tool(input="hello")
```
