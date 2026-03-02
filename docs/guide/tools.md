# Tool Definition Files (Tools)

This guide introduces how to use tool definition files (`.json`) to manage and reuse AI tool configurations.

## What Are Tool Definition Files

Tool definition files allow you to store OpenAI Function Calling format tool definitions independently, making it easy to:

- **Modular management** - Separate tool definitions from business logic
- **Reuse** - Share the same tool set across multiple Agents and Workflows
- **Version control** - Track tool definition changes independently
- **Team collaboration** - Different team members maintain different tool sets

## File Format

### Basic Structure

```json
{
  "slug": "web-tools",
  "name": "Web Scraping Tools",
  "description": "Tools for fetching and parsing web content",
  "tools": [
    {
      "type": "function",
      "function": {
        "name": "fetch_url",
        "description": "Fetch web page content",
        "parameters": {
          "type": "object",
          "properties": {
            "url": {"type": "string", "description": "Target URL"}
          },
          "required": ["url"]
        }
      }
    }
  ]
}
```

### Field Descriptions

| Field | Type | Required | Description |
|------|------|------|------|
| `slug` | string | Yes | Unique identifier, used for referencing |
| `name` | string | Yes | Tool set name |
| `description` | string | No | Tool set description |
| `tools` | array | Yes | Tool definition array (OpenAI format) |

## Usage in Workflows

### 1. Importing Tool Definitions

Import tool definitions at the top of a workflow file:

```yaml
name: "My Workflow"

# Import tool definition files
tools: ["./tools/*.json"]
agents: ["./agents/*.jgagent"]
prompts: ["./prompts/*.jgprompt"]

entry: [start]
```

### 2. Referencing Tool Sets

#### Single Tool Set

```yaml
[step]: chat(
  agent="assistant",
  message=$input.query,
  tools="web-tools"  # Reference by slug
)
```

#### Multiple Tool Sets

```yaml
[step]: chat(
  agent="assistant",
  message=$input.query,
  tools=["web-tools", "data-tools"]  # Merge multiple tool sets
)
```

#### Inline JSON (Backward Compatible)

```yaml
[step]: chat(
  agent="assistant",
  message=$input.query,
  tools=[
    {
      "type": "function",
      "function": {"name": "custom_tool", ...}
    }
  ]
)
```

## Usage in Agents

### Agent Default Tools

Configure default tool sets in `.jgagent` files:

```yaml
slug: "web-agent"
model: "gpt-4o"
system_prompt: "You are a web scraping assistant."

# Single tool set
tools: "web-tools"

# Or multiple tool sets
tools: ["web-tools", "data-tools"]
```

The Agent's default tools are automatically attached to all `chat()` calls, unless explicitly overridden in the workflow.

## Built-in Developer Tools (devtools)

Juglans includes 6 Claude Code-style built-in developer tools, automatically registered under the `"devtools"` slug. No need to create JSON files -- just reference them directly.

### Usage in Agents

```yaml
slug: "code-assistant"
model: "deepseek-chat"
tools: ["devtools"]

# Can be combined with other tool sets
# tools: ["devtools", "web-tools"]
```

### Usage in Workflows

devtools can be called directly in nodes as built-in tools, without declaring them in the `tools:` field:

```yaml
# Call directly as nodes
[read]: read_file(file_path="./src/main.rs")
[search]: grep(pattern="TODO|FIXME", path="./src")
[review]: chat(agent="reviewer", message="Review:\n$read.output.content")
[read] -> [search] -> [review]
```

### Included Tools

| Tool | Description |
|------|------|
| `read_file` | Read a file, returns content with line numbers |
| `write_file` | Write a file, automatically creates parent directories |
| `edit_file` | Exact string replacement |
| `glob` | File pattern matching |
| `grep` | Regex search file contents |
| `bash` | Execute shell commands (alias: `sh`) |

