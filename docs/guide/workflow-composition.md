# Workflow Composition (Flow Imports)

Through `flows:` declarations, you can compose multiple `.jg` files into a unified execution graph. Subworkflow nodes are merged into the parent DAG with namespace prefixes, enabling free cross-file branching design.

## Basic Syntax

### Declaring Imports

Use the `flows:` object mapping in the metadata section to declare subworkflows to import:

```juglans
flows: {
  auth: "./auth.jg"
  trading: "./trading.jg"
}

[start]: print(msg="Importing flows")
```

Keys are aliases, and values are relative paths (relative to the directory of the current `.jg` file).

### Referencing Subworkflow Nodes

Use the `[alias.node_id]` format to reference nodes in subworkflows:

```juglans
# Cross-flow references use alias.node_id format
[route]: set_context(need_auth=$input.need_auth)
[auth_start]: print(msg="Auth started")
[next_step]: print(msg="Next step")

[route] if $ctx.need_auth -> [auth_start]
[auth_start] -> [next_step]
```

### Minimal Complete Example

```juglans
name: "Main Router"
prompts: ["./prompts/*.jgprompt"]
agents: ["./agents/*.jgagent"]

flows: {
  trading: "./trading.jg"
  events: "./events.jg"
}

[start]: set_context(event_type=$input.event_type, message=$input.message)
[route]: set_context(routed=true)
[done]: reply(output=$output)

[start] -> [route]
[route] if $ctx.event_type -> [events.start]
[route] if $ctx.message -> [trading.start]

[events.respond] -> [done]
[trading.done] -> [done]
```

---

## Variable Namespacing

Variable references inside subworkflows are automatically prefixed with the namespace. The rule is: **only variables whose first segment matches a subworkflow internal node ID get prefixed**; other variables (`$ctx`, `$input`, `$output`, etc.) remain unchanged.

### Transformation Rules

Assume the `auth` subworkflow has two internal nodes, `verify` and `extract`:

| Original Variable (inside subworkflow) | Variable After Merging | Description |
|--------------------------|-----------|------|
| `$verify.output` | `$auth.verify.output` | `verify` is a subflow node, prefix added |
| `$extract.output.intent` | `$auth.extract.output.intent` | `extract` is a subflow node, prefix added |
| `$ctx.some_var` | `$ctx.some_var` | `ctx` is not a node, unchanged |
| `$input.message` | `$input.message` | `input` is not a node, unchanged |
| `$output` | `$output` | Unchanged |

### Referencing from the Parent Workflow

The parent workflow can access subworkflow node outputs through namespace paths:

```juglans
# Access subworkflow node outputs via namespace
[next]: chat(message=$ctx.auth_result)
[check]: set_context(intent=$output.intent)
[trade]: print(msg="Trading")
[done]: print(msg="Done")

[next] -> [check]
[check] if $ctx.intent == "trade" -> [trade]
[check] -> [done]
```

---

## Execution Model

### Compile-Time Merging

`flows:` imports are processed at **compile time** (after parsing, before execution). All nodes and edges of subworkflows are merged into the parent graph with namespace prefixes, forming a unified DAG.

```
Parse phase:
  parent.jg  →  WorkflowGraph (with pending edges)
  trading.jg →  WorkflowGraph
  events.jg  →  WorkflowGraph

Merge phase:
  parent + trading.* + events.* → Unified DAG

Execution phase:
  executor runs all nodes in topological order (unaware of node origin)
```

### Shared Context

All nodes after merging share the same `WorkflowContext`:

- `$ctx` is shared across the entire merged graph
- `$input` is the parent workflow's input
- Each node's `$output` is updated in normal topological order

### Execution Flow

When a parent workflow edge points to a subworkflow node (e.g., `[route] -> [trading.start]`), the executor starts from `[trading.start]` and continues along the subworkflow's internal edges until it encounters an edge leading back to the parent workflow (e.g., `[trading.done] -> [done]`).

All intermediate subworkflow nodes (including internal conditional branches, switch routing, etc.) execute normally.

