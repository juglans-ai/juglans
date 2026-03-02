# Agent Configuration Syntax (.jgagent)

`.jgagent` files define AI Agent configuration, including model, behavior, and capabilities.

## Basic Structure

```yaml
slug: "agent_identifier"
name: "Display Name"
description: "A brief description of what this agent does"
model: "deepseek-chat"
temperature: 0.7
system_prompt: "You are a helpful assistant."
```

## Configuration Fields

| Field | Type | Required | Default | Description |
|------|------|------|--------|------|
| `slug` | string | Yes | - | Unique identifier |
| `name` | string | No | - | Display name |
| `description` | string | No | - | Agent description |
| `model` | string | No | gpt-4o | Model name |
| `temperature` | float | No | 0.7 | Temperature parameter (0-2) |
| `system_prompt` | string | No | - | System prompt (inline or referenced) |
| `tools` | array/string | No | - | Default tool configuration (JSON array or string) |
| `mcp` | array | No | [] | MCP server list |
| `skills` | array | No | [] | Skills list |
| `workflow` | string | No | - | Associated workflow file path |

## Model Configuration

### Supported Models

```yaml
# DeepSeek
model: "deepseek-chat"
model: "deepseek-coder"

# OpenAI
model: "gpt-4o"
model: "gpt-4-turbo"
model: "gpt-3.5-turbo"

# Anthropic
model: "claude-3-opus"
model: "claude-3-sonnet"
model: "claude-3-haiku"

# Local models (Ollama)
model: "llama3"
model: "codellama"
model: "mistral"
```

### Temperature Parameter

```yaml
temperature: 0.0    # Deterministic output
temperature: 0.7    # Balanced creativity (recommended)
temperature: 1.0    # More randomness
temperature: 2.0    # Highly random
```

## System Prompt

### Inline Method

```yaml
system_prompt: "You are a helpful assistant."

# Multi-line prompt
system_prompt: |
  You are a professional data analyst.

  Your responsibilities:
  - Analyze data accurately
  - Provide clear insights
  - Use proper formatting
```

### Referencing a Prompt File

```yaml
system_prompt: p(slug="system-analyst")
```

This looks up the template with `slug="system-analyst"` from the loaded Prompts and uses it as the system prompt.

## Tool Configuration

Agents can be configured with a default tool set. These tools are automatically attached to requests when calling `chat()` (unless the `tools` parameter is explicitly specified in the workflow).

### JSON Array Format

Define tools directly using a JSON array, no escaping needed:

```yaml
slug: "web-agent"
model: "gpt-4o"
tools: [
  {
    "type": "function",
    "function": {
      "name": "fetch_url",
      "description": "Fetch web page content",
      "parameters": {
        "type": "object",
        "properties": {
          "url": {"type": "string", "description": "Target URL"},
          "method": {"type": "string", "enum": ["GET", "POST"]}
        },
        "required": ["url"]
      }
    }
  },
  {
    "type": "function",
    "function": {
      "name": "parse_html",
      "description": "Parse HTML content",
      "parameters": {
        "type": "object",
        "properties": {
          "html": {"type": "string", "description": "HTML source code"},
          "selector": {"type": "string", "description": "CSS selector"}
        },
        "required": ["html"]
      }
    }
  }
]
```

### String Format

You can also use a JSON string:

```yaml
slug: "tool-agent"
model: "deepseek-chat"
tools: "[{\"type\":\"function\",\"function\":{\"name\":\"calculator\",\"description\":\"Perform mathematical calculations\"}}]"
```

### Slug Reference Format (Recommended)

Reference registered tool sets, with support for combining multiple toolboxes:

```yaml
slug: "code-agent"
model: "deepseek-chat"

# Use built-in developer tools
tools: ["devtools"]

# Or combine multiple tool sets
# tools: ["devtools", "web-tools", "data-tools"]
```

Where:
- `"devtools"` — 6 built-in developer tools (read_file, write_file, edit_file, glob, grep, bash), automatically available
- Other slugs (e.g., `"web-tools"`) — from `tools/*.json` files, need to be imported in the workflow via `tools: ["./tools/*.json"]`

### Tool Priority

When a `chat()` call in a workflow also specifies the `tools` parameter, the workflow configuration takes priority:

```yaml
# Agent configuration
slug: "my-agent"
tools: [{"type": "function", "function": {"name": "default_tool"}}]

# Usage in workflow
[step]: chat(
  agent="my-agent",
  message=$input,
  tools=[{"type": "function", "function": {"name": "override_tool"}}]  # This one will be used
)

# When tools is not specified, the Agent's default configuration is used
[step2]: chat(
  agent="my-agent",
  message=$input  # Will use the Agent's configured default_tool
)
```

## MCP Tool Integration

Configure Model Context Protocol servers to extend Agent capabilities:

```yaml
slug: "tool-agent"
model: "gpt-4o"
mcp:
  - "filesystem"     # File system operations
  - "github"         # GitHub operations
  - "database"       # Database operations
```

MCP servers need to be configured in `juglans.toml` (HTTP connection method):

```toml
[[mcp_servers]]
name = "filesystem"
base_url = "http://localhost:3001/mcp/filesystem"

[[mcp_servers]]
name = "github"
base_url = "http://localhost:3001/mcp/github"
token = "${GITHUB_TOKEN}"
```

**Note:** Juglans connects to MCP servers via HTTP/JSON-RPC. The MCP service must be started first. See the [MCP Integration Guide](../integrations/mcp.md) for details.

## Skills System

Add predefined skills to an Agent:

```yaml
slug: "skilled-agent"
model: "deepseek-chat"
skills:
  - "code_review"
  - "documentation"
  - "testing"
```

## Associated Workflow

Bind an Agent to a specific workflow:

```yaml
slug: "workflow-agent"
model: "gpt-4o"
workflow: "../complex-task.jg"
```

When a user interacts with this Agent, it can trigger the associated workflow execution.

## Complete Examples

### General Assistant

```yaml
slug: "assistant"
name: "General Assistant"
model: "deepseek-chat"
temperature: 0.7
system_prompt: |
  You are a helpful, harmless, and honest AI assistant.

  Guidelines:
  - Be concise and clear
  - Admit when you don't know something
  - Ask clarifying questions when needed
```

### Code Expert

```yaml
slug: "code-expert"
name: "Code Expert"
model: "deepseek-coder"
temperature: 0.3
system_prompt: |
  You are an expert software engineer with deep knowledge of:
  - Python, TypeScript, Rust, Go
  - System design and architecture
  - Best practices and design patterns

  When providing code:
  1. Write clean, readable code
  2. Include comments for complex logic
  3. Consider edge cases
  4. Suggest tests when appropriate
mcp:
  - "code-executor"
skills:
  - "code_review"
  - "refactoring"
```

### Data Analyst

```yaml
slug: "data-analyst"
name: "Data Analyst"
model: "gpt-4o"
temperature: 0.5
system_prompt: p(slug="analyst-system-prompt")
mcp:
  - "python-executor"
  - "chart-generator"
skills:
  - "data_visualization"
  - "statistical_analysis"
```

### Creative Writing

```yaml
slug: "creative-writer"
name: "Creative Writer"
model: "claude-3-opus"
temperature: 1.2
system_prompt: |
  You are a creative writing assistant with a talent for:
  - Storytelling and narrative
  - Poetry and prose
  - Marketing copy
  - Script writing

  Be imaginative, evocative, and original.
  Adapt your style to match the requested genre or tone.
```

### Router Agent

```yaml
slug: "router"
name: "Intent Router"
model: "gpt-3.5-turbo"
temperature: 0.0
system_prompt: |
  You are an intent classifier. Analyze the user's message and classify it.

  Categories:
  - technical: Programming, system, debugging questions
  - creative: Writing, design, artistic requests
  - analytical: Data, research, analysis tasks
  - general: General conversation, simple questions

  Respond with ONLY a JSON object:
  {"category": "...", "confidence": 0.0-1.0}
```

### Multi-Step Workflow Agent

```yaml
slug: "research-agent"
name: "Research Agent"
model: "gpt-4o"
temperature: 0.7
system_prompt: |
  You are a research assistant capable of:
  1. Breaking down complex questions
  2. Searching for information
  3. Synthesizing findings
  4. Providing cited conclusions
workflow: "../research-pipeline.jg"
mcp:
  - "web-search"
  - "document-reader"
```

## Usage in Workflows

### Basic Call

```yaml
[chat]: chat(agent="assistant", message=$input.question)
```

### Specifying Output Format

```yaml
[classify]: chat(
  agent="router",
  message=$input.query,
  format="json"
)
```

### Stateless Call

```yaml
[analyze]: chat(
  agent="analyst",
  message=$input.data,
  stateless="true"    # Not saved to conversation history
)
```

## Interactive Usage

Chat directly with an Agent:

```bash
juglans src/agents/assistant.jgagent
```

Pass an initial message:

```bash
juglans src/agents/assistant.jgagent --message "Hello, how are you?"
```

## Best Practices

1. **Define roles clearly** - Clearly define the Agent's role and capabilities in the system_prompt
2. **Appropriate temperature** - Choose temperature based on task type (low for analytical tasks, high for creative tasks)
3. **Modular design** - One Agent focuses on one domain
4. **Composable** - Design multiple Agents that can collaborate
5. **Test and validate** - Test Agent behavior with various inputs

## Debugging

### View Agent Configuration

```bash
juglans src/agents/my-agent.jgagent --info
```

### Verbose Logging

```bash
juglans src/agents/my-agent.jgagent --verbose
```