For detailed parameters, see [Built-in Tools Reference](../reference/builtins.md#developer-tools).

## Priority Rules

```
Workflow inline JSON > Workflow reference > Agent default
```

Example:

```yaml
# src/agents/my-agent.jgagent
tools: "default-tools"

# workflow.jg
[step1]: chat(agent="my-agent", message="...")
# Uses "default-tools"

[step2]: chat(agent="my-agent", message="...", tools="override-tools")
# Uses "override-tools" (overrides)
```

## Tool Merging and Deduplication

When referencing multiple tool sets:

```yaml
tools: ["web-tools", "data-tools"]
```

The system will:
1. Load all tool sets
2. Merge all tool definitions
3. Deduplicate (tools with the same name are overridden by the latter)

```
web-tools: [fetch_url, parse_html]
data-tools: [calculate, fetch_url]  # fetch_url overrides the version from web-tools

Final: [parse_html, calculate, fetch_url]
```

## Examples

### Example 1: Web Scraping Tools

**tools/web-tools.json:**

```json
{
  "slug": "web-tools",
  "name": "Web Scraping Tools",
  "tools": [
    {
      "type": "function",
      "function": {
        "name": "fetch_url",
        "description": "Fetch web page content",
        "parameters": {
          "type": "object",
          "properties": {
            "url": {"type": "string"},
            "method": {"type": "string", "enum": ["GET", "POST"]}
          },
          "required": ["url"]
        }
      }
    }
  ]
}
```

**workflow.jg:**

```yaml
tools: ["./tools/*.json"]

[fetch]: chat(
  agent="assistant",
  message="Fetch https://example.com",
  tools="web-tools"
)
```

### Example 2: Combining Multiple Tool Sets

**tools/math-tools.json:**

```json
{
  "slug": "math-tools",
  "tools": [
    {
      "type": "function",
      "function": {
        "name": "calculate",
        "description": "Perform mathematical calculations"
      }
    }
  ]
}
```

**agents/analyst.jgagent:**

```yaml
slug: "analyst"
tools: ["web-tools", "math-tools"]  # Combined tools
```

## Best Practices

### 1. Naming Conventions

```
tools/
├── web-tools.json      # Named by functional category
├── data-tools.json
├── api-tools.json
└── custom-tools.json
```

### 2. Tool Granularity

- **Coarse-grained** - Group by functional domain (web-tools, data-tools)
- **Fine-grained** - Split by specific use case (github-tools, slack-tools)

Choose the granularity that suits your team.

### 3. Version Management

```bash
# Commit tool definitions to version control
git add tools/
git commit -m "feat: Add web scraping tools"
```

### 4. Documentation

Provide clear descriptions in tool definitions:

```json
{
  "slug": "api-tools",
  "description": "Tool set for connecting to external APIs, including authentication and data transformation",
  "tools": [...]
}
```

### 5. Testing

Create test workflows to verify tool definitions:

```yaml
name: "Test Web Tools"
tools: ["./tools/web-tools.json"]

[test]: chat(
  agent="assistant",
  message="Test fetch_url tool",
  tools="web-tools"
)
```

## Error Handling

### Tool Set Not Found

```yaml
tools: "nonexistent"  # Error
```

Error message:
```
Tool resource 'nonexistent' not found
```

**Solution:**
1. Check the slug spelling
2. Confirm the tool file has been imported
3. Review the loading logs

### Tool Definition Format Error

```json
{
  "slug": "bad-tools",
  "tools": "not an array"  // Error
}
```

**Solution:**
Check the JSON format and ensure `tools` is an array.

## Debugging

### View Loaded Tools

Enable debug logging:

```bash
DEBUG=true juglans workflow.jg
```

Output:
```
Loading tool definitions from 1 pattern(s)...
  Loaded 2 tool resource(s) with 5 total tools
Registered tool resource: web-tools
Registered tool resource: data-tools
```

### Tool Resolution Logs

```
Resolving tool reference: web-tools
Attaching 2 custom tools to the request.
```

## Related Documentation

- [Agent Configuration](./agent-syntax.md) - Agent default tool configuration
- [Workflow Syntax](./workflow-syntax.md) - Importing tool definitions
- [Built-in Tools](../reference/builtins.md) - chat() parameter descriptions
