# .jg Workflow Syntax Reference

Complete syntax specification for Juglans `.jg` workflow files.

## File Structure

A `.jg` file consists of three sections in order:

```text
1. Metadata      — imports, schedule, visibility
2. Node definitions  — [id]: tool(params) or [id]: "literal"
3. Edge definitions  — [a] -> [b], conditionals, switch
```

All sections are optional. Comments begin with `#` and extend to end of line.

---

## Metadata

Metadata lines appear at the top of the file. Each line follows `key: value` format.

| Field | Type | Description |
|---|---|---|
| `slug` | string | Unique identifier (for registry) |
| `name` | string | Display name |
| `version` | string | Workflow version |
| `description` | string | Human-readable description |
| `author` | string | Author name |
| `source` | string | Source file path |
| `entry` | string or list | Explicit entry node ID (overrides topological inference) |
| `exit` | string list | Explicit terminal node IDs |
| `prompts` | string list | Prompt file glob patterns |
| `agents` | string list | _(deprecated, silently ignored)_ |
| `tools` | string list | Tool definition file patterns |
| `python` | string list | Python module imports |
| `flows` | object map | Subworkflow imports: `{ alias: "path.jg" }` |
| `libs` | list or map | Library imports (function-only) |
| `is_public` | boolean | Resource visibility |
| `schedule` | string | Cron schedule expression |

### Metadata Examples

Minimal metadata:

```juglans
[start]: notify(status="hello")
```

Full metadata:

```juglans
prompts: ["./prompts/*.jgx"]

[start]: notify(status="begin")
[done]: notify(status="end")
[start] -> [done]
```

Multiple terminal nodes:

```juglans
[start]: notify(status="begin")
[success]: notify(status="ok")
[failure]: notify(status="fail")

[start] -> [success]
[start] on error -> [failure]
```

Entry nodes are determined automatically by topological sort (nodes with in-degree 0).

### Resource Imports

```juglans
prompts: ["./prompts/*.jgx", "./shared/*.jgx"]

[step]: notify(status="imported")
```

### Flow Imports

```juglans
flows: {
  auth: "./auth.jg"
  trading: "./trading.jg"
}

[start]: notify(status="routing")
[done]: notify(status="complete")

[start] -> [done]
```

### Lib Imports

Map form (explicit namespace):

```juglans
libs: { db: "./libs/sqlite.jg" }

[step1]: db.read(table="users")
```

List form (auto namespace from filename stem):

```juglans
libs: ["./libs/utils.jg"]

[step1]: utils.helper(x="test")
```

### Python Module Imports

```juglans
python: ["pandas", "sklearn.ensemble", "./utils.py"]

[load]: notify(status="python ready")
```

---

## Node Definitions

### Basic Syntax

```text
[node_id]: tool_name(param1=value1, param2=value2)
```

Node IDs use letters, digits, and underscores. They must be wrapped in `[...]`.

### Tool Call Node

```juglans
[greet]: notify(status="Hello, world!")
```

With multiple parameters:

```juglans
[ask]: chat(
  agent="assistant",
  message="What is Rust?",
  format="json"
)
```

### Variable References in Parameters

```juglans
[a]: chat(message=input.question)
[b]: notify(status=output)
[c]: data = results
[a] -> [b] -> [c]
```

### String Literal Node

```juglans
[greeting]: "Hello, World!"
```

### JSON Literal Node

```juglans
[config]: {
  "model": "gpt-4o-mini",
  "temperature": 0.7
}
```

### Assignment Syntax

Assignment syntax sets context variables directly:

```juglans
[init]: count = 0, name = "Alice"
```

Multiple assignments separated by commas:

```juglans
[setup]: status = "ready", retries = 3, items = []
```

---

## Edge Definitions

### Unconditional Edge

```juglans
[a]: notify(status="first")
[b]: notify(status="second")

[a] -> [b]
```

### Chain Edge

```juglans
[a]: notify(status="1")
[b]: notify(status="2")
[c]: notify(status="3")

[a] -> [b] -> [c]
```

### Conditional Edge

```juglans
[check]: score = 85
[pass]: notify(status="passed")
[fail]: notify(status="failed")

[check] if score >= 60 -> [pass]
[check] if score < 60 -> [fail]
```

Supported comparison operators: `==`, `!=`, `>`, `<`, `>=`, `<=`.

Boolean condition:

```juglans
[gate]: ready = true
[go]: notify(status="go")
[wait]: notify(status="wait")

[gate] if ready -> [go]
[gate] if !ready -> [wait]
```

### Default Path (Unconditional Fallback)

When no conditional edge matches, an unconditional edge serves as the default:

