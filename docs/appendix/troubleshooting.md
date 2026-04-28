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

# Or configure inside juglans.toml:
# [ai.providers.openai]
# api_key = "sk-..."
```

---

### 4. MCP server connection failed

**Error:** `Failed to connect to MCP server: connection refused`

**Cause:** The MCP server is not running or the URL is wrong.

**Solution:**

```bash
# Verify the MCP server is running
curl http://localhost:3001/mcp/filesystem
```

MCP servers are declared inline on the `chat()` call (not in `juglans.toml`), so check the workflow itself:

```juglans
[reply]: chat(
  agent = my_agent,
  message = input.text,
  mcp = {
    "filesystem": "http://localhost:3001/mcp/filesystem"
  }
)
```

Make sure the URL matches the running server's address (host + port + path).

---

### 5. Variable resolution failed

**Error:** `Failed to resolve variable: result`

**Cause:** The variable was not set before being referenced. Context variables must be set via assignment syntax before use.

**Solution:** Ensure the node that sets the variable runs before the one that reads it:

```juglans
[init]: result = "data"
[use]: chat(agent="assistant", message=result)

[init] -> [use]
```

---

### 6. Agent not found

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

### 7. Parse error: unexpected token

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

### 9. Resource already exists

**Error:** `Resource 'my-prompt' already exists (use --force to overwrite)`

**Cause:** Pushing a resource that already exists on the server.

**Solution:**

```bash
juglans push src/prompts/my-prompt.jgx --force
```

---

### 10. Flow import file not found

**Error:** `Flow import failed: file './auth.jg' not found`

**Cause:** The `flows:` declaration references a file path that doesn't exist relative to the current .jg file.

**Solution:** Verify the file path is correct and relative to the importing file's directory.

---

### 11. Max loop iterations exceeded

**Error:** `Loop exceeded maximum iterations (100)`

**Cause:** A `foreach` or `while` loop hit the iteration limit.

**Solution:** Increase the limit in `juglans.toml`:

```toml
[limits]
max_loop_iterations = 500
```

---

### 12. HTTP request timeout

**Error:** `HTTP request timed out after 120s`

**Cause:** A `fetch()` call or API request exceeded the timeout.

**Solution:**

```toml
[limits]
http_timeout_secs = 300
```

---

### 13. Port already in use

**Error:** `Address already in use (port 3000)`

**Cause:** Another process is using the port when starting `juglans web` (or the unified `juglans serve`, which wraps web, channels, and cron triggers).

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

### 14. Python worker failed to start

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

---

### 16. Conversation history isn't persisting

**Symptom:** A bot that should have memory keeps treating each message as a first turn. `reply.chat_id` is empty or the history file is missing.

**Cause:** Likely one of (a) `[history].enabled = false`, (b) the `chat()` call has `state="silent"` / `state="display_only"` which skips persistence, (c) no `chat_id` is being resolved — check `input.chat_id` is set by the adapter, or that `chat(chat_id=...)` is passed explicitly.

**Solution:**

```bash
# Verify the configured path is writable
ls -la .juglans/history/

# Inspect what's stored for one thread
cat .juglans/history/telegram_12345_agent.jsonl

