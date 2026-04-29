# Agent Syntax Reference

Agents are defined as **inline JSON map nodes** in `.jg` workflow files — they live alongside the workflow that uses them, and can be exported for reuse via `libs:` imports (see below).

## Inline Agent Syntax

An agent is a regular node whose body is a JSON object containing model configuration:

```juglans
[my_agent]: {
  "model": "gpt-4o-mini",
  "system_prompt": "You are a helpful assistant.",
  "temperature": 0.7
}

[ask]: chat(agent=my_agent, message="Hello!")
[my_agent] -> [ask]
```

The node ID becomes the agent's identifier. Reference it by name (without quotes) in `chat(agent=...)`.

---

## Field Reference

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `model` | string | No | `"gpt-4o"` | LLM model name |
| `system_prompt` | string | No | -- | System prompt content |
| `temperature` | number | No | `0.7` | Sampling temperature (0.0 -- 2.0) |
| `tools` | JSON array / string list | No | -- | Default tool configuration |

> MCP servers are configured per-call on `chat()` via the `mcp` parameter — see [`chat()` in builtins](./builtins.md#chat). The agent inline map does not carry an `mcp` field.

---

## Field Details

### model

The LLM model to use. Any model string supported by your backend.

```juglans
[fast_agent]: {
  "model": "gpt-4o-mini"
}
```

### Model strings

Juglans does not maintain its own model catalog — every provider's model IDs come from the provider's own API and change frequently. The model string is passed verbatim to the upstream provider.

Two equivalent forms work:

```juglans
# Bare model — provider inferred from the prefix or from `[ai].default_model`
[a]: { "model": "gpt-4o-mini" }
[b]: { "model": "claude-haiku-4-5" }

# Explicit `provider/model` — picks the provider before the slash
[c]: { "model": "openai/llama3" }            # local Ollama via OpenAI base_url
[d]: { "model": "claude-code/sonnet" }       # local Claude Code CLI
[e]: { "model": "juglans/deepseek-chat" }    # routed through juglans-wallet proxy
```

Provider prefixes Juglans understands: `openai`, `anthropic` (also `claude`), `deepseek`, `gemini`, `qwen`, `byteplus` (also `ark`), `xai`, `claude-code`, `juglans`. See the [Connect AI Models guide](../guide/connect-ai.md) for the full provider list and how to configure each.

Pin specific model IDs against each provider's own docs (OpenAI / Anthropic / DeepSeek / Gemini / Qwen / xAI / ByteDance Ark) — the IDs change every few months and any list shipped here would be stale before this release lands.

### temperature

Controls randomness of outputs. Lower values produce more deterministic results.

```juglans
[precise_agent]: {
  "model": "gpt-4o-mini",
  "temperature": 0.0
}
```

| Value | Use case |
|---|---|
| `0.0` | Deterministic, classification, extraction |
| `0.3` | Analytical, data processing |
| `0.7` | General conversation (default) |
| `1.0 -- 2.0` | Creative writing, brainstorming |

### system_prompt

The system prompt that defines the agent's behavior.

```juglans
[simple]: {
  "model": "gpt-4o-mini",
  "system_prompt": "You are a helpful assistant."
}
```

For multi-line system prompts, use standard JSON string escaping (`\n`):

```juglans
[detailed]: {
  "model": "gpt-4o-mini",
  "system_prompt": "You are a professional analyst.\n\nYour responsibilities:\n- Analyze data accurately\n- Provide clear insights\n- Use proper formatting"
}
```

### tools

Default tools attached to every `chat()` request for this agent.

```juglans
[dev_agent]: {
  "model": "gpt-4o-mini",
  "tools": ["devtools"]
}
```

Built-in slug `"devtools"` is auto-populated with every builtin that implements `schema()` — this includes the 6 developer tools (`read_file`, `write_file`, `edit_file`, `glob`, `grep`, `bash`), `http_request`, and any other schema-registered builtins.

---

## Complete Examples

### General Assistant

```juglans
[assistant]: {
  "model": "gpt-4o-mini",
  "temperature": 0.7,
  "system_prompt": "You are a helpful, harmless, and honest AI assistant.\n\nGuidelines:\n- Be concise and clear\n- Admit when you don't know something\n- Ask clarifying questions when needed"
}

[ask]: chat(agent=assistant, message=input.question)
[assistant] -> [ask]
```

### Code Expert with Tools

```juglans
[code_expert]: {
  "model": "deepseek-coder",
  "temperature": 0.3,
  "tools": ["devtools"],
  "system_prompt": "You are an expert software engineer.\n\nWhen providing code:\n1. Write clean, readable code\n2. Include comments for complex logic\n3. Consider edge cases"
}

[ask]: chat(agent=code_expert, message=input.question)
[code_expert] -> [ask]
```

### Minimal Agent

```juglans
[minimal]: {
  "system_prompt": "You are a concise assistant. Answer in one sentence."
}

[ask]: chat(agent=minimal, message=input.question)
[minimal] -> [ask]
```

All fields are optional. Omitted fields use defaults (`model: "gpt-4o-mini"`, `temperature: 0.7`).

---

## Usage in Workflows

### Basic Usage

```juglans
[assistant]: {
  "model": "gpt-4o-mini",
  "system_prompt": "You are a helpful assistant."
}

[ask]: chat(agent=assistant, message=input.question)
[assistant] -> [ask]
```

### With Output Format

```juglans
[router]: {
  "model": "gpt-4o-mini",
  "temperature": 0.0,
  "system_prompt": "Classify user intent. Return JSON with 'intent' field."
}

[classify]: chat(agent=router, message=input.query, format="json")
[router] -> [classify]
```

### Multiple Agents in One Workflow

```juglans
[router]: {
  "model": "gpt-4o-mini",
  "temperature": 0.0,
  "system_prompt": "Classify intent as 'technical' or 'general'. Return JSON."
}

[code_expert]: {
  "model": "deepseek-coder",
  "temperature": 0.3,
  "system_prompt": "You are a code expert."
}

[assistant]: {
  "model": "gpt-4o-mini",
  "system_prompt": "You are a general assistant."
}

[classify]: chat(agent=router, message=input.query, format="json")
[tech]: chat(agent=code_expert, message=input.query)
[general]: chat(agent=assistant, message=input.query)
[done]: notify(status="done")

[router] -> [classify]
[code_expert] -> [tech]
[assistant] -> [general]

[classify] if output.category == "technical" -> [tech]
[classify] -> [general]
[tech] -> [done]
[general] -> [done]
```

---

## Cross-Workflow Agent Reuse

Define agents in a library file and import them with `libs:`:

```juglans
# agents.jg — agent library
[assistant]: {
  "model": "gpt-4o-mini",
  "system_prompt": "You are a helpful assistant."
}

[classifier]: {
  "model": "gpt-4o-mini",
  "temperature": 0.0,
  "system_prompt": "Classify user intent. Return JSON."
}
```

```juglans
# main.jg — uses agents from library
libs: ["./agents.jg"]

[ask]: chat(agent=agents.assistant, message=input.query)
[classify]: chat(agent=agents.classifier, message=input.text, format="json")
```

This pattern keeps agent definitions centralized and reusable across multiple workflows.