```juglans
[router]: notify(status="routing")
[path_a]: notify(status="a")
[path_b]: notify(status="b")
[default_path]: notify(status="default")

[router] if x == "a" -> [path_a]
[router] if x == "b" -> [path_b]
[router] -> [default_path]
```

### Error Handling Edge

```juglans
[risky]: notify(status="try something risky")
[ok]: notify(status="success")
[err]: notify(status="error occurred")

[risky] -> [ok]
[risky] on error -> [err]
```

The `error` variable is set in the error handler node with two fields: `node` (the failing node ID) and `message` (the error text).

```json
{"node": "risky", "message": "connection refused"}
```

### Switch Routing

Execute exactly one matching branch:

```juglans
[classify]: intent = "question"
[answer]: notify(status="answering")
[execute]: notify(status="executing")
[fallback]: notify(status="fallback")

[classify] -> switch intent {
    "question": [answer]
    "task": [execute]
    default: [fallback]
}
```

Switch with numeric cases:

```juglans
[score]: level = 2
[low]: notify(status="low")
[mid]: notify(status="mid")
[high]: notify(status="high")

[score] -> switch level {
    1: [low]
    2: [mid]
    default: [high]
}
```

**switch vs if edges:**

| Feature | `if` edges | `switch` |
|---|---|---|
| Multiple branches can fire | Yes | No (exactly one) |
| Syntax | Multiple lines | Single block |
| Default | Unconditional edge | `default:` keyword |

### Cross-Workflow Edges (with flows)

```juglans
flows: {
  auth: "./auth.jg"
}

[start]: notify(status="begin")
[done]: notify(status="end")

[start] if need_auth -> [auth.start]
[auth.done] -> [done]
[start] -> [done]
```

Namespaced node references use `[alias.node_id]` format.

---

## Function Definitions

Functions are reusable parameterized blocks. They are NOT added to the main DAG.

### Single-Step Function

```juglans
[greet(name)]: notify(status="Hello " + name)

[step1]: greet(name="world")
```

### Multi-Step Function (Block Body)

```juglans
[pipeline(msg)]: {
  notify(status="start: " + msg)
  notify(status="end: " + msg)
}

[run]: pipeline(msg="test")
```

Steps separated by newlines or semicolons. The function returns `output` from its last step.

### Multiple Parameters

```juglans
[deploy(env, tag)]: {
  notify(status="deploying " + tag + " to " + env)
  notify(status="done deploying " + tag)
}

[staging]: deploy(env="staging", tag="v1.0")
[prod]: deploy(env="production", tag="v1.0")

[staging] -> [prod]
```

### Function with assign_call

```juglans
[check_health(url)]: {
  result = fetch_url(url=url)
  notify(status="checked " + url)
}

[step]: check_health(url="https://example.com")
```

### Function with assert

```juglans
[validate(x)]: {
  assert x != ""
  notify(status="valid: " + x)
}

[step]: validate(x="hello")
```

---

## Loop Constructs

### foreach

Iterate over a collection:

```juglans
[init]: results = []

[loop]: foreach(item in input.items) {
  [handle]: notify(status=item)
  [save]: results = append(results, output)
  [handle] -> [save]
}

[done]: notify(status="finished")

[init] -> [loop] -> [done]
```

### foreach parallel

Run iterations concurrently:

```juglans
[batch]: foreach parallel(item in input.urls) {
  [fetch]: fetch_url(url=item)
}
```

### while

Condition-based loop:

```juglans
[init]: count = 0

[loop]: while(count < 5) {
  [inc]: count = count + 1
  [log]: notify(status="count=" + count)
  [inc] -> [log]
}

[done]: notify(status="loop done")

[init] -> [loop] -> [done]
```

### Nested Loop Body

Loop bodies contain their own node definitions and edge definitions:

```juglans
[start]: total = 0

[process]: foreach(item in input.data) {
  [step_a]: notify(status="processing " + item.id)
  [step_b]: total = total + 1
  [step_a] -> [step_b]
}

[end]: notify(status="total: " + total)

[start] -> [process] -> [end]
```

---

## Struct Definitions

Structs define typed data containers with fields and optional defaults:

```juglans
[User]: {
  name: str
  email: str
  age: int = 0
  role: str = "user"
}

[req]: new User(name="Alice", email="alice@example.com")
```

### Field Syntax

```text
field_name: type_hint [= default_value]
```

Supported types: `str`, `int`, `float`, `bool`, `list[T]`, `dict[K, V]`, `T?` (optional). Class names are also valid types.

### Instantiation

```juglans
[u]: new User(name="Alice", email="alice@example.com", age=30)
```

Fields with defaults can be omitted — they use their default value.

---

## impl Blocks

`impl` blocks group methods and associated functions for a struct, similar to Rust:

