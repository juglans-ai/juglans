# .jgagent Syntax Reference

Complete syntax specification for Juglans `.jgagent` agent configuration files.

## File Format

`.jgagent` files use a YAML-like key-value format. Each line follows `key: value` syntax. Comments begin with `#`.

```text
# Comment
slug: "identifier"
name: "Display Name"
model: "gpt-4o"
temperature: 0.7
system_prompt: "You are a helpful assistant."
```

---

## Field Reference

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `slug` | string | **Yes** | -- | Unique identifier for this agent |
| `name` | string | No | -- | Display name |
| `description` | string | No | -- | Human-readable description |
| `model` | string | No | `"gpt-4o"` | LLM model name |
| `temperature` | number | No | `0.7` | Sampling temperature (0.0 -- 2.0) |
| `system_prompt` | string / multiline / p() | No | -- | System prompt content |
| `tools` | JSON array / string list / string | No | -- | Default tool configuration |
| `mcp` | string list | No | `[]` | MCP server names to attach |
| `skills` | string list | No | `[]` | Skill identifiers |
| `source` | string | No | -- | Associated workflow file path (local execution) |
| `endpoint` | string | No | -- | Deployment endpoint URL (for jug0 push) |
| `username` | string | No | -- | @handle for this agent (auto-registers in jug0) |
| `avatar` | string | No | -- | Avatar image (local file path or URL) |
| `is_public` | boolean | No | `false` | Whether the agent is publicly visible |

---

## Field Details

### slug (required)

The unique identifier used to reference this agent in workflows and the registry.

```jgagent
slug: "my-assistant"
```

### name

Human-readable display name.

```jgagent
slug: "code-helper"
name: "Code Helper"
```

### description

A brief description of the agent's purpose.

```jgagent
slug: "analyst"
name: "Data Analyst"
description: "Specialized agent for data analysis and visualization"
```

### model

The LLM model to use. Any model string supported by your backend.

```jgagent
slug: "fast-agent"
model: "gpt-4o"
```

Common model values:

| Provider | Models |
|---|---|
| OpenAI | `gpt-4o`, `gpt-4-turbo`, `gpt-3.5-turbo` |
| Anthropic | `claude-3-opus`, `claude-3-sonnet`, `claude-3-haiku` |
| DeepSeek | `deepseek-chat`, `deepseek-coder` |
| Local (Ollama) | `llama3`, `codellama`, `mistral` |

### temperature

Controls randomness of outputs. Lower values produce more deterministic results.

```jgagent
slug: "precise-agent"
model: "gpt-4o"
temperature: 0.0
```

| Value | Use case |
|---|---|
| `0.0` | Deterministic, classification, extraction |
| `0.3` | Analytical, data processing |
| `0.7` | General conversation (default) |
| `1.0 -- 2.0` | Creative writing, brainstorming |

### system_prompt

The system prompt that defines the agent's behavior. Three formats are supported.

**Inline string:**

```jgagent
slug: "simple"
system_prompt: "You are a helpful assistant."
```

**Multiline block scalar (YAML `|` syntax):**

```jgagent
slug: "detailed"
system_prompt: |
  You are a professional analyst.

  Your responsibilities:
  - Analyze data accurately
  - Provide clear insights
  - Use proper formatting
```

The `|` marker starts a block scalar. Subsequent indented lines are preserved with their line breaks. The block ends when indentation returns to the key level.

**Prompt template reference:**

```jgagent
slug: "template-agent"
system_prompt: p(slug="analyst-system-prompt")
```

This renders the `.jgprompt` template identified by `slug="analyst-system-prompt"` and uses the result as the system prompt.

### tools

Default tools attached to every `chat()` request for this agent. Three formats are supported.

**JSON array (inline tool definitions):**

```jgagent
slug: "tool-agent"
model: "gpt-4o"
tools: [
  {
    "type": "function",
    "function": {
      "name": "search",
      "description": "Search the web",
      "parameters": {
        "type": "object",
        "properties": {
          "query": {"type": "string"}
        },
        "required": ["query"]
      }
    }
  }
]
```

**String list (slug references):**

```jgagent
slug: "dev-agent"
model: "gpt-4o"
tools: ["devtools", "web-tools"]
```

Built-in slug `"devtools"` provides 6 developer tools: `read_file`, `write_file`, `edit_file`, `glob`, `grep`, `bash`.

**Single string (JSON or slug reference):**

```jgagent
slug: "single-ref"
tools: "devtools"
```

### mcp

List of MCP (Model Context Protocol) server names. Servers must be configured in `juglans.toml`.

```jgagent
slug: "mcp-agent"
model: "gpt-4o"
mcp: ["filesystem", "github"]
```

### skills

List of skill identifiers to enable.