---

## Recursive Imports

Subworkflows can have their own `flows:` declarations, enabling multi-level composition:

```juglans
# Recursive imports: main imports order, order imports payment
# After merging, payment nodes get prefix: order.payment.*

[start]: print(msg="Order flow")
[validate]: print(msg="Validating")
[charge]: print(msg="Charging")
[confirm]: print(msg="Confirmed")

[start] -> [validate] -> [charge] -> [confirm]
```

After merging, the `payment` subworkflow's nodes appear in the final DAG with the `order.payment.` prefix:

```
order.start → order.validate → order.payment.charge → order.payment.confirm → order.done
```

---

## Circular Import Detection

If a circular import occurs (A imports B, B imports A), the compiler will report an error:

```
Error: Circular flow import detected: 'auth' (/path/to/auth.jg)
Import chain: ["/path/to/main.jg", "/path/to/auth.jg"]
```

---

## Resource Merging

Resource imports declared in subworkflows (prompts, agents, tools) are automatically merged into the parent workflow, with paths resolved relative to the subworkflow file's directory:

```juglans
# src/trading.jg — resource patterns relative to the .jg file's directory
prompts: ["./prompts/*.jgprompt"]
agents: ["./agents/*.jgagent"]

[start]: print(msg="Trading workflow")
```

After merging, the parent workflow can use prompts and agents introduced by the subworkflow. Python module imports are also automatically merged (with deduplication).

---

## Complete Example

### Project Structure

```
my-project/
├── juglans.toml
└── src/
    ├── main.jg
    ├── trading.jg
    ├── events.jg
    ├── agents/
    │   └── router.jgagent
    └── pure-agents/
        ├── trader.jgagent
        └── event-handler.jgagent
```

### Main Workflow — `src/main.jg`

```juglans
name: "Event Router"
agents: ["./agents/*.jgagent"]

flows: {
  trading: "./trading.jg"
  events: "./events.jg"
}

entry: [start]
exit: [done]

[start]: set_context(
  event_type=$input.event_type,
  message=$input.message
)
[route]: chat(
  agent="router",
  message=$input.message,
  format="json"
)
[done]: reply(output=$output)

[start] -> [route]

# Route to different subworkflows based on routing result
[route] if $output.type == "event" -> [events.start]
[route] if $output.type == "trade" -> [trading.start]
[route] -> [done]

# Converge after subworkflow completion
[events.respond] -> [done]
[trading.done] -> [done]
```

### Subworkflow — `src/trading.jg`

```juglans
name: "Trading Flow"
agents: ["./pure-agents/*.jgagent"]

entry: [start]
exit: [done]

[start]: set_context(trade_started=true)
[extract]: chat(
  agent="trader",
  message=$ctx.message,
  format="json"
)
[execute]: chat(
  agent="trader",
  message="Execute trade: " + json($extract.output)
)
[done]: set_context(trade_result=$output)

[start] -> [extract] -> [execute] -> [done]
[extract] on error -> [done]
```

### Equivalent DAG After Merging

```
[start] → [route] ─── if "event" ──→ [events.start] → ... → [events.respond] ─┐
                  ─── if "trade" ──→ [trading.start] → [trading.extract]        │
                  ─── default ─────→ [done] ←──────── → [trading.execute]       │
                                       ↑                → [trading.done] ───────┤
                                       └────────────────────────────────────────┘
```

---

## Best Practices

1. **Clear naming** — Aliases should reflect the subworkflow's responsibility, such as `auth`, `trading`, `notification`
2. **Explicit connections** — Edges between parent and subworkflows must be explicitly written (`[route] -> [auth.start]`, `[auth.done] -> [next]`); implicit entry points are not supported
3. **Single responsibility** — Each subworkflow should focus on one functional area; complex logic is achieved through composition
4. **Avoid deep nesting** — Although recursive imports are supported, it is recommended to keep them within 2-3 levels
5. **Context protocol** — Document the expected `$ctx` variables and output format in subworkflow comments, making it easier for other workflows to integrate correctly
