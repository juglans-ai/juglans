# Context Variable Reference

During workflow execution, a context object is maintained for storing and passing data.

## Variable Types

| Prefix | Description | Writable |
|------|------|------|
| `$input` | Workflow input | No |
| `$output` | Current node output | No |
| `$ctx` | Custom context | Yes |
| `$reply` | AI reply metadata | No |
| `$alias.node.field` | Subworkflow node output (produced by `flows:` imports) | No |

## $input - Input Variables

Data passed in when the workflow starts.

### Sources

```bash
# CLI
juglans workflow.jg --input '{"query": "hello", "count": 5}'

# API
POST /api/workflows/my-flow/execute
{"query": "hello", "count": 5}
```

### Access

```text
$input              # Entire input object
$input.query        # String: "hello"
$input.count        # Number: 5
$input.nested.field # Nested access
```

### Examples

```juglans
[process]: chat(
  agent="assistant",
  message=$input.query
)

[loop]: foreach($item in $input.items) {
  [handle]: chat(message=$item.content)
}
```

---

## $output - Node Output

The execution result of the most recent node.

### Characteristics

- Updated after each node executes
- Only valid within the current execution chain
- Type depends on the node's return value

### Output from Different Tools

| Tool | Output Type |
|------|----------|
| `chat()` | string or object (format="json") |
| `p()` | string |
| `notify()` | null |
| `set_context()` | null |
| `fetch_url()` | string or object |
| `timer()` | null |

### Examples

```juglans
# String output
[ask]: chat(agent="assistant", message="Hello")
[log]: notify(status="Response: " + $output)

# JSON output
[classify]: chat(agent="classifier", format="json")
[route]: notify(status="Category: " + $output.category)

# Chained usage
[render]: p(slug="template", data=$input)
[process]: chat(agent="processor", message=$output)
[save]: set_context(result=$output)
```

---

## $ctx - Custom Context

User-defined variable storage, set via `set_context()`.

### Setting Variables

```juglans
# Simple values
[init]: set_context(count=0, status="ready")

# Objects
[setup]: set_context(config={"timeout": 30, "retries": 3})

# Arrays
[prepare]: set_context(results=[], history=[])

# From output
[update]: set_context(name=$output.name, score=$output.score)
```

### Reading Variables

```text
$ctx.count           # Number
$ctx.status          # String
$ctx.config.timeout  # Nested access
$ctx.results         # Array
$ctx.user.name       # Nested object
```

### Updating Variables

```juglans
# Increment
[inc]: set_context(count=$ctx.count + 1)

# Append to array
[add]: set_context(results=append($ctx.results, $output))

# Conditional update
[update]: set_context(
  status=if($ctx.count > 10, "complete", "running")
)
```

### Scope

`$ctx` persists throughout the entire workflow execution:

```juglans
[init]: set_context(total=0)
[step1]: set_context(total=$ctx.total + 10)  # total=10
[step2]: set_context(total=$ctx.total + 20)  # total=30
[final]: notify(status="Total: " + $ctx.total)  # "Total: 30"
```

---

## $reply - Reply Metadata

Metadata information from AI replies.

### Available Fields

| Field | Type | Description |
|------|------|------|
| `$reply.content` | string | Reply content |
| `$reply.tokens` | number | Number of tokens used |
| `$reply.model` | string | Model used |
| `$reply.finish_reason` | string | Finish reason |

### Examples

```juglans
[ask]: chat(agent="assistant", message=$input.query)
[log]: notify(status="Used " + $reply.tokens + " tokens")
[save]: set_context(
  last_response=$reply.content,
  token_count=$reply.tokens
)
```

---

## Namespaced Variables (Flow Imports)

When using `flows:` to import subworkflows, variable references to internal subworkflow nodes are automatically prefixed with the namespace.

### Transformation Rules

Only variables whose first segment matches a subworkflow internal node ID get prefixed. Global variables (`$ctx`, `$input`, `$output`) are not affected:

```text
# Assume the auth subworkflow has two internal nodes: verify and extract

# Syntax inside subworkflow           →  Actual variable after merging
$verify.output              →  $auth.verify.output
$extract.output.intent      →  $auth.extract.output.intent
$ctx.some_var               →  $ctx.some_var          # Unchanged
$input.message              →  $input.message          # Unchanged
$output                     →  $output                 # Unchanged
```

### Usage in the Parent Workflow

