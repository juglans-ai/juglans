# Tool Calling

Demonstrates how to use tools (Function Calling) in Agent configuration and workflows.

## Overview

Juglans supports two ways to configure tools:

1. **Agent level** - Configure default tools in `.jgagent` files
2. **Workflow level** - Dynamically specify tools in `chat()` calls

Workflow-level configuration overrides the Agent's default configuration.

## Example: Complex Problem Solver with Tools

### Workflow File

#### tool-router.jg

```juglans
name: "AI Router with Tooling"
description: "Route simple vs complex questions, use tools for complex ones"

prompts: ["./prompts/*.jgprompt"]
agents: ["./agents/*.jgagent"]

entry: [init]
exit: [final_notify]

[init]: notify(status="🔍 Analyzing your question...")

# Step 1: Complexity analysis (stateless, does not pollute conversation history)
[classify]: chat(
  agent="classifier",
  format="json",
  stateless="true",
  message=p(slug="router", user_msg=$input.message)
)

# Simple question: answer directly
[simple_reply]: chat(
  agent="assistant",
  chat_id=$reply.chat_id,
  message="The user just asked: '$input.message'. Please answer concisely based on context."
)

# Complex question notification
[complex_thinking]: notify(status="🧠 Complex question, activating tools...")

# Complex problem solving (with tools)
[complex_solver]: chat(
  agent="tool-agent",
  chat_id=$reply.chat_id,
  message=p(slug="solver", user_msg=$input.message),
  tools=[
    {
      "type": "function",
      "function": {
        "name": "fetch_url",
        "description": "Fetch the source code or text content of a webpage",
        "parameters": {
          "type": "object",
          "properties": {
            "url": {"type": "string", "description": "Full webpage URL"},
            "method": {"type": "string", "enum": ["GET", "POST"]}
          },
          "required": ["url"]
        }
      }
    },
    {
      "type": "function",
      "function": {
        "name": "calculate",
        "description": "Perform mathematical calculations",
        "parameters": {
          "type": "object",
          "properties": {
            "expression": {"type": "string", "description": "Mathematical expression, e.g. '2 + 2'"}
          },
          "required": ["expression"]
        }
      }
    }
  ]
)

[final_notify]: notify(status="✅ Processing complete")

# Routing logic
[init] -> [classify]

[classify] if $ctx.classify.output.complexity == "simple" -> [simple_reply]
[classify] if $ctx.classify.output.complexity == "complex" -> [complex_thinking]

[complex_thinking] -> [complex_solver]

[simple_reply] -> [final_notify]
[complex_solver] -> [final_notify]
```

## Agent Definitions

### src/agents/classifier.jgagent

```jgagent
slug: "classifier"
name: "Intent Classifier"
model: "gpt-3.5-turbo"
temperature: 0.0

system_prompt: |
  You are a question complexity classifier.

  Analyze the user's question and classify it as "simple" or "complex".

  - simple: General knowledge, greetings, basic questions
  - complex: Requires web search, calculations, or multi-step reasoning

  Respond with JSON:
  {"complexity": "simple" | "complex", "reason": "brief explanation"}
```

### src/agents/tool-agent.jgagent (with default tools)

```jgagent
slug: "tool-agent"
name: "Tool-enabled Agent"
model: "gpt-4o"
temperature: 0.7

system_prompt: |
  You are a helpful assistant with access to tools.

  When you need information from the web, use fetch_url.
  When you need to perform calculations, use calculate.

  Always explain your reasoning and the results from tool calls.

# Default tool configuration (can be overridden by workflows)
tools: [
  {
    "type": "function",
    "function": {
      "name": "search_knowledge",
      "description": "Search the knowledge base",
      "parameters": {
        "type": "object",
        "properties": {
          "query": {"type": "string", "description": "Search keywords"}
        },
        "required": ["query"]
      }
    }
  }
]
```

### src/agents/assistant.jgagent

```jgagent
slug: "assistant"
name: "General Assistant"
model: "gpt-3.5-turbo"
temperature: 0.7

system_prompt: |
  You are a helpful, friendly AI assistant.
  Answer questions clearly and concisely.
```

## Prompt Templates

### src/prompts/router.jgprompt

```jgprompt
slug: "router"
name: "Complexity Router Prompt"

template: |
  User question: {{ user_msg }}

  Classify the complexity of this question.
```

### src/prompts/solver.jgprompt

```jgprompt
slug: "solver"
name: "Complex Problem Solver Prompt"

template: |
  User asked a complex question: {{ user_msg }}

  Please analyze and solve this problem step by step.
  Use available tools when needed.
```

## Running Examples

### Simple Question

```bash
juglans tool-router.jg --input '{"message": "Who are you?"}'
```

Output:
```
🔍 Analyzing your question...
[classify] complexity: simple
The user just asked: 'Who are you?'. I am an AI assistant...
✅ Processing complete
```

### Complex Question (requires tools)

```bash
juglans tool-router.jg --input '{"message": "Help me check the latest updates on juglans.ai"}'
```

Output:
```
🔍 Analyzing your question...
[classify] complexity: complex
🧠 Complex question, activating tools...
[tool-agent] Calling fetch_url(url="https://juglans.ai")
[tool-agent] Based on the website content, the latest updates include...
✅ Processing complete
```

## Tool Configuration Priority

### Scenario 1: Using Agent Default Tools

```juglans
# Agent has default tools configured
[step]: chat(
  agent="tool-agent",
  message=$input
  # No tools specified, uses Agent's default tools
)
```

### Scenario 2: Workflow Overrides Tools

```juglans
# Tools specified in the workflow override Agent defaults
[step]: chat(
  agent="tool-agent",
  message=$input,
  tools=[
    {
      "type": "function",
      "function": {
        "name": "custom_tool",
        "description": "Custom tool"
      }
    }
  ]
  # The tools here replace the Agent's default configuration
)
```

### Scenario 3: No Tool Calls

```juglans
# Agent has no default tools, workflow doesn't specify any either
[step]: chat(
  agent="assistant",
  message=$input
  # Plain text conversation, no tool calls
)
```

## Tool Definition Format

Tool definitions follow the OpenAI Function Calling format:

```json
{
  "type": "function",
  "function": {
    "name": "tool_name",
    "description": "Clear description of the tool's functionality",
    "parameters": {
      "type": "object",
      "properties": {
        "param1": {
          "type": "string",
          "description": "Parameter description"
        },
        "param2": {
          "type": "number",
          "enum": [1, 2, 3],
          "description": "Enum value parameter"
        }
      },
      "required": ["param1"]
    }
  }
}
```

## Best Practices

1. **Agent default tools** - Configure commonly used tools for domain-specific Agents
2. **Workflow overrides** - Dynamically adjust available tools for specific tasks
3. **Tool descriptions** - Write clear tool descriptions to help the model understand when to use them
4. **Parameter validation** - Use `required` and type definitions to ensure correct parameters
5. **Stateless classification** - Use `stateless="true"` to prevent classifiers from polluting conversation history

## Directory Structure

```
tool-calling/
├── tool-router.jg
├── agents/
│   ├── classifier.jgagent
│   ├── tool-agent.jgagent
│   └── assistant.jgagent
└── prompts/
    ├── router.jgprompt
    └── solver.jgprompt
```

## Debugging Tool Calls

Enable verbose logging to view the tool calling process:

```bash
DEBUG=true juglans tool-router.jg --input '{"message": "question"}'
```

The output will include:
- Tool call requests
- Tool execution results
- Model responses
