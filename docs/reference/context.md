# Context Variable Reference

The workflow context is a shared data store maintained throughout execution. All variables are accessed via dollar-sign prefix and dot-notation paths.

---

## Variable Types

| Prefix | Source | Writable | Scope | Description |
|--------|--------|----------|-------|-------------|
| `$input` | CLI `--input` / API body / web server injection | No | Entire workflow | Workflow input data |
| `$output` | Previous node's return value | No | Overwritten after each node | Current node output |
| `$ctx` | `set_context()` tool | Yes | Entire workflow | User-defined variable storage |
| `$reply` | AI runtime / `chat()` / `reply()` | Partially | Entire workflow | AI reply metadata |
| `$error` | Executor on node failure | No | Set on error, persists | Error information |
| `$config` | `juglans.toml` | No | Entire workflow | Configuration from config file |
| `$response` | `response()` tool | Yes | HTTP workflows only | HTTP response control |

---

## $input -- Workflow Input

Data passed when the workflow starts. Read-only throughout execution.

### Sources

**CLI:**
```bash
juglans workflow.jg --input '{"query": "hello", "count": 5}'
juglans workflow.jg --input-file data.json
```

**API (juglans web):**
```
POST /api/workflows/my-flow/execute
{"query": "hello", "count": 5}
```

**HTTP handler (serve()):** The web server pre-injects `$input.method`, `$input.path`, `$input.query`, `$input.body`, `$input.headers`, and `$input.route`.

### Path Access

```text
$input              # Entire input object
$input.query        # Top-level field
$input.user.name    # Nested object field
$input.items.0      # Array element by index
```

### Example

```juglans
[greet]: print(message="Hello, " + $input.name)
```

---

## $output -- Node Output

The return value of the most recently executed node. Overwritten after each node completes.

### Output Types by Tool

| Tool | `$output` Type |
|------|---------------|
| `chat()` | string (or object if `format="json"`) |
| `p()` | string |
| `fetch()` | `{status, ok, data}` |
| `fetch_url()` | `{status, ok, method, url, content}` |
| `bash()` | `{stdout, stderr, exit_code, ok}` |
| `read_file()` | `{content, total_lines, lines_returned, offset}` |
| `write_file()` | `{status, file_path, lines_written, bytes_written}` |
| `glob()` | `{matches, count, pattern}` |
| `grep()` | `{matches, total_matches, files_searched, truncated}` |
| `set_context()` | null |
| `notify()` | `{status, content}` |
| `print()` | string (the printed message) |
| `reply()` | `{content, status}` |
| `return()` | the evaluated value |
| `timer()` | `{status, duration_ms}` |

### Path Access

```text
$output             # Entire output value
$output.status      # Field access (when output is object)
$output.data.items  # Nested field access
$output.items.0     # Array element by index
```

### Example

```juglans
[ask]: chat(agent="assistant", message="Hello")
[log]: print(message="Response: " + $output)

[ask] -> [log]
```

```juglans
[api]: fetch(url="https://api.example.com/data")
[check]: print(message="Status: " + str($output.status))

[api] -> [check]
```

---

## $ctx -- Custom Context

User-defined variable storage. Set via `set_context()`. Persists for the entire workflow execution.

### Setting Variables

```juglans
[init]: set_context(count=0, status="ready", items=[])
```

```juglans
[update]: set_context(count=$ctx.count + 1)
```

```juglans
[collect]: set_context(results=append($ctx.results, $output))
```

### Path Access

```text
$ctx.count           # Number
$ctx.status          # String
$ctx.config.timeout  # Nested object field
$ctx.results         # Array
$ctx.results.0       # Array element by index
$ctx.user.name       # Deeply nested field
```

### Lifecycle

`$ctx` variables persist from the moment they are set until the workflow terminates. Subsequent calls to `set_context()` with the same key overwrite the previous value.

```juglans
[a]: set_context(total=0)
[b]: set_context(total=$ctx.total + 10)
[c]: set_context(total=$ctx.total + 20)
[d]: print(message="Total: " + str($ctx.total))

[a] -> [b] -> [c] -> [d]
```

The final output is `Total: 30`.

---

## $reply -- AI Reply Metadata

Metadata from AI interactions. Updated by `chat()` and `reply()`.

### Available Fields

| Field | Type | Source | Description |
|-------|------|--------|-------------|
| `$reply.output` | string | `chat()`, `reply()` | Accumulated AI response text |
| `$reply.chat_id` | string | `chat()` | Conversation session ID |
| `$reply.status` | string | `notify()` | Latest status message |
| `$reply.user_message_id` | integer | Web server | ID of the user message that triggered execution |

### Example

```juglans
[ask]: chat(agent="assistant", message=$input.question)
[followup]: chat(
  agent="assistant",
  chat_id=$reply.chat_id,
  message="Can you elaborate?"
)

[ask] -> [followup]
```

---

## $error -- Error Information

Set automatically by the executor when a node fails. Contains the error details.

