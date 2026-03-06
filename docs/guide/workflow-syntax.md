# Workflow Syntax (.jg)

`.jg` files define the structure and execution logic of workflows.

## File Structure

```text
# Metadata
name: "Workflow Name"
version: "0.1.0"
author: "Author Name"
description: "Workflow description"

# Resource imports
prompts: ["./prompts/*.jgprompt"]
agents: ["./agents/*.jgagent"]

# Entry and exit
entry: [start_node]
exit: [end_node]

# Node definitions
[node_id]: tool_call(param1="value1")

# Edge definitions
[A] -> [B]
```

## Metadata

| Field | Type | Required | Description |
|------|------|------|------|
| `name` | string | Yes | Workflow name |
| `version` | string | No | Version number |
| `author` | string | No | Author |
| `description` | string | No | Description |
| `flows` | object | No | Workflow import mapping (see [Workflow Composition](./workflow-composition.md)) |

## Resource Imports

Workflows can import local Prompt and Agent files, and can also reference remote resources.

### Local Resource Imports

Use glob patterns to import local files:

```text
# Relative path (relative to the directory containing the .jg file)
prompts: ["./prompts/*.jgprompt"]
agents: ["./agents/*.jgagent"]

# Multiple paths
prompts: [
  "./local/*.jgprompt",
  "./shared/*.jgprompt"
]

# Single file
agents: ["./agents/main-agent.jgagent"]

# Absolute path
prompts: ["/absolute/path/to/prompts/*.jgprompt"]
```

**Path resolution rules:**
- Relative paths: relative to the directory containing the `.jg` file
- Absolute paths: paths starting with `/`
- Glob wildcards: `*` matches filenames, `**` matches subdirectories

### Workflow Imports

Use `flows:` to merge nodes from other `.jg` files into the current workflow, enabling cross-file branching:

```juglans
flows: {
  auth: "./auth.jg"
  trading: "./trading.jg"
}

entry: [route]
exit: [next_step]

[route]: print(msg="routing")
[next_step]: print(msg="done")

# Reference sub-workflow nodes
[route] if $ctx.need_auth -> [auth.start]
[auth.done] -> [next_step]
```

Sub-workflow nodes are merged into the parent DAG with the alias as a namespace prefix (e.g., `auth.start`, `auth.verify`), and variable references are automatically transformed. See the [Workflow Composition Guide](./workflow-composition.md) for details.

### Local vs Remote Resources

Imported local resources can be referenced by slug:

```juglans
# Import local Agent
agents: ["./agents/*.jgagent"]

# Reference local Agent (by slug)
[chat]: chat(agent="my-local-agent", message=$input)
```

To reference remote (Jug0) resources, use the `owner/slug` format:

```juglans
# No import needed, directly reference remote resources
[chat]: chat(agent="juglans/premium-agent", message=$input)
[render]: p(slug="owner/shared-prompt", data=$input)
```

### Mixed Usage

```juglans
# Import local resources
prompts: ["./prompts/*.jgprompt"]
agents: ["./agents/*.jgagent"]

entry: [start]
exit: [end]

[start]: print(msg="begin")

# Use local Agent
[local_chat]: chat(agent="my-agent", message=$input)

# Use remote Agent
[remote_chat]: chat(agent="juglans/cloud-agent", message=$output)

[end]: print(msg="done")

[start] -> [local_chat] -> [remote_chat] -> [end]
```

## Entry and Exit

Define the starting and ending points of a workflow:

```juglans
entry: [start]           # Single entry
exit: [end]              # Single exit

# Multiple exits (for branched results)
exit: [success, failure]
```

## Node Definitions

### Basic Syntax

```juglans
[node_id]: tool_name(param1=value1, param2=value2)
```

### Node ID Rules

- Use letters, numbers, underscores, and hyphens
- Must be enclosed in square brackets
- Case-sensitive

```text
[start]              # Valid
[process_data]       # Valid
[step-1]             # Valid
[MyNode]             # Valid
```

In edge definitions, you can also use namespaced format to reference imported sub-workflow nodes:

```text
[auth.start]         # Reference the start node of the auth sub-workflow
[trading.done]       # Reference the done node of the trading sub-workflow
```

Namespaced nodes are produced by `flows:` imports. See [Workflow Composition](./workflow-composition.md) for details.

### Tool Calls

```juglans
# String parameters
[node]: notify(status="Processing...")

# Variable references
[node]: chat(message=$input.text)

# Nested objects
[node]: chat(
  agent="assistant",
  message=$input.question,
  format="json"
)

# Array parameters
[node]: some_tool(items=["a", "b", "c"])
```

### Literal Nodes

```juglans
# String literal
[message]: "Hello, World!"

# JSON literal
[config]: {
  "model": "gpt-4",
  "temperature": 0.7
}
```

## Edge Definitions

### Simple Connections

