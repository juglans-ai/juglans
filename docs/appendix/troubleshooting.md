# Troubleshooting

Common errors, their causes, and solutions.

---

### 1. No entry node specified (warning)

**Warning:** `No entry node specified; using first node as entry point` (W001)

**Cause:** The workflow does not explicitly declare an entry node. This is a warning, not a fatal error — Juglans falls back to the first node in file order and continues execution.

**Solution:** Either ignore the warning, or make the entry explicit by ensuring one node has no incoming edges so the topological sort picks it unambiguously:

```juglans
[start]: chat(agent="assistant", message="Hello")
```

---

### 2. Unreachable node

**Warning:** `Node 'process' is not reachable from entry node` (W002)

**Cause:** A node exists in the workflow but has no path from the entry node, so it will never execute.

**Solution:** Add an edge connecting it to the graph, or remove the unused node.

---

### 3. API key not configured

**Error:** `No API-key provided` or `401 Unauthorized`

**Cause:** No LLM provider configured. juglans is local-first and calls providers directly using their API keys.

**Solution:**

```bash
# Set any one (or more) provider API key
export OPENAI_API_KEY="sk-..."
export ANTHROPIC_API_KEY="sk-ant-..."
export DEEPSEEK_API_KEY="sk-..."
export QWEN_API_KEY="sk-..."
export GEMINI_API_KEY="..."
export ARK_API_KEY="..."        # ByteDance / BytePlus Ark
export XAI_API_KEY="xai-..."

# Or add to juglans.toml (optional since v0.2.5)
# [ai.providers.openai]
# api_key = "sk-..."
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

**Cause:** The agent node is not defined in the workflow or not imported via `libs:`.

**Solution:**

Ensure the agent is defined as an inline JSON map node in the same `.jg` file, or imported from a library:

```juglans
# Define inline
[my_agent]: { "model": "gpt-4o-mini", "system_prompt": "..." }
[ask]: chat(agent=my_agent, message=input.query)

# Or import from library
libs: ["./agents.jg"]
[ask]: chat(agent=agents.my_agent, message=input.query)
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

### 8. Cycle detected

**Error:** `Cycle detected involving node 'process'. Workflows must be acyclic (DAG)` (E002)

**Cause:** Edges form a cycle in the DAG. Juglans requires acyclic graphs (use `foreach`/`while` for loops).

**Solution:** Break the cycle by restructuring the workflow, or use loop constructs for intentional iteration.

---

### 10. Resource already exists

**Error:** `Resource 'my-prompt' already exists (use --force to overwrite)`

**Cause:** Pushing a resource that already exists on the server.

**Solution:**

```bash
juglans push src/prompts/my-prompt.jgx --force
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

**Cause:** Another process is using the port when starting `juglans web` (or the unified `juglans serve`, which wraps web, bot adapters, and cron triggers).

**Solution:**

```bash
# Use a different port
juglans web --port 8081
# or
juglans serve --port 8081

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

---

### 15. Registry publish unauthorized

**Error:** `401 Unauthorized` or `missing registry API key` from `juglans publish`

**Cause:** The registry client could not find a publish credential in the environment or config.

**Solution:** Export one of the accepted environment variables before running `juglans publish`:

```bash
export JUGLANS_REGISTRY_API_KEY="jgr_..."
# or (legacy alias)
export REGISTRY_API_KEY="jgr_..."

juglans publish
```