```juglans
# After flow import merging, subworkflow node outputs are accessed via namespace
[next]: chat(message=$ctx.auth_result)

# Conditions can use any context variable
[check]: set_context(intent=$output.intent)
[trade]: print(msg="Trading")
[done]: print(msg="Done")

[check] if $ctx.intent == "trade" -> [trade]
[check] -> [done]
```

See [Workflow Composition Guide](../guide/workflow-composition.md) for details.

---

## Loop Context

Available in `foreach` and `while` loops:

| Variable | Type | Description |
|------|------|------|
| `loop.index` | number | Current index (0-based) |
| `loop.first` | boolean | Whether this is the first iteration |
| `loop.last` | boolean | Whether this is the last iteration |

### Examples

```juglans
[init]: set_context(results=[])

[process]: foreach($item in $input.items) {
  [log]: notify(
    status="Processing " + (loop.index + 1) + "/" + len($input.items)
  )

  [handle]: chat(agent="processor", message=$item)
  [collect]: set_context(results=append($ctx.results, $output))

  [log] -> [handle] -> [collect]
}

[done]: notify(status="All items processed! Count: " + len($ctx.results))

[init] -> [process] -> [done]
```

---

## Expression Syntax

### Arithmetic Operations

```juglans
[add]: set_context(result=$ctx.a + $ctx.b)       # Addition
[sub]: set_context(result=$ctx.a - $ctx.b)       # Subtraction
[mul]: set_context(result=$ctx.a * $ctx.b)       # Multiplication
[div]: set_context(result=$ctx.a / $ctx.b)       # Division
[mod]: set_context(result=$ctx.a % $ctx.b)       # Modulo
```

### Comparison Operations

```juglans
[eq]: set_context(result=$ctx.a == $ctx.b)       # Equal to
[ne]: set_context(result=$ctx.a != $ctx.b)       # Not equal to
[gt]: set_context(result=$ctx.a > $ctx.b)        # Greater than
[lt]: set_context(result=$ctx.a < $ctx.b)        # Less than
[ge]: set_context(result=$ctx.a >= $ctx.b)       # Greater than or equal to
[le]: set_context(result=$ctx.a <= $ctx.b)       # Less than or equal to
```

### Logical Operations

```juglans
[and]: set_context(result=$ctx.a && $ctx.b)      # AND
[or]: set_context(result=$ctx.a || $ctx.b)       # OR
[not]: set_context(result=!$ctx.a)               # NOT
```

### String Operations

```juglans
[greet]: notify(status="Hello, " + $input.name)
[info]: notify(status=$input.text + " (length: " + len($input.text) + ")")
```

### Built-in Functions

| Function | Description | Example |
|------|------|------|
| `len(x)` | Length | `len($ctx.items)` |
| `json(x)` | Convert to JSON | `json($ctx.data)` |
| `append(arr, item)` | Append | `append($ctx.list, $output)` |
| `if(cond, a, b)` | Conditional | `if($ctx.ok, "yes", "no")` |

---

## Complete Example

```juglans
name: "Context Demo"
version: "0.1.0"

entry: [init]
exit: [summary]

# Initialize context
[init]: set_context(
  processed=0,
  successes=0,
  failures=0,
  results=[]
)

# Process input items
[process]: foreach($item in $input.items) {
  [log_start]: notify(
    status="[" + (loop.index + 1) + "/" + len($input.items) + "] Processing: " + $item.name
  )

  [analyze]: chat(
    agent="analyzer",
    message=$item.content,
    format="json"
  )

  # Update counts based on results
  [update]: set_context(
    processed=$ctx.processed + 1,
    successes=$ctx.successes + if($output.success, 1, 0),
    failures=$ctx.failures + if(!$output.success, 1, 0),
    results=append($ctx.results, {
      "name": $item.name,
      "result": $output
    })
  )

  [log_start] -> [analyze] -> [update]
}

# Summary
[summary]: notify(
  status="Complete! Processed: " + $ctx.processed +
         ", Successes: " + $ctx.successes +
         ", Failures: " + $ctx.failures
)

[init] -> [process] -> [summary]
```

---

## Debugging Tips

### Print Context

```juglans
[debug]: notify(status="Context: " + json($ctx))
```

### Check Variable Type

```juglans
[check]: notify(status="Type: " + type($ctx.value))
```

### Conditional Breakpoint

```juglans
[breakpoint]: notify(status="count=" + $ctx.count)
[error_handler]: notify(status="Count exceeded 100!")

[breakpoint] if $ctx.count > 100 -> [error_handler]
```