```juglans
[Counter]: {
  value: int = 0
  step: int = 1
}

impl Counter {
  [get(self)]: output = self.value
  [describe(self)]: output = "Counter(" + str(self.value) + ")"
  [defaults()]: output = "Counter defaults: value=0, step=1"
}

[c]: new Counter(value=10)
[val]: c.get()
[c] -> [val]
```

### Instance Methods

Methods with `self` as the first parameter operate on an instance:

```juglans
[c2]: new Counter(value=5, step=2)
[desc]: c2.describe()
[c2] -> [desc]
```

Inside a method body, `self.field_name` accesses instance fields.

### Associated Functions

Methods without `self` are associated functions (static methods), called via `Type.function()`:

```juglans
[info]: Counter.defaults()
```

### Standalone Method Syntax

Methods can also be defined outside `impl` blocks (backward-compatible):

```juglans
[User]: {
  name: str
  email: str
}

[User.validate(self)]: valid = self.name != ""

[u]: new User(name="Alice", email="alice@example.com")
[v]: u.validate()
[u] -> [v]
```

This is equivalent to defining `validate` inside `impl User { ... }`.

---

## Trait Definitions

Traits define behavior contracts (interfaces) without inheritance:

```juglans
trait Displayable {
  [format(self)]:
  [label(self)]: output = "[display] " + str(self.format())
}

[User]: {
  name: str
  email: str
}

impl Displayable for User {
  [format(self)]: output = self.name + " <" + self.email + ">"
}

[u]: new User(name="Alice", email="alice@example.com")
[show]: u.format()
[u] -> [show]
```

- Methods ending with `:` followed by a newline are **required** — implementing types must provide them.
- Methods with a body are **default implementations** — inherited if not overridden.

### impl Trait for Type

The compiler validates that all required methods are provided. Default methods (like `label` above) are automatically inherited.

### Trait in Libraries

Traits and impl blocks can be defined in library files and imported via `libs:`. Trait names are namespace-prefixed like functions:

```juglans
trait Renderable {
  [render(self)]:
}

[MyType]: {
  value: str = "hello"
}

impl Renderable for MyType {
  [render(self)]: output = self.value
}

[m]: new MyType(value="world")
[r]: m.render()
[m] -> [r]
```

---

## Validator Diagnostics

`juglans check` reports diagnostics with stable codes. Errors fail the check; warnings are only shown with `--all`.

| Code | Severity | Message |
|------|----------|---------|
| `W001` | Warning | No entry node specified; using first node as entry point |
| `W002` | Warning | Node `<id>` is not reachable from entry node |
| `W003` | Warning | No terminal nodes found (all nodes have outgoing edges) |
| `W004` | Warning | Unknown tool `<name>` |
| `E001` | Error | Entry node `<id>` does not exist |
| `E002` | Error | Cycle detected involving node `<id>`. Workflows must be acyclic (DAG). |
| `E004` | Error | Workflow contains no nodes |

---

## Comments

Line comments start with `#`:

```juglans
# This is a comment

# First node
[start]: notify(status="hello")

# Another comment
[end]: notify(status="bye")

[start] -> [end]  # inline comments are also allowed
```

---

## Variable System

| Variable | Description | Example |
|---|---|---|
| `input` | CLI/API input data | `input.message`, `input.items` |
| `output` | Previous node output | `output`, `output.field` |
| Context vars | Workflow context (via assignment syntax) | `count`, `results` |
| `reply` | Agent reply metadata | `reply.output`, `reply.status` |
| `error` | Error info (in error handlers) | `error.node`, `error.message` |

Variables use dot notation for nested access: `output.data.items[0].name`.

---

## Complete Example

```juglans
prompts: ["./prompts/*.jgx"]

# Agent definitions
[classifier]: {
  "model": "gpt-4o-mini",
  "temperature": 0.0,
  "system_prompt": "Classify user intent as question, task, or chat. Return JSON."
}

[expert]: {
  "model": "gpt-4o-mini",
  "system_prompt": "You are a knowledgeable expert."
}

[assistant]: {
  "model": "gpt-4o-mini",
  "system_prompt": "You are a friendly assistant."
}

# Classify user intent
[classify]: chat(agent=classifier, message=input.query, format="json")

# Handler branches
[handle_question]: chat(agent=expert, message=input.query)
[handle_task]: chat(agent=expert, message=input.query)
[handle_chat]: chat(agent=assistant, message=input.query)

# Final output
[done]: notify(status="processed")

# Routing logic
[classifier] -> [classify]
[expert] -> [handle_question]
[assistant] -> [handle_chat]

[classify] -> switch output.intent {
    "question": [handle_question]
    "task": [handle_task]
    default: [handle_chat]
}

[handle_question] -> [done]
[handle_task] -> [done]
[handle_chat] -> [done]
```
