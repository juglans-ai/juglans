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

Supported provider keys: `openai`, `anthropic`, `deepseek`, `gemini`, `qwen`, `byteplus`, `xai`.

Configuration file search order: `./juglans.toml` → `~/.config/juglans/juglans.toml` → `/etc/juglans/juglans.toml`.

You can also configure providers via environment variables (no `juglans.toml` needed):

```bash
export OPENAI_API_KEY="sk-..."
export ANTHROPIC_API_KEY="sk-ant-..."
export DEEPSEEK_API_KEY="sk-..."
export GEMINI_API_KEY="..."
export QWEN_API_KEY="sk-..."
export XAI_API_KEY="..."
export ARK_API_KEY="..."
```

Juglans will pick up any of these on startup.

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

## Troubleshooting

| Problem | Solution |
|---------|----------|
| `No API-key provided` | Set the relevant `*_API_KEY` env var or add `[ai.providers.<name>]` to `juglans.toml` |
| `401 Unauthorized` | Verify the api_key is valid and not expired |
| `Agent not found` | Confirm the agent node is defined inline or imported via `libs:` |
| `Timeout` | Check network access to the provider's API endpoint |

## Next Steps

- [Configuration Reference](../reference/config.md) — Complete configuration options
- [Agent Syntax](../reference/agent-spec.md) — Inline agent configuration