```juglans
[A]: print(msg="a")
[B]: print(msg="b")
[C]: print(msg="c")

[A] -> [B]              # Execute B after A completes
[A] -> [B] -> [C]       # Chained connection
```

### Conditional Branches

```juglans
[router]: print(msg="routing")
[simple_handler]: print(msg="simple")
[complex_handler]: print(msg="complex")
[node]: print(msg="scoring")
[high_score]: print(msg="high")
[low_score]: print(msg="low")
[check]: print(msg="checking")
[proceed]: print(msg="proceed")
[reject]: print(msg="reject")

# Expression-based conditions
[router] if $ctx.type == "simple" -> [simple_handler]
[router] if $ctx.type == "complex" -> [complex_handler]

# Comparison operators
[node] if $output.score > 0.8 -> [high_score]
[node] if $output.score <= 0.8 -> [low_score]

# Boolean values
[check] if $ctx.is_valid -> [proceed]
[check] if !$ctx.is_valid -> [reject]
```

**Cross-workflow branching:** Conditional edge targets can be sub-workflow nodes:

```juglans
flows: {
  auth: "./auth.jg"
  trading: "./trading.jg"
}

entry: [route]
exit: [done]

[route]: print(msg="routing")
[done]: print(msg="done")

[route] if $output.type == "auth" -> [auth.start]
[route] if $output.type == "trade" -> [trading.start]
[auth.done] -> [done]
[trading.done] -> [done]
```

