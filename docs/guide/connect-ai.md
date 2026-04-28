# How to Connect AI Models

Juglans is local-first. It calls LLM providers directly using API keys you configure either in `juglans.toml` or via environment variables. There is no remote backend dependency — the language runs entirely on your machine.

## Configure Providers

Configure one or more providers in `juglans.toml` at the project root:

```toml
[account]
id = "your_user_id"
name = "Your Name"
role = "developer"

[ai.providers.openai]
api_key = "sk-..."

[ai.providers.anthropic]
api_key = "sk-ant-..."

[ai.providers.deepseek]
api_key = "sk-..."

[ai.providers.gemini]
api_key = "..."

[ai.providers.qwen]
api_key = "sk-..."

[ai.providers.byteplus]
api_key = "..."

[ai.providers.xai]
api_key = "..."
```

Supported provider keys: `openai`, `anthropic`, `deepseek`, `gemini`, `qwen`, `byteplus` (ByteDance Ark), `xai`, plus two specialty providers documented below: `claude_code` and `juglans`.

Configuration file search order: `./juglans.toml` → `~/.config/juglans/juglans.toml` → `/etc/juglans/juglans.toml`.

You can also configure providers via environment variables (no `juglans.toml` needed):

```bash
export OPENAI_API_KEY="sk-..."
export ANTHROPIC_API_KEY="sk-ant-..."
export DEEPSEEK_API_KEY="sk-..."
export GEMINI_API_KEY="..."
export QWEN_API_KEY="sk-..."
export XAI_API_KEY="..."
export ARK_API_KEY="..."              # ByteDance / BytePlus Ark
```

Juglans will pick up any of these on startup. Each provider also accepts `base_url` for local proxies (see the Ollama section below); the environment overrides `OPENAI_API_BASE` / `ANTHROPIC_BASE_URL` / `ARK_API_BASE` apply for the corresponding upstreams. The `juglans/` proxy provider is configured exclusively via `[ai.providers.juglans]` in `juglans.toml` — no env var.

### Claude Code provider