### Structure

```json
{"node": "failed_node_id", "message": "Error description"}
```

Per-node error is also available as `$<node_id>.error`.

### Usage in Error Edges

```juglans
[api]: fetch(url="https://api.example.com/data")
[ok]: print(message="Success")
[fail]: print(message="Error: " + $error.message)

[api] -> [ok]
[api] on error -> [fail]
```

---

## $config -- Configuration

The parsed `juglans.toml` configuration, injected at workflow start. Read-only.

### Example

```juglans
[info]: print(message="Server port: " + str($config.server.port))
```

---

## $response -- HTTP Response

Used exclusively in `serve()` workflows. Written by the `response()` tool.

### Fields

| Field | Type | Description |
|-------|------|-------------|
| `$response.status` | integer | HTTP status code |
| `$response.body` | any | Response body |
| `$response.headers` | object | Custom response headers |
| `$response.file` | string | File path to serve |

---

## Namespaced Variables (Flow Imports)

When using `flows:` to import subworkflows, internal node references are automatically prefixed with the namespace.

### Transformation Rules

Only variables whose first segment matches a subworkflow internal node ID get prefixed. Global variables (`$ctx`, `$input`, `$output`, `$reply`) are unchanged:

```text
# Assume flows: { auth: "auth.jg" }
# auth.jg has internal nodes: verify, extract

# Inside subworkflow          After merge
$verify.output         ->     $auth.verify.output
$extract.output.name   ->     $auth.extract.output.name
$ctx.token             ->     $ctx.token              # Unchanged
$input.message         ->     $input.message           # Unchanged
```

---

## Loop Context

Available inside `foreach` and `while` blocks:

| Variable | Type | Description |
|----------|------|-------------|
| `loop.index` | number | Current iteration index (0-based) |
| `loop.first` | boolean | True on first iteration |
| `loop.last` | boolean | True on last iteration |

### Example

```juglans
[init]: set_context(results=[])

[process]: foreach($item in $input.items) {
  [log]: notify(status="Processing " + str(loop.index + 1) + "/" + str(len($input.items)))
  [handle]: chat(agent="processor", message=$item)
  [collect]: set_context(results=append($ctx.results, $output))

  [log] -> [handle] -> [collect]
}

[done]: print(message="Processed " + str(len($ctx.results)) + " items")

[init] -> [process] -> [done]
```

---

## Path Access Syntax

All variable types use the same dot-notation path syntax:

```text
$prefix                    # Entire value
$prefix.field              # Object field
$prefix.nested.field       # Nested object field
$prefix.array.0            # Array element (0-based index)
$prefix.array.0.field      # Field of array element
```

### Behavior in Different Positions

**Node parameters** -- Variables are resolved before passing to the tool:

```juglans
[step]: chat(agent=$input.agent, message=$ctx.prompt)
```

**Conditional edges** -- Variables are resolved and compared:

```juglans
[check]: set_context(status="done")
[next]: print(message="Moving on")
[retry]: print(message="Retrying")

[check] if $ctx.status == "done" -> [next]
[check] if $ctx.status == "error" -> [retry]
```

**Switch routing** -- The switch subject is resolved to a string for branch matching:

```juglans
[classify]: chat(agent="classifier", message=$input.text, format="json")
[handle_a]: print(message="Category A")
[handle_b]: print(message="Category B")
[handle_other]: print(message="Unknown")

[classify] -> switch $output.category {
  "a": [handle_a]
  "b": [handle_b]
  default: [handle_other]
}
```

---

## Comprehensive Example

```juglans
[init]: set_context(
  processed=0,
  successes=0,
  failures=0,
  results=[]
)

[process]: foreach($item in $input.items) {
  [log_start]: notify(
    status="[" + str(loop.index + 1) + "/" + str(len($input.items)) + "] Processing: " + $item.name
  )

  [analyze]: chat(
    agent="analyzer",
    message=$item.content,
    format="json"
  )

  [update]: set_context(
    processed=$ctx.processed + 1,
    successes=$ctx.successes + if($output.success, 1, 0),
    failures=$ctx.failures + if(not $output.success, 1, 0),
    results=append($ctx.results, {"name": $item.name, "result": $output})
  )

  [log_start] -> [analyze] -> [update]
}

[summary]: print(
  message="Complete! Processed: " + str($ctx.processed) +
         ", Successes: " + str($ctx.successes) +
         ", Failures: " + str($ctx.failures)
)

[init] -> [process] -> [summary]
```

---

## Debugging Tips

**Print entire context:**

```juglans
[debug]: print(message="Ctx: " + json($ctx))
```

**Check variable type:**

```juglans
[check]: print(message="Type: " + type($ctx.value))
```

**Conditional breakpoint:**

```juglans
[step]: set_context(count=50)
[ok]: print(message="Count is fine")
[warn]: print(message="Count exceeded limit!")

[step] if $ctx.count > 100 -> [warn]
[step] -> [ok]
```
