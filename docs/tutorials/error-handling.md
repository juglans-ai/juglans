# Tutorial 5: Error Handling

This chapter covers how to handle failures gracefully in workflows: **on error edges** make error paths explicit, and the **error variable** lets you access error details in fallback nodes.

## Without Error Handling

First, consider a workflow that will fail — reading a nonexistent file:

```juglans
[read]: read_file(file_path="/nonexistent/file.txt")
[show]: print(message="Content: " + output)
[read] -> [show]
```

When `[read]` executes, it will error (file does not exist) and the entire workflow terminates. `[show]` will never execute. The console output will be something like:

```text
Error: Node [read] failed: No such file or directory
```

This is unacceptable in many scenarios. For example, when calling external APIs, reading user files, or running AI inference — any step can fail, but you want the workflow to keep running and follow an alternate path.

## on error Basics

`on error` is an edge modifier. When the source node fails, instead of terminating the workflow, execution jumps to the specified fallback node.

```juglans
[read]: read_file(file_path="/nonexistent/file.txt")
[fallback]: print(message="File not found, using default")
[show]: print(message="Reached the end")

[read] -> [show]
[read] on error -> [fallback]
[fallback] -> [show]
```

Line-by-line explanation:

1. `[read]` attempts to read the file. If successful, it follows `[read] -> [show]`.
2. If it fails, the `on error` edge activates, jumping to `[fallback]`.
3. `[fallback]` outputs an informational message, then follows `[fallback] -> [show]`.
4. Whether it succeeds or fails, execution ultimately reaches `[show]`.

Syntax:

```text
[source_node] on error -> [fallback_node]
```

Rules:

- An `on error` edge is **only followed when the source node fails**. Under normal conditions it is ignored.
- A node can have both normal edges and `on error` edges. Normal edges are followed on success; `on error` edges are followed on failure.
- When a node has an `on error` edge, failure **will not** terminate the workflow.

### Success Path vs Error Path

Use assignment syntax to mark which path was taken:

```juglans
[step]: read_file(file_path="/nonexistent/file.txt")
[ok]: status = "success"
[err]: status = "failed"
[report]: print(message="Status: " + status)

[step] -> [ok]
[step] on error -> [err]
[ok] -> [report]
[err] -> [report]
```

At runtime, `[step]` will fail, so `[err]` executes, `status` is set to `"failed"`, and the final output is:

```text
Status: failed
```

If you change `file_path` to an existing file, `[ok]` executes and outputs `Status: success`. Two paths, one exit — this is the fundamental pattern for error handling.

## The error Variable

When `on error` triggers, the engine automatically sets the `error` variable with two fields:

| Field | Type | Content |
|------|------|------|
| `error.node` | string | The ID of the node that errored |
| `error.message` | string | The error message |

Read it in a fallback node:

```juglans
[read]: read_file(file_path="/nonexistent/file.txt")
[handle]: print(message="Error in [" + error.node + "]: " + error.message)

[read] on error -> [handle]
```

Output example:

```text
Error in [read]: No such file or directory
```

### node_id.error

In addition to the global `error`, each errored node's error message is also stored in `node_id.error`:

```juglans
[read]: read_file(file_path="/nonexistent/file.txt")
[handle]: print(message="read node error: " + read.error)

[read] on error -> [handle]
```

`error` is global and always points to the most recently errored node. `node_id.error` is node-specific and more precise in multi-node error handling scenarios.

## Multi-Level Error Handling

Different nodes can have different fallbacks:

```juglans
[load_config]: read_file(file_path="/etc/app/config.json")
[load_data]: read_file(file_path="/tmp/data.csv")
[process]: print(message="Processing data...")
[config_fallback]: config = "default"
[data_fallback]: data = "empty"
[done]: print(message="Workflow complete")

[load_config] -> [load_data]
[load_config] on error -> [config_fallback]
[config_fallback] -> [load_data]

[load_data] -> [process]
[load_data] on error -> [data_fallback]
[data_fallback] -> [process]

[process] -> [done]
```

