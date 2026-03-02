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

```yaml
$input              # Entire input object
$input.query        # String: "hello"
$input.count        # Number: 5
$input.nested.field # Nested access
```

### Examples

```yaml
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

```yaml
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

```yaml
# Simple values
[init]: set_context(count=0, status="ready")

# Objects
[init]: set_context(config={"timeout": 30, "retries": 3})

# Arrays
[init]: set_context(results=[], history=[])

# Nested paths
[update]: set_context(user.name="Alice", user.score=100)
```

### Reading Variables

```yaml
$ctx.count           # Number
$ctx.status          # String
$ctx.config.timeout  # Nested access
$ctx.results         # Array
$ctx.user.name       # Nested object
```

### Updating Variables

```yaml
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

```yaml
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

```yaml
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

```yaml
# Assume the auth subworkflow has two internal nodes: verify and extract

# Syntax inside subworkflow           →  Actual variable after merging
$verify.output              →  $auth.verify.output
$extract.output.intent      →  $auth.extract.output.intent
$ctx.some_var               →  $ctx.some_var          # Unchanged
$input.message              →  $input.message          # Unchanged
$output                     →  $output                 # Unchanged
```

### Usage in the Parent Workflow

```yaml
flows: {
  auth: "./auth.jg"
}

# Access subworkflow node output through namespace path
[next]: chat(message=$auth.verify.output)

# Used in conditions
[check] if $auth.extract.output.intent == "trade" -> [trade]
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

```yaml
[process]: foreach($item in $input.items) {
  [log]: notify(
    status="Processing " + (loop.index + 1) + "/" + len($input.items)
  )

  {% if loop.first %}
  [init]: set_context(results=[])
  {% endif %}

  [handle]: chat(agent="processor", message=$item)
  [collect]: set_context(results=append($ctx.results, $output))

  {% if loop.last %}
  [summary]: notify(status="All items processed!")
  {% endif %}
}
```

---

## Expression Syntax

### Arithmetic Operations

```yaml
$ctx.a + $ctx.b      # Addition
$ctx.a - $ctx.b      # Subtraction
$ctx.a * $ctx.b      # Multiplication
$ctx.a / $ctx.b      # Division
$ctx.a % $ctx.b      # Modulo
```

### Comparison Operations

```yaml
$ctx.a == $ctx.b     # Equal to
$ctx.a != $ctx.b     # Not equal to
$ctx.a > $ctx.b      # Greater than
$ctx.a < $ctx.b      # Less than
$ctx.a >= $ctx.b     # Greater than or equal to
$ctx.a <= $ctx.b     # Less than or equal to
```

### Logical Operations

```yaml
$ctx.a && $ctx.b     # AND
$ctx.a || $ctx.b     # OR
!$ctx.a              # NOT
```

### String Operations

```yaml
"Hello, " + $input.name              # Concatenation
$input.text + " (length: " + len($input.text) + ")"
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

```yaml
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

```yaml
[debug]: notify(status="Context: " + json($ctx))
```

### Check Variable Type

```yaml
[check]: notify(status="Type: " + type($ctx.value))
```

### Conditional Breakpoint

```yaml
[breakpoint] if $ctx.count > 100 -> [error_handler]
```
