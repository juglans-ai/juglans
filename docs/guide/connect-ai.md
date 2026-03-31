# How to Connect AI Models

This guide covers how to configure Juglans to connect to AI models, including the Jug0 backend and local models.

## Configure Jug0

Juglans calls LLMs through the Jug0 backend. Configure it in `juglans.toml` at the project root:

```toml
[account]
id = "your_user_id"
api_key = "jug0_sk_your_api_key"

[jug0]
base_url = "http://localhost:3000"   # Local development
# base_url = "https://api.jug0.com" # Production
```

Configuration file search order: `./juglans.toml` -> `~/.config/juglans/juglans.toml` -> `/etc/juglans/juglans.toml`.

You can also override settings via environment variables:

```bash
export JUGLANS_API_KEY="jug0_sk_..."
export JUGLANS_JUG0_URL="http://localhost:3000"
```

## Get API Key

1. Log in to the Jug0 console
2. Go to Settings > API Keys
3. Create a new API Key (format: `jug0_sk_...`)
4. Copy it into the `[account].api_key` field in `juglans.toml`

## Test Connection

Create a minimal chat workflow to verify the connection:

```juglans
[assistant]: {
  "model": "gpt-4o",
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

If configured correctly, you will see the model's response. If it fails, check:

- Whether the `api_key` and `base_url` in `juglans.toml` are correct
- Whether the Jug0 backend is running (`curl http://localhost:3000/health`)
- Use `juglans whoami --check-connection` to test the connection status

## Use Local Models (Ollama)

Juglans supports connecting to local models through the Jug0 backend. Example Ollama configuration:

```toml
[jug0]
base_url = "http://localhost:3000"

# Set up the Ollama provider in the Jug0 backend configuration,
# then specify the model in your inline agent definition
```

Create an inline agent that uses a local model:

```juglans
[local_agent]: {
  "model": "ollama/llama3",
  "temperature": 0.7,
  "system_prompt": "You are a helpful assistant."
}

[ask]: chat(agent=local_agent, message=input.query)
[done]: print(message=output)

[local_agent] -> [ask] -> [done]
```

## Resource Management

Juglans resources (Workflows, Agents, Prompts) can be synchronized between local storage and Jug0.

### Push (Local -> Remote)

```bash
# Push a single file
juglans push src/prompts/greeting.jgx

# Force overwrite
juglans push src/main.jg --force

# Batch push (using workspace configuration)
juglans push

# Preview
juglans push --dry-run
```

### Pull (Remote -> Local)

```bash
juglans pull my-prompt --type prompt
juglans pull my-agent --type agent --output ./agents/
```

### List and Delete

```bash
juglans list                    # List all remote resources
juglans list --type agent       # List Agents only
juglans delete old-prompt --type prompt
```

## Local vs Remote Resources

| | Local | Remote |
|---|---|---|
| Reference style | Inline node or `libs:` import | owner/slug (e.g., `"juglans/assistant"`) |
| Requires import | Define inline or import via `libs:` | No, reference directly |
| Best for | Development, testing | Production deployment, team sharing |

Mix both in the same Workflow:

```juglans
[my_agent]: {
  "model": "gpt-4o",
  "system_prompt": "You are a helpful assistant."
}

[start]: print(msg="begin")
[local_chat]: chat(agent=my_agent, message=input.query)
[remote_chat]: chat(agent="juglans/premium-agent", message=output)
[end]: print(msg="done")

[my_agent] -> [start] -> [local_chat] -> [remote_chat] -> [end]
```

## Troubleshooting

| Problem | Solution |
|---------|----------|
| `Connection refused` | Confirm the Jug0 backend is running; check `base_url` |
| `401 Unauthorized` | Verify the `api_key` is correct |
| `Agent not found` | Confirm the agent node is defined inline or imported via `libs:` |
| `Timeout` | Increase the timeout configuration or check the network connection |

## Next Steps

- [Jug0 Integration](../integrations/jug0.md) -- Full API reference
- [Agent Syntax](../reference/agent-spec.md) -- Inline agent configuration
- [Configuration Reference](../reference/config.md) -- Complete configuration options
