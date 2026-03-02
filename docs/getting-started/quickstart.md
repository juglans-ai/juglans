# Quick Start

This guide will help you create and run your first Juglans workflow in 5 minutes.

## Prerequisites

- Rust 1.70+ (for compilation)
- An LLM API Key (DeepSeek, OpenAI, etc.)

## 1. Install Juglans

```bash
# Clone the repository
git clone https://github.com/juglans-ai/juglans.git
cd juglans

# Build
cargo build --release

# Add to PATH (optional)
export PATH="$PATH:$(pwd)/target/release"
```

## 2. Initialize a Project

```bash
# Create a new project
juglans init my-ai-project
cd my-ai-project
```

This creates the following structure:
```
my-ai-project/
├── juglans.toml        # Configuration file
└── src/                # All source files
    ├── prompts/        # Prompt templates
    ├── agents/         # Agent configurations (entry agents with workflow field)
    ├── pure-agents/    # Pure agents (no workflow, called by workflows)
    ├── workflows/      # .jgflow metadata manifests
    └── tools/          # Tool definitions
```
.jg source files are placed directly in the `src/` root directory.

## 3. Configure the API

Edit `juglans.toml`:

```toml
[account]
id = "your_user_id"
api_key = "your_api_key"

[jug0]
base_url = "http://localhost:3000"  # Or your Jug0 service address
```

## 4. Create an Agent

Create `src/agents/assistant.jgagent`:

```yaml
slug: "assistant"
name: "AI Assistant"
model: "deepseek-chat"
temperature: 0.7
system_prompt: |
  You are a helpful AI assistant.
  Be concise and accurate in your responses.
```

## 5. Create a Prompt Template

Create `src/prompts/analyze.jgprompt`:

```yaml
---
slug: "analyze"
name: "Analysis Prompt"
description: "Analyze user input and provide a structured response"
inputs:
  topic: ""
  style: "professional"
---
Please analyze the following topic: {{ topic }}

Requirements:
- Style: {{ style }}
- Provide key insights
- Be structured and clear

{% if style == "casual" %}
Feel free to use informal language.
{% endif %}
```

## 6. Create a Workflow

Create `src/main.jg`:

```yaml
# Import resources
prompts: ["./prompts/*.jgprompt"]
agents: ["./agents/*.jgagent"]

# Entry and exit nodes
entry: [init]
exit: [complete]

# Node definitions
[init]: notify(status="Starting analysis...")

[render_prompt]: p(
  slug="analyze",
  topic=$input.topic,
  style=$input.style
)

[analyze]: chat(
  agent="assistant",
  message=$output
)

[complete]: notify(status="Analysis complete!")

# Execution flow
[init] -> [render_prompt] -> [analyze] -> [complete]
```

## 7. Run the Workflow

```bash
# Run the workflow
juglans src/main.jg --input '{
  "topic": "AI workflow orchestration",
  "style": "professional"
}'
```

Example output:
```
[init] Starting analysis...
[render_prompt] Rendered prompt: analyze
[analyze] Calling agent: assistant
[analyze] Response: AI workflow orchestration is a systematic approach...
[complete] Analysis complete!
```

## 8. Interactive Agent

Chat directly with an Agent:

```bash
juglans src/agents/assistant.jgagent
```

Entering interactive mode:
```
> What is Juglans?
Juglans is a Rust-based AI workflow orchestration framework...

> Tell me more about its features
Juglans offers several key features:
1. Declarative DSL for defining workflows
2. Support for conditional branching and loops
...

> exit
Goodbye!
```

## Next Steps

- [Core Concepts](../guide/concepts.md) - Deep dive into Agent, Prompt, and Workflow
- [Workflow Syntax](../guide/workflow-syntax.md) - Complete .jg syntax
- [Built-in Tools](../reference/builtins.md) - Detailed guide to chat, p, notify, and other tools
- [Conditionals and Branching](../guide/conditionals.md) - Implementing complex logic

## FAQ

### Q: How do I debug a workflow?

Use the `--verbose` flag to see detailed logs:
```bash
juglans src/main.jg --verbose
```

### Q: How do I use a local model?

Configure a local endpoint in `juglans.toml`:
```toml
[jug0]
base_url = "http://localhost:11434/v1"  # Ollama example
```

### Q: Which models are supported?

Any model compatible with the OpenAI API is supported:
- DeepSeek (deepseek-chat, deepseek-coder)
- OpenAI (gpt-4o, gpt-4-turbo)
- Anthropic (claude-3-opus, claude-3-sonnet)
- Local models (Ollama, vLLM)
