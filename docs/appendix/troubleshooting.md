# Troubleshooting

Common errors, their causes, and solutions.

---

### 1. No entry node found

**Error:** `No entry node found (no node with in-degree 0)`

**Cause:** Every node in the workflow has at least one incoming edge, so topological sort cannot determine a starting point.

**Solution:** Ensure at least one node has no incoming edges — that node will be the entry point:

```juglans
[start]: chat(agent="assistant", message="Hello")
```

---

### 2. Unreachable node

**Error:** `Node 'process' is unreachable from entry`

**Cause:** A node exists in the workflow but has no incoming edge from any reachable node (determined by topological sort from nodes with in-degree 0).

**Solution:** Add an edge connecting it to the graph, or remove the unused node.

---

### 3. Configuration file not found

**Error:** `juglans.toml not found`

**Cause:** No `juglans.toml` in the current directory or parent directories.

**Solution:**

```bash
# Create a project with config
juglans init my-project

# Or specify config path
JUGLANS_CONFIG=/path/to/juglans.toml juglans workflow.jg
```

---

### 4. API key not configured

**Error:** `API key not configured` or `401 Unauthorized`

**Cause:** `account.api_key` is missing from config and `JUGLANS_API_KEY` is not set.

**Solution:**

```bash
# Set via environment variable
export JUGLANS_API_KEY="jug0_sk_..."

# Or add to juglans.toml
# [account]
# api_key = "jug0_sk_..."
```

---

### 5. MCP server connection failed

**Error:** `Failed to connect to MCP server: connection refused`

**Cause:** The MCP server is not running or the URL is wrong.

**Solution:**

```bash
# Verify the MCP server is running
curl http://localhost:3001/mcp/filesystem

# Check juglans.toml [[mcp_servers]] config
# Ensure base_url matches the actual server address
```

---

### 6. Variable resolution failed

**Error:** `Failed to resolve variable: result`

**Cause:** The variable was not set before being referenced. Context variables must be set via assignment syntax before use.

**Solution:** Ensure the node that sets the variable runs before the one that reads it:

```juglans
[init]: result = "data"
[use]: chat(agent="assistant", message=result)

[init] -> [use]
```

---

### 7. Agent not found

**Error:** `Agent 'my-agent' not found in registry`

**Cause:** The agent slug doesn't match any loaded `.jgagent` file or remote resource.

**Solution:**

```bash
# Check available agents
juglans list -t agent

# Ensure the agents import path is correct in the .jg file:
# agents: ["./agents/*.jgagent"]
```

---

### 8. Parse error: unexpected token

**Error:** `Parse error at line 5: expected node definition, found '...'`

**Cause:** Syntax error in the .jg file -- often a missing bracket, wrong delimiter, or invalid node format.

**Solution:**

```bash
# Validate syntax
juglans check workflow.jg

# Common fixes:
# - Node IDs must be in brackets: [node_id]
# - Strings use double quotes: "value"
# - Parameters use = not : inside tool calls
```

---

### 9. Circular dependency detected

**Error:** `Circular dependency detected: [A] -> [B] -> [A]`

**Cause:** Edges form a cycle in the DAG. Juglans requires acyclic graphs (use `foreach`/`while` for loops).

**Solution:** Break the cycle by restructuring the workflow, or use loop constructs for intentional iteration.

---

### 10. Resource already exists

**Error:** `Resource 'my-prompt' already exists (use --force to overwrite)`

**Cause:** Pushing a resource that already exists on the server.

**Solution:**

```bash
juglans push src/prompts/my-prompt.jgprompt --force
```

---

### 11. Flow import file not found

**Error:** `Flow import failed: file './auth.jg' not found`

**Cause:** The `flows:` declaration references a file path that doesn't exist relative to the current .jg file.

**Solution:** Verify the file path is correct and relative to the importing file's directory.

---

### 12. Max loop iterations exceeded

**Error:** `Loop exceeded maximum iterations (100)`

**Cause:** A `foreach` or `while` loop hit the iteration limit.

**Solution:** Increase the limit in `juglans.toml`:

```toml
[limits]
max_loop_iterations = 500
```

---

### 13. HTTP request timeout

**Error:** `HTTP request timed out after 120s`

**Cause:** A `fetch()` call or API request exceeded the timeout.

**Solution:**

```toml
[limits]
http_timeout_secs = 300
```

---

### 14. Port already in use

**Error:** `Address already in use (port 3000)`

**Cause:** Another process is using the port when starting `juglans web`.

**Solution:**

```bash
# Use a different port
juglans web --port 8081

# Or find and stop the conflicting process
lsof -i :3000
```

---

### 15. Python worker failed to start

**Error:** `Failed to start Python worker`

**Cause:** Python is not installed or the required module is missing.

**Solution:**

```bash
# Verify Python is available
python3 --version

# Install required modules
pip install pandas scikit-learn

# Adjust worker count if needed in juglans.toml
# [limits]
# python_workers = 2
```