**Branch convergence behavior:** When multiple conditional branches converge on the same node, **OR semantics** are used (the node executes as soon as any one predecessor completes), not AND semantics (waiting for all predecessors). Unexecuted branches are automatically marked as unreachable. See the [Conditionals Guide](./conditionals.md#branch-convergence-semantics) for details.

### Switch Routing

Multi-branch selection that executes only one matching branch (unlike conditional edges):

```juglans
[classify]: chat(agent="classifier", message=$input)
[answer]: print(msg="answering")
[execute]: print(msg="executing")
[fallback]: print(msg="fallback")

# switch block: only one branch is taken
[classify] -> switch $output.intent {
    "question": [answer]
    "task": [execute]
    default: [fallback]
}
```

**Syntax notes**:
- `switch $variable { ... }` - Match based on variable value
- Each case format: `"value": [target_node]`
- `default:` - Handles unmatched cases (optional but recommended)

**Differences from conditional edges**:

| Feature | Conditional edges (`if`) | Switch |
|------|--------------|--------|
| Branches executed | Can execute multiple simultaneously | Executes only one |
| Syntax | Multiple lines | Single block |
| Default handling | Requires an extra edge | `default` keyword |

```juglans
[node]: print(msg="start")
[path_a]: print(msg="a")
[path_b]: print(msg="b")
[default]: print(msg="default")
[path_default]: print(msg="path_default")

# Conditional edges: may execute multiple simultaneously
[node] if $ctx.a -> [path_a]
[node] if $ctx.b -> [path_b]  # Both a and b may execute
[node] -> [default]            # Unconditional edge also executes

# Switch: executes only the first match
[node] -> switch $ctx.type {
    "a": [path_a]
    "b": [path_b]
    default: [path_default]    # Only one of these three will execute
}
```

---

### Error Handling

```juglans
[risky_operation]: print(msg="risky")
[error_handler]: print(msg="error")
[api_call]: print(msg="api")
[process]: print(msg="process")
[fallback]: print(msg="fallback")

# Jump on error
[risky_operation] on error -> [error_handler]

# Combined usage
[api_call] -> [process]
[api_call] on error -> [fallback]
```

### Default Path

```juglans
[router]: print(msg="routing")
[path_a]: print(msg="a")
[path_b]: print(msg="b")
[default_path]: print(msg="default")

# Default path when no conditions are met
[router] if $ctx.a == 1 -> [path_a]
[router] if $ctx.b == 1 -> [path_b]
[router] -> [default_path]          # Default
```

## Function Definitions

Define reusable, parameterized node blocks with `[name(params)]: { ... }`.

Functions are **not** added to the main DAG — they exist as callable templates. When a node calls a function by name, the executor binds arguments to the function's parameter variables and executes its body sub-graph.

### Single-Step Function

```juglans
# Define
[greet(name)]: bash(command="echo Hello, " + $name)

# Call
[step1]: greet(name="world")
```

### Multi-Step Function (Block Body)

Use `{ ... }` to define a function with multiple sequential steps:

```juglans
# Define: steps run in sequence (__0 -> __1)
[build(dir)]: {
  bash(command="cd " + $dir + " && make")
  bash(command="cd " + $dir + " && make test")
}

# Call
[step1]: build(dir="/app")
```

Steps can be separated by newlines or semicolons:

```text
[pipeline(a, b)]: { bash(command=$a); bash(command=$b) }
```

### Multiple Parameters

```juglans
[deploy(env, version)]: {
  bash(command="docker build -t app:" + $version + " .")
  bash(command="docker push app:" + $version)
  bash(command="kubectl set image deployment/app app=app:" + $version + " --namespace=" + $env)
}

[staging]: deploy(env="staging", version="1.2.0")
[production]: deploy(env="production", version="1.2.0")

[staging] -> [production]
```

### How It Works

1. **Parser**: `[name(params)]` detected → stored in `workflow.functions`, not in main graph
2. **Executor**: When a node calls `greet(name="world")`, it checks the function registry first
3. **Parameter binding**: Arguments are set as context variables (`$name`, `$dir`, etc.)
4. **Body execution**: The internal sub-graph runs sequentially
5. **Return**: Function returns `$output` from its last step

### Function vs Regular Node

| | Regular Node | Function Definition |
|---|---|---|
| Syntax | `[id]: tool(...)` | `[id(params)]: tool(...)` or `[id(params)]: { ... }` |
| In main DAG | Yes | No (stored separately) |
| Callable | No | Yes, by name |
| Parameters | — | Bound to `$param` variables |

---

## Loop Constructs

### While Loop

```juglans
[loop]: while($ctx.count < 10) {
  [increment]: set_context(count=$ctx.count + 1)
  [process]: chat(agent="worker", message="Item " + $ctx.count)
  [increment] -> [process]
}
```

### Foreach Loop

```juglans
[process_items]: foreach($item in $input.items) {
  [handle]: chat(agent="processor", message=$item.content)
  [save]: set_context(results=append($ctx.results, $output))
  [handle] -> [save]
}
```

### Loop Context Variables

Available inside loops:

| Variable | Description |
|------|------|
| `loop.index` | Current index (0-based) |
| `loop.first` | Whether this is the first iteration |
| `loop.last` | Whether this is the last iteration |

## Variable References

### Path Syntax

```text
$input.field           # Input variable
$output                # Current node output
$output.nested.field   # Nested access
$ctx.variable          # Context variable
$reply.content         # Last reply content
```

### Usage in Tool Calls

```juglans
[step1]: chat(message=$input.question)
[step2]: p(slug="template", data=$output)
[step3]: notify(status="Result: " + $output.summary)
```

## Complete Examples

### Simple Chat

```juglans
name: "Simple Chat"
version: "0.1.0"

agents: ["./agents/*.jgagent"]

entry: [start]
exit: [end]

[start]: notify(status="Chat started")
[chat]: chat(agent="assistant", message=$input.message)
[end]: notify(status="Chat ended")

[start] -> [chat] -> [end]
```

### Workflow with Routing

```juglans
name: "Smart Router"
version: "0.1.0"

prompts: ["./prompts/*.jgprompt"]
agents: ["./agents/*.jgagent"]

entry: [classify]
exit: [done]

# Classification node
[classify]: chat(
  agent="classifier",
  message=$input.query,
  format="json"
)

# Processing branches
[technical]: chat(agent="tech-expert", message=$input.query)
[general]: chat(agent="general-assistant", message=$input.query)
[creative]: chat(agent="creative-writer", message=$input.query)

# Completion node
[done]: notify(status="Query processed")

# Routing logic
[classify] if $output.category == "technical" -> [technical]
[classify] if $output.category == "creative" -> [creative]
[classify] -> [general]

[technical] -> [done]
[general] -> [done]
[creative] -> [done]
```

### Batch Processing

```juglans
name: "Batch Processor"
version: "0.1.0"

agents: ["./agents/*.jgagent"]

entry: [init]
exit: [summary]

[init]: set_context(results=[])

[process]: foreach($item in $input.items) {
  [analyze]: chat(
    agent="analyzer",
    message=$item.content
  )
  [collect]: set_context(
    results=append($ctx.results, {
      "id": $item.id,
      "result": $output
    })
  )
  [analyze] -> [collect]
}

[summary]: chat(
  agent="summarizer",
  message="Summarize these results: " + json($ctx.results)
)

[init] -> [process] -> [summary]
```

### With Error Handling

```juglans
name: "Robust Workflow"
version: "0.1.0"

entry: [start]
exit: [success, failure]

[start]: notify(status="Starting...")

[risky_call]: chat(
  agent="external-api",
  message=$input.data
)

[process]: p(slug="process-result", data=$output)

[success]: notify(status="Completed successfully")
[failure]: notify(status="Failed, using fallback")

[start] -> [risky_call] -> [process] -> [success]
[risky_call] on error -> [failure]
```

## Best Practices

1. **Clear naming** - Use descriptive node IDs
2. **Modularize** - Use `flows:` to split complex logic into multiple workflow files (see [Workflow Composition](./workflow-composition.md))
3. **Error handling** - Add `on error` paths for critical nodes
4. **Comments** - Use `#` to add explanatory comments
5. **Version control** - Use the `version` field to track changes