Execution flow:

1. `[load_config]` attempts to read the configuration. If it fails, it jumps to `[config_fallback]`, sets a default value, then continues to `[load_data]`.
2. `[load_data]` attempts to read data. If it fails, it jumps to `[data_fallback]`, sets empty data, then continues to `[process]`.
3. Regardless of which step fails, the workflow can always reach `[done]`.

Each "potentially failing" node has its own fallback, without interfering with others.

## Error Handling + Conditional Branching

`on error` can be combined with conditional edges:

```juglans
[init]: mode = "strict"
[work]: read_file(file_path="/tmp/important.txt")
[ok]: print(message="File loaded")
[warn]: print(message="File missing, but mode is lenient")
[abort]: print(message="File missing in strict mode, aborting")
[done]: print(message="Done")

[init] -> [work]
[work] -> [ok]
[work] on error -> [warn]
[work] on error -> [abort]
[ok] -> [done]
[warn] -> [done]
[abort] -> [done]
```

Here, two `on error` edges are defined for the same node. When `[work]` fails, the engine selects the first reachable `on error` edge in definition order.

If you need to differentiate error handling strategies based on context, a better approach is to have the fallback node handle the decision internally:

```juglans
[init]: mode = "strict"
[work]: read_file(file_path="/tmp/important.txt")
[ok]: print(message="File loaded")
[error_router]: print(message="Handling error...")
[warn]: print(message="File missing, lenient mode")
[abort]: print(message="Strict mode, abort!")
[done]: print(message="Done")

[init] -> [work]
[work] -> [ok]
[work] on error -> [error_router]

[error_router] if mode == "strict" -> [abort]
[error_router] -> [warn]

[ok] -> [done]
[warn] -> [done]
[abort] -> [done]
```

`on error` jumps to `[error_router]`, which then uses conditional branching based on `mode` — error handling and routing logic each have their own responsibility.

## Comprehensive Example

A complete workflow with a normal path and multiple error handlers:

```juglans
[start]: errors = 0

# Step 1: Load configuration
[load_config]: read_file(file_path="/etc/app/config.json")
[config_ok]: config_loaded = true
[config_err]: config_loaded = false, errors = errors + 1

# Step 2: Load data
[load_data]: read_file(file_path="/tmp/dataset.csv")
[data_ok]: data_loaded = true
[data_err]: data_loaded = false, errors = errors + 1

# Step 3: Summary
[report]: print(
  message="Pipeline done. Errors: " + str(errors)
)

# Normal path
[start] -> [load_config]
[load_config] -> [config_ok]
[config_ok] -> [load_data]

# Config load failure
[load_config] on error -> [config_err]
[config_err] -> [load_data]

# Data load
[load_data] -> [data_ok]
[data_ok] -> [report]

# Data load failure
[load_data] on error -> [data_err]
[data_err] -> [report]
```

This workflow demonstrates a common "best effort" pattern:

1. Each potentially failing step has an `on error` edge.
2. Fallback nodes record the error (incrementing a counter), then let the workflow continue.
3. The final `[report]` node summarizes the error count.
4. Regardless of which intermediate step fails, the workflow always reaches the end.

## Summary

- `[node] on error -> [fallback]` -- When a node errors, jump to the fallback instead of terminating the workflow
- `error.node` and `error.message` -- Global error variables containing the most recent error's node ID and error message
- `node_id.error` -- Node-level error message, suitable for multi-node error handling
- Each potentially failing node can have its own independent fallback
- `on error` can be combined with conditional edges and switch routing
- Common pattern: fallback nodes set default values or record errors, then rejoin the normal flow

Next chapter: [Tutorial 6: AI Chat](./ai-chat.md) -- Learn how to call AI models in workflows, construct multi-turn conversations, and obtain structured output.
