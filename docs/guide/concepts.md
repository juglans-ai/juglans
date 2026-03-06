# Core Concepts

This guide introduces the core concepts and design philosophy of Juglans.

## Overview

Juglans is an AI workflow orchestration framework that organizes and executes complex AI tasks through three core resource types:

```
┌─────────────────────────────────────────────────────┐
│                    Workflow                          │
│                   (.jg)                          │
│                                                      │
│   ┌─────────┐    ┌─────────┐    ┌─────────┐        │
│   │  Node   │───▶│  Node   │───▶│  Node   │        │
│   └────┬────┘    └────┬────┘    └────┬────┘        │
│        │              │              │              │
│        ▼              ▼              ▼              │
│   ┌─────────┐    ┌─────────┐    ┌─────────┐        │
│   │  Agent  │    │ Prompt  │    │  Agent  │        │
│   └─────────┘    └─────────┘    └─────────┘        │
└─────────────────────────────────────────────────────┘
```

## Agent

An **Agent** is a configurable AI entity that defines:

- The model to use (e.g., GPT-4, DeepSeek)
- Behavioral characteristics (temperature, system prompt)
- Available capabilities (MCP tools, skills)

### Use Cases

- Encapsulate domain-specific AI capabilities
- Reuse consistent AI behavior configurations
- Enable multi-Agent collaboration

### Example

```jgagent
# src/agents/analyst.jgagent
slug: "analyst"
model: "gpt-4o"
temperature: 0.5
system_prompt: |
  You are a data analyst expert.
  Provide accurate, data-driven insights.
```

### Usage in Workflows

```juglans
[analyze]: chat(agent="analyst", message=$input.data)
```

---

## Prompt

A **Prompt** is a reusable prompt template that supports:

- Variable interpolation
- Conditional logic
- Loop iteration
- Filters

### Use Cases

- Separate prompt logic from workflow logic
- Reuse common prompt patterns
- Version control and iterative optimization

### Example

```jgprompt
# src/prompts/report.jgprompt
---
slug: "report"
inputs:
  data: {}
  format: "markdown"
---
Generate a {{ format }} report for:

{{ data | json }}

Include key findings and recommendations.
```

### Usage in Workflows

```juglans
[render]: p(slug="report", data=$ctx.results, format="html")
[generate]: chat(agent="writer", message=$output)
```

---

## Workflow

A **Workflow** is an execution graph that defines:

- Nodes (execution units)
- Edges (execution order and conditions)
- Entry and exit points

### Use Cases

- Orchestrate complex multi-step tasks
- Implement conditional branches and loops
- Compose multiple Agents and Prompts
- Compose multiple workflow files via `flows:`

### Example

```juglans
# src/pipeline.jg
name: "Data Pipeline"

entry: [start]
exit: [end]

[start]: notify(status="Starting...")
[process]: chat(agent="processor", message=$input.data)
[end]: notify(status="Done")

[start] -> [process] -> [end]
```

---

## Workflow Composition

When a single workflow becomes complex, you can use `flows:` to compose multiple `.jg` files into a unified execution graph:

```juglans
# main.jg — uses flows: to import sub-workflows
[start]: set_context(type=$input.type)
[route]: print(msg="Routing...")
[done]: print(msg="Complete")

[start] -> [route]
[route] if $ctx.type == "auth" -> [done]
[route] if $ctx.type == "trade" -> [done]
[route] -> [done]
```

```
┌─────────────────────────────────────────────────────────────┐
│  main.jg (parent workflow)                                  │
│                                                              │
│   [start] ──→ [route] ──→ [auth.start] ──→ ... ──→ [done]  │
│                   │                                    ↑     │
│                   └──→ [trading.start] ──→ ... ────────┘     │
│                                                              │
│         auth.* and trading.* nodes come from sub-workflows   │
└─────────────────────────────────────────────────────────────┘
```

Sub-workflow nodes are merged into the parent DAG with the alias as a namespace prefix. All nodes share the same execution context. Variable references inside sub-workflows are automatically prefixed with the namespace (only node reference variables; `$ctx`/`$input`/`$output` are unaffected).

See the [Workflow Composition Guide](./workflow-composition.md) for details.

---

## Execution Context

During workflow execution, a Context is maintained that stores:

- Input data (`$input`)
- Node output (`$output`)
- Custom variables (`$ctx`)
- Reply metadata (`$reply`)

### Variable Paths

```text
$input.field          # Input field
$output               # Current node output
$output.nested.field  # Nested access
$ctx.my_var           # Context variable
$reply.tokens         # Reply token count
```

### Variable Flow