# Turn backend to memory for a quick test
JUGLANS_HISTORY_BACKEND=memory juglans your-workflow.jg
```

Then check that `chat()` nodes you expect to persist do not have `state="silent"` or `state="display_only"`.

---

### 17. `history.*` tool returns `{ "ok": false, "reason": "history disabled" }`

**Cause:** The workflow called a `history.*` builtin but the global history store was never initialized — either `[history].enabled = false`, `backend = "none"`, or the call path doesn't pass through a code entry point that runs `init_global()`.

**Solution:** Confirm `[history] enabled = true` and a valid `backend`. For bot and `juglans serve` paths this happens automatically; for custom embeddings check that `juglans::services::history::init_global(&config.history)` runs at startup.

---

### 18. Corrupt JSONL history file

**Symptom:** A thread loads fewer messages than expected, or parse warnings appear in the log (`[history] skipping corrupt line N`).

**Cause:** A previous process was killed mid-write, leaving a truncated JSON line. The loader skips the bad line but the turn is lost.

**Solution:** Either accept the skip (subsequent turns will continue appending cleanly), or hand-edit the `.jsonl` file to remove the truncated line. For higher durability under kill-9 / power-loss, switch the backend to `sqlite`:

```toml
[history]
backend = "sqlite"
path = ".juglans/history.db"
```

---

### 19. `feishu_send` / `feishu_webhook` not found after upgrading

**Symptom:** `juglans check` warns `W004: Unknown tool 'feishu_send'` (or `'feishu_webhook'`); workflows that previously worked now fail at the validator.

**Cause:** Both tools were **removed in v0.2.18** in favor of platform-namespaced builtins under the `feishu.*` namespace. This is a breaking change documented in CHANGELOG 0.2.18.

**Solution:** Migrate the call sites:

| Old | New |
|---|---|
| `feishu_send(chat_id="oc_x", message="hi")` | `feishu.send_message(chat_id="oc_x", text="hi")` — note `message` → `text` |
| `feishu_send(chat_id="oc_x", image="./pic.png")` | `feishu.send_image(chat_id="oc_x", image="./pic.png")` |
| `feishu_send(chat_id=..., message=..., image=...)` | Two separate calls — `feishu.send_message` then `feishu.send_image` |
| `feishu_webhook(message="hi")` | `feishu.send_webhook(message="hi")` — same params, just renamed |

The new tools follow the same dotted convention as `db.*` / `history.*` and share behavior with `telegram.*` / `discord.*` / `wechat.*` (see [Platform Messaging in builtins.md](../reference/builtins.md#platform-messaging-telegram-discord-wechat-feishu)).

---

### 20. Discord bot exits with close code 4004 or 4014

**Symptom:**

```
[discord] Authentication failed (4004). Check [channels.discord.<id>].token.
```

or

```
[discord] Gateway rejected intents (close code 4014). Enable 'MESSAGE CONTENT INTENT'
         in the Discord Developer Portal …
```

**Cause:**

- **4004**: Token wrong, expired, or `${DISCORD_BOT_TOKEN}` interpolation failed (`.env` not loaded, var name typo).
- **4014**: The `MESSAGE_CONTENT` intent is **privileged** — Discord rejects connections that request it without portal opt-in. This is the #1 first-time setup failure.

**Solution:**

```bash
# 4004 — verify the token interpolates
grep DISCORD_BOT_TOKEN .env
# regenerate at https://discord.com/developers/applications → Bot → Reset Token if needed

# 4014 — open the dev portal:
#   Application → Bot → Privileged Gateway Intents → enable MESSAGE CONTENT INTENT
# OR drop `message_content` from the channel's intents:
```

```toml
[channels.discord.community]
token = "${DISCORD_BOT_TOKEN}"
intents = ["guilds", "guild_messages", "direct_messages"]   # no message_content
```

Without `message_content`, the bot connects but receives empty `content` fields on incoming messages — fine for slash-command-only setups, broken for chat workflows.

If session resumes keep failing with `Invalid Session (op 9)`, delete the cached resume file:

```bash
rm .juglans/discord/gateway.json
```

---

### 21. `<platform>.send_message` errors with "no target" or missing token

**Symptom:**

```
telegram.send_message: no target — pass `chat_id` explicitly, or run from a bot
                       workflow where `input.platform_chat_id` is set
```

or

```
wechat.send_message: no WeChat session found in .juglans/wechat/.
                     Run `juglans serve` once with `[channels.wechat]` to complete QR login.
```

**Cause:** Each `*.send_message` builtin auto-resolves its target from `input.platform_chat_id` — set automatically by the channel on inbound messages. From a cron job, error handler, or any node that doesn't have a current platform message, you must pass the target explicitly.

**Solution:**

```juglans
# Auto-resolve from inbound message (bot reply branch)
[reply]: telegram.send_message(text = "hi")

# Explicit target (cron, broadcast)
[alert]: telegram.send_message(chat_id = "12345", text = "deploy done")
[ping]:  discord.send_message(channel_id = "987", text = "ping")
[push]:  wechat.send_message(user_id = "u_abc", text = "reminder")
```

For WeChat specifically, the token + base_url come from `.juglans/wechat/{account}.json` (the file written after QR login). Start `juglans serve` with `[channels.wechat]` configured and complete the QR login once; from then on the `wechat.send_message` builtin works from any context. Delete the file to force a re-login.

For Telegram / Discord / Feishu, ensure the `[channels.<kind>.<id>]` section in `juglans.toml` has a non-empty `token` (or `app_id` + `app_secret` for Feishu event-mode, or `incoming_webhook_url` for egress-only Feishu).