If you have the `claude` CLI installed (from the [Claude Code](https://docs.claude.com/en/docs/claude-code) release), Juglans can dispatch through it as an LLM provider. No API key is needed — authentication is handled by the CLI itself:

```juglans
[reply]: chat(model="claude-code/sonnet", message=input.text)
```

Model names use the `claude-code/<variant>` prefix. Tool calling goes through MCP; see [use-mcp.md](./use-mcp.md) for how to wire MCP servers to a Claude Code agent.

### `juglans/` proxy provider

The `juglans/` provider routes requests through the juglans-wallet proxy. Agents running behind the proxy don't need to hold LLM credentials directly — the proxy handles provider keys and quota server-side:

```toml
[ai.providers.juglans]
api_key  = "${JUGLANS_API_KEY}"
base_url = "http://127.0.0.1:3002/v1/llm"    # default
```

```juglans
[reply]: chat(model="juglans/deepseek-chat", message=input.text)
```

The `juglans/<upstream-model>` format picks any model the proxy is configured to forward — the proxy rewrites the upstream model name and injects the real key.

## Test Connection

Create a minimal chat workflow:

```juglans
[assistant]: {
  "model": "gpt-4o-mini",
  "system_prompt": "You are a helpful assistant."
}

[test]: chat(agent=assistant, message="Say hello in one word.")
[done]: print(message="Connection OK. Response: " + output)

[assistant] -> [test] -> [done]
```

Run it:

```bash
juglans test-connection.jg
```

If configured correctly, you will see `Using local LLM provider (direct API)` in the log followed by the model's response. If it fails, check:

- That at least one provider's `api_key` is set in `juglans.toml` or env
- That the model name is supported by the configured provider
- That you have network access to the provider's API

## Use Local Models (Ollama)

Juglans does not have a dedicated Ollama provider. Instead, point the built-in OpenAI provider at the Ollama OpenAI-compatible endpoint:

```toml
[ai.providers.openai]
api_key = "ollama"
base_url = "http://localhost:11434/v1"
```

Then reference a local model in your workflow using the `openai/` prefix:

```juglans
[local_agent]: {
  "model": "openai/llama3",
  "temperature": 0.7,
  "system_prompt": "You are a helpful assistant."
}

[ask]: chat(agent=local_agent, message=input.query)
[done]: print(message=output)

[local_agent] -> [ask] -> [done]
```

The same trick works for any OpenAI-compatible server (LM Studio, vLLM, llama.cpp, etc.) -- just swap the `base_url`.

## Inline Agents and Library Agents

Agents (model + system prompt + temperature etc.) can be defined inline inside a workflow or imported from `libs:` for reuse across files.

```juglans
[my_agent]: {
  "model": "gpt-4o-mini",
  "system_prompt": "You are a helpful assistant."
}

[start]: print(msg="begin")
[chat]: chat(agent=my_agent, message=input.query)
[end]: print(msg="done")

[my_agent] -> [start] -> [chat] -> [end]
```

## Conversation History

When a `chat()` node has a `chat_id` resolved, Juglans automatically loads the tail of that thread before the LLM call and appends the user / assistant turn afterwards — so multi-turn conversations "just work" without you threading a history array by hand. Persistence honors the `state` parameter (`silent` and `display_only` skip storage).

### `chat_id` resolution (highest to lowest priority)

1. **Explicit** — `chat(message=..., chat_id="support:123")`
2. **`reply.chat_id`** — set by a prior `chat()` in the same run, so a chain of `chat()` nodes stays on one thread
3. **`input.chat_id`** — injected by channels as `"{platform}:{platform_chat_id}:{agent_slug}"` (e.g. `telegram:12345:support_bot`, `discord:987654321:support_bot` where the middle segment is the channel id for guild bots and the DM id for direct messages), so chat workflows get memory without any code change
4. **None** — the call is stateless; nothing is loaded or stored

To explicitly skip history for a single call, pass `state="silent"` (drops the turn from storage) or an empty `chat_id=""`.

### Storage backends

Configured once in `juglans.toml`:

```toml
[history]
enabled = true              # master switch
backend = "jsonl"           # "jsonl" | "sqlite" | "memory" | "none"
dir = ".juglans/history"    # JSONL: one file per chat_id
# path = ".juglans/history.db"  # SQLite path
max_messages = 20           # cap on auto-loaded messages per call
max_tokens = 8000           # soft token budget
retention_days = 30         # GC threshold (0 disables)
```

| Backend | Use when | Scales to |
|---|---|---|
| `jsonl` (default) | Single-machine, human-inspectable files | Tens of thousands of threads |
| `sqlite` | Concurrent writers, bigger corpus, indexed queries | Millions of messages |
| `memory` | Tests / ephemeral / serverless | In-process only |
| `none` | Disable entirely | – |

Environment overrides: `JUGLANS_HISTORY_BACKEND`, `JUGLANS_HISTORY_DIR`, `JUGLANS_HISTORY_PATH`, `JUGLANS_HISTORY_MAX_MESSAGES`, `JUGLANS_HISTORY_MAX_TOKENS`, `JUGLANS_HISTORY_ENABLED`.

### Direct access from workflows

The [`history.*` builtins](../reference/builtins.md#conversation-history-history) — `history.load`, `history.append`, `history.replace`, `history.trim`, `history.clear`, `history.stats`, `history.list_chats` — let workflows inspect and rewrite the store. Useful for building memory-summary flows or "reset conversation" handlers.

## Troubleshooting

| Problem | Solution |
|---------|----------|
| `No API-key provided` | Set the relevant `*_API_KEY` env var or add `[ai.providers.<name>]` to `juglans.toml` |
| `401 Unauthorized` | Verify the api_key is valid and not expired |
| `Agent not found` | Confirm the agent node is defined inline or imported via `libs:` |
| `Timeout` | Check network access to the provider's API endpoint |
| History not persisting | Check `[history].enabled = true`, the configured path is writable, and the node's `state` is not `silent` / `display_only` |

## Next Steps

- [Configuration Reference](../reference/config.md) — Complete configuration options
- [Agent Syntax](../reference/agent-spec.md) — Inline agent configuration