```
                    Input
                      │
                      ▼
┌──────────────────────────────────────┐
│              Context                  │
│  $input: { query: "..." }            │
│  $ctx: {}                            │
│  $output: null                        │
└──────────────────────────────────────┘
                      │
          ┌───────────┴───────────┐
          ▼                       ▼
    ┌──────────┐           ┌──────────┐
    │  Node A  │           │  Node B  │
    │  $output │           │  $output │
    └────┬─────┘           └────┬─────┘
         │                      │
         └───────────┬──────────┘
                     ▼
┌──────────────────────────────────────┐
│              Context                  │
│  $input: { query: "..." }            │
│  $ctx: { result_a: ..., result_b: }  │
│  $output: (last node's output)       │
└──────────────────────────────────────┘
```

---

## Execution Model

### Graph Traversal

A workflow is a Directed Acyclic Graph (DAG), and the executor traverses it in topological order:

```
     [A]
    /   \
  [B]   [C]
    \   /
     [D]
```

Execution order: A → B (parallel C) → D

### Conditional Routing

```juglans
[router]: set_context(type=$input.type)
[path_a]: print(msg="Path A")
[path_b]: print(msg="Path B")
[default]: print(msg="Default")

[router] if $ctx.type == "a" -> [path_a]
[router] if $ctx.type == "b" -> [path_b]
[router] -> [default]
```

Only paths whose conditions are met will be executed.

### Error Handling

```juglans
[risky]: fetch_url(url=$input.url)
[success]: print(msg="OK")
[fallback]: print(msg="Error occurred")

[risky] -> [success]
[risky] on error -> [fallback]
```

The `on error` path is executed when a node fails.

---

## Resource Organization

### Recommended Project Structure

```
my-project/
├── juglans.toml              # Configuration
└── src/
    ├── main.jg               # Main workflow (.jg source files directly in src/)
    ├── sub-flow.jg           # Sub-workflow
    ├── workflows/            # .jgflow metadata
    │   └── main.jgflow
    ├── agents/               # Entry Agents (with workflow)
    │   └── my-agent.jgagent
    ├── pure-agents/          # Pure Agents (without workflow)
    │   └── assistant.jgagent
    ├── prompts/              # Prompt templates
    │   └── system.jgprompt
    └── tools/                # Tool definitions
        └── my-tools.json
```

### Resource References

**Relative path imports:**

```juglans
prompts: ["src/prompts/**/*.jgprompt"]
agents: ["src/agents/**/*.jgagent"]

[start]: print(msg="loaded")
```

**Reference by slug:**

```juglans
[node]: chat(agent="my-agent")     # Reference by slug
[node]: p(slug="my-prompt")       # Reference by slug
```

---

## Design Principles

### 1. Declarative over Imperative

Define "what" rather than "how":

```juglans
# Good: declarative
[classify]: chat(agent="classifier", format="json")
[handle_a]: print(msg="Handling A")
[handle_b]: print(msg="Handling B")

[classify] if $output.type == "A" -> [handle_a]
[classify] -> [handle_b]
```

### 2. Composition over Inheritance

Build complex functionality by composing small, focused resources:

```juglans
# Compose specialized Agents in a workflow pipeline
agents: ["agents/*.jgagent"]

[classify]: chat(agent="classifier", format="json")
[analyze]: chat(agent="analyzer", message=$output)
[write]: chat(agent="writer", message=$output)

[classify] -> [analyze] -> [write]
```

### 3. Separation of Concerns

- Prompt: content and formatting
- Agent: capabilities and behavior
- Workflow: process and logic

### 4. Testability

Each component can be tested independently:

```bash
# Test Prompt
juglans src/prompts/my-prompt.jgprompt --input '{...}'

# Test Agent
juglans src/agents/my-agent.jgagent --message "test"

# Test Workflow
juglans src/main.jg --input '{...}'
```

---

## Relationship with Jug0

Juglans is the DSL and local executor; Jug0 is the backend platform:

```
┌─────────────────┐     ┌─────────────────┐
│    Juglans      │     │      Jug0       │
│    (Local)      │────▶│    (Backend)    │
│                 │     │                 │
│  - DSL parsing  │     │  - LLM calls   │
│  - Workflow     │     │  - Resource     │
│    execution    │     │    storage      │
│  - Local dev    │     │  - API service  │
└─────────────────┘     └─────────────────┘
```

**Local mode:** Uses local files, suitable for development

**Remote mode:** Resources stored in Jug0, suitable for production

---

## Next Steps

- [Workflow Syntax](./workflow-syntax.md) - Detailed syntax reference
- [Prompt Syntax](./prompt-syntax.md) - Template syntax
- [Agent Syntax](./agent-syntax.md) - Agent configuration
- [Built-in Tools](../reference/builtins.md) - Available tools