```jgagent
slug: "skilled"
model: "gpt-4o"
skills: ["code_review", "documentation", "testing"]
```

### source

Path to an associated workflow file (relative to the `.jgagent` file location). When set, the agent triggers this workflow on user interaction instead of a direct LLM call.

```jgagent
slug: "assistant"
name: "Assistant"
description: "AI assistant powered by workflow"
source: "../main.jg"
```

**Best practice: Workflow-bound agents vs Pure agents**

There are two styles of agent configuration:

| Style | Location | Has `source:` | Has `model:`/`system_prompt:` | Use case |
|-------|----------|---------------|-------------------------------|----------|
| **Workflow-bound** | `src/agents/` | Yes | No | Complex, multi-step agent behavior |
| **Pure** | `src/pure-agents/` | No | Yes | Single-task agents called inside workflows |

Workflow-bound agents delegate all behavior to their source workflow. The workflow uses `chat(agent="pure-agent")` with `p()` to render prompts dynamically, keeping system prompts in `.jgprompt` files rather than hardcoded in agent definitions.

Example project structure:

```
src/
├── main.jg                    # Workflow: the agent's brain
├── agents/
│   └── assistant.jgagent      # source: "../main.jg"
├── pure-agents/
│   └── helper.jgagent         # model + system_prompt
└── prompts/
    └── system.jgprompt        # System prompt template
```

```juglans
# main.jg — uses p() for prompt rendering
prompts: ["./prompts/*.jgprompt"]
agents: ["./pure-agents/*.jgagent"]

[respond]: chat(
  agent="helper",
  message=p(slug="system", user_message=$input.message)
)
```

### endpoint

Deployment URL. Used when pushing to jug0 to specify the request forwarding address.

```jgagent
slug: "deployed"
model: "gpt-4o"
endpoint: "https://api.example.com/agent"
system_prompt: "You are a deployed agent."
```

### username

The @handle for this agent. Auto-registers the handle in jug0 when pushed.

```jgagent
slug: "bot"
username: "my-bot"
system_prompt: "I am a bot."
```

### avatar

Avatar image path (local file or URL).

```jgagent
slug: "visual"
avatar: "./assets/avatar.png"
system_prompt: "I have an avatar."
```

### is_public

Controls visibility in the registry.

```jgagent
slug: "public-agent"
is_public: true
system_prompt: "I am publicly visible."
```

---

## Complete Examples

### General Assistant

```jgagent
slug: "assistant"
name: "General Assistant"
model: "gpt-4o"
temperature: 0.7
description: "A general-purpose helpful assistant"
system_prompt: |
  You are a helpful, harmless, and honest AI assistant.

  Guidelines:
  - Be concise and clear
  - Admit when you don't know something
  - Ask clarifying questions when needed
```

### Code Expert with Tools

```jgagent
slug: "code-expert"
name: "Code Expert"
model: "deepseek-coder"
temperature: 0.3
tools: ["devtools"]
system_prompt: |
  You are an expert software engineer.

  When providing code:
  1. Write clean, readable code
  2. Include comments for complex logic
  3. Consider edge cases
```

### Minimal Agent

```jgagent
slug: "minimal"
system_prompt: "You are a concise assistant. Answer in one sentence."
```

Only `slug` is required. All other fields use defaults (`model: "gpt-4o"`, `temperature: 0.7`).

---

## Usage in Workflows

Import agent files with the `agents:` metadata, then reference by slug in `chat()` calls.

### Basic Usage

```juglans
agents: ["./agents/*.jgagent"]

[ask]: chat(agent="assistant", message=$input.question)
```

### With Output Format

```juglans
agents: ["./agents/*.jgagent"]

[classify]: chat(
  agent="router",
  message=$input.query,
  format="json"
)
```

### Stateless Call

```juglans
agents: ["./agents/*.jgagent"]

[analyze]: chat(
  agent="analyst",
  message=$input.data,
  stateless="true"
)
```

The `stateless="true"` parameter prevents the message from being saved to conversation history.

### Multiple Agents in One Workflow

```juglans
agents: ["./agents/*.jgagent"]

[classify]: chat(
  agent="router",
  message=$input.query,
  format="json"
)

[tech]: chat(agent="code-expert", message=$input.query)
[general]: chat(agent="assistant", message=$input.query)
[done]: notify(status="done")

[classify] if $output.category == "technical" -> [tech]
[classify] -> [general]
[tech] -> [done]
[general] -> [done]
```

---

## CLI Usage

```bash
# Interactive chat with an agent
juglans agent.jgagent

# One-off message
juglans agent.jgagent --message "What is Rust?"

# View agent info
juglans agent.jgagent --info

# Validate syntax
juglans check agent.jgagent
```
