# Tutorial 8: Workflow Composition

This chapter covers how to eliminate duplicate code: use **function definitions** to encapsulate reusable logic, **flows** to import external sub-workflows, and **libs** to import function libraries, splitting large workflows into composable modules.

## 8.1 The Problem: Code Duplication

Suppose you need to perform the same "logging" operation multiple times in a workflow:

```juglans
[log1]: print(message="[LOG] Step 1 started")
[work1]: data = "result1"
[log2]: print(message="[LOG] Step 2 started")
[work2]: data = "result2"
[log3]: print(message="[LOG] Step 3 started")
[work3]: data = "result3"

[log1] -> [work1] -> [log2] -> [work2] -> [log3] -> [work3]
```

`print(message="[LOG] ...")` is repeated three times. If the log format needs to change, you have to update all three places. As the workflow grows, this kind of duplication becomes increasingly difficult to maintain.

## 8.2 Function Definitions

Functions let you encapsulate repeated logic into a reusable unit.

### Minimal Example

```juglans
[log(msg)]: print(message="[LOG] " + msg)

[step1]: log(msg="Step 1 started")
[step2]: log(msg="Step 2 started")
[step3]: log(msg="Step 3 started")

[step1] -> [step2] -> [step3]
```

Line-by-line explanation:

1. `[log(msg)]` -- defines a function named `log` that accepts one parameter `msg`. The `(msg)` inside the brackets is the parameter list.
2. `: print(message="[LOG] " + msg)` -- the function body. This is a single-step function that directly binds a tool call. `msg` references the passed parameter.
3. `[step1]: log(msg="Step 1 started")` -- calls the function. The syntax is identical to calling a built-in tool: `function_name(param=value)`.
4. Function definitions **do not** appear in the DAG; they are simply callable templates.

Function definition syntax:

```text
[function_name(param1, param2, ...)]: function_body
```

### Multiple Parameters

Functions can accept any number of parameters:

```juglans
[greet(name, greeting)]: print(message=greeting + ", " + name + "!")

[step1]: greet(name="Alice", greeting="Hello")
[step2]: greet(name="Bob", greeting="Good morning")
[done]: print(message="All greeted!")

[step1] -> [step2] -> [done]
```

All parameters must be provided when calling.

### Multi-Step Functions

When the function body requires multiple steps, wrap them in curly braces `{ ... }`:

```juglans
[deploy(service, env)]: {
  print(message="Deploying " + service + " to " + env)
  notify(status=service + " deployed to " + env)
}

[step1]: deploy(service="api", env="staging")
[step2]: deploy(service="web", env="staging")
[done]: print(message="All deployed")

[step1] -> [step2] -> [done]
```

Line-by-line explanation:

1. `[deploy(service, env)]:` -- function signature with two parameters.
2. `{ ... }` -- multi-step function body. Steps inside are executed sequentially, separated by newlines or `;`.
3. The first step `print(...)` outputs a log message, the second step `notify(...)` sends a notification.
4. Calling `deploy(service="api", env="staging")` executes both steps in sequence.

Steps within a multi-step function body are automatically chained into a sequential execution pipeline.

### Semicolon Separation

Multi-step function bodies can also be written on a single line, separated by `;`:

```juglans
[ping(host)]: { print(message="Pinging " + host); notify(status="Pinged " + host) }

[a]: ping(host="server-1")
[b]: ping(host="server-2")

[a] -> [b]
```

## 8.3 Function Calls

Function calls are used in node positions, with the same syntax as tool calls:

```juglans
[check(item)]: print(message="Checking: " + item)

[c1]: check(item="database")
[c2]: check(item="cache")
[c3]: check(item="queue")
[report]: print(message="All checks done")

[c1] -> [c2] -> [c3] -> [report]
```

When a function is called, the engine:

1. Looks up the function definition named `check`.
2. Binds the parameter `item="database"` to `item` in the function body.
3. Executes the function body.
4. Stores the function body's output in `output`, available to subsequent nodes.

Function calls and built-in tool calls are syntactically identical. The engine resolves tool names in this order: built-in tools -> function definitions -> Python -> MCP -> client bridge.

## 8.4 Flow Import

As workflows grow larger, you will want to split logic across multiple files. `flows:` lets you import external `.jg` files as sub-workflows.

### Basic Usage

Suppose you have an authentication sub-workflow `auth.jg`:

```text
# auth.jg
[login]: user = "authenticated"
[verify]: print(message="Verifying user...")
[complete]: print(message="Auth complete")

[login] -> [verify] -> [complete]
```

Import it in the main workflow:

```juglans
flows: {
  auth: "./auth.jg"
}

[start]: print(message="Starting app")
[done]: print(message="App ready")

[start] -> [auth.login]
[auth.complete] -> [done]
```

Line-by-line explanation:

1. `flows: { auth: "./auth.jg" }` -- declares a sub-workflow import in the file header. `auth` is the alias, `"./auth.jg"` is the file path (relative to the current `.jg` file).
2. `[auth.login]` -- references a node in the sub-workflow. The format is `[alias.node_name]`.
3. At compile time, the engine merges the nodes and edges from `auth.jg` into the main graph, automatically prefixing all nodes with the `auth.` namespace.

### Namespace Prefixing

After import, all node IDs in the sub-workflow are automatically prefixed with `alias.original_ID`:

| Node in auth.jg | Merged ID |
|------------------|-----------|
| `[login]` | `[auth.login]` |
| `[verify]` | `[auth.verify]` |
| `[complete]` | `[auth.complete]` |

Namespace prefixing isolates node names from different sub-workflows, preventing conflicts.

### Cross-Workflow Connections

Reference sub-workflow nodes in the main workflow's edge definitions:

```juglans
flows: {
  auth: "./auth.jg"
  payment: "./payment.jg"
}

[start]: print(message="Order flow")
[done]: print(message="Order complete")

[start] -> [auth.login]
[auth.complete] -> [payment.charge]
[payment.done] -> [done]
```

Sub-workflows can also be connected to each other through the main workflow's edges. `[auth.complete] -> [payment.charge]` connects the output of the authentication sub-flow into the payment sub-flow.

### Multiple Flow Imports

`flows:` supports importing multiple sub-workflows simultaneously:

```juglans
flows: {
  auth: "./flows/auth.jg"
  notify: "./flows/notify.jg"
  log: "./flows/log.jg"
}

[start]: print(message="Begin")
[done]: print(message="End")

[start] -> [auth.start]
[auth.done] -> [notify.send]
[notify.done] -> [log.write]
[log.done] -> [done]
```

Each sub-workflow has its own independent namespace, so node names will not conflict.

## 8.5 Library Import

`libs:` is used to import **function libraries** -- `.jg` files that contain only function definitions. Unlike `flows:`, `libs:` does not merge sub-graph nodes; it only extracts function definitions.

### Library Files

A typical library file (`utils.jg`) contains only function definitions:

```text
# utils.jg

[log(msg)]: print(message="[LOG] " + msg)
[format_name(first, last)]: full_name = first + " " + last
```

### List-Style Import

```juglans
libs: ["./utils.jg"]

[step1]: utils.log(msg="Starting")
[step2]: print(message="Working...")

[step1] -> [step2]
```

Line-by-line explanation:

1. `libs: ["./utils.jg"]` -- list-style library import. You can import multiple libraries at once: `libs: ["./utils.jg", "./math.jg"]`.
2. `utils.log(msg="Starting")` -- calls a function from the library, in the format `namespace.function_name(params)`.

**Namespace rule** (list-style): Library namespaces use the file name stem (e.g. `utils.jg` → namespace `utils`).

### Object-Style Import

To customize the namespace, use object-style import:

```juglans
libs: {
  u: "./utils.jg"
}

[step1]: u.log(msg="Custom namespace")
```

`u` is the namespace you specify, overriding the default filename-stem namespace.

### Importing Multiple Libraries

```juglans
libs: ["./string_utils.jg", "./math_utils.jg"]

[step1]: string_utils.upper(text="hello")
[step2]: math_utils.add(a=1, b=2)
[done]: print(message="Done")

[step1] -> [step2] -> [done]
```

Each library's functions reside in their own namespace, preventing conflicts between libraries.

## 8.6 Struct, impl, and Trait

Beyond functions, Juglans supports Rust-style struct definitions with `impl` blocks and `trait` contracts.

### Struct + impl Block

`impl` blocks group methods for a struct. Methods with `self` are instance methods; methods without `self` are associated functions (static methods).

The snippet below shows the shape of struct + impl declarations. For a fully-runnable end-to-end example, see [`examples/impl_trait_demo.jg`](https://github.com/juglans-ai/juglans/blob/main/examples/impl_trait_demo.jg) and [`examples/associated_fn_demo.jg`](https://github.com/juglans-ai/juglans/blob/main/examples/associated_fn_demo.jg) in the repo:

```juglans
[Config]: {
  host: str = "localhost"
  port: int = 8080
}

impl Config {
  [info(self)]: output = self.host + ":" + str(self.port)
  [defaults()]: output = "localhost:8080"
}

[default_addr]: Config.defaults()
```

Usage:

```juglans
[d]: Config.defaults()
[c]: new Config(host="0.0.0.0", port=3000)
[i]: c.info()
[c] -> [i]
```

### Trait Definitions

Traits define behavior contracts. Methods without a body are required; methods with a body provide a default implementation:

```juglans
trait Validatable {
  [validate(self)]:
  [is_valid(self)]: output = self.validate().valid == true
}

[ready]: notify(status="trait Validatable defined")
```

### impl Trait for Type

```juglans
trait Validatable {
  [validate(self)]:
}

[User]: {
  name: str
  email: str
}

impl Validatable for User {
  [validate(self)]: valid = self.name != "" and self.email != ""
}

[u]: new User(name="Alice", email="a@b.com")
```

The compiler validates that all required methods are provided. Default methods are automatically inherited.

### Traits in Libraries

Traits, structs, and impl blocks defined in library files are imported with namespace prefixes, just like functions:

```juglans
# lib/models.jg
[User]: {
  name: str
  email: str
}

trait Displayable {
  [format(self)]:
}

impl Displayable for User {
  [format(self)]: output = self.name + " <" + self.email + ">"
}

[demo]: new User(name="Alice", email="a@b.com")
```

Import and use:

```juglans
libs: ["./lib/models.jg"]

[u]: new models.User(name="Alice", email="alice@example.com")
[show]: u.format()
[u] -> [show]
```

## 8.7 Comprehensive Example

Combining function definitions, flow imports, and library imports together:

```juglans
flows: {
  payment: "./flows/payment.jg"
}
libs: ["./lib/helpers.jg"]

# Local function definition
[validate(data)]: {
  print(message="Validating: " + data)
  is_valid = true
}

# Entry
[start]: order_id = "ORD-001"

# Call local function
[check]: validate(data=order_id)

# Call library function
[log]: helpers.log(msg="Order validated: " + order_id)

# Report
[report]: print(message="Order " + order_id + " processed")

# Execution flow: local -> sub-workflow -> report
[start] -> [check]
[check] -> [log]
[log] -> [payment.start]
[payment.done] -> [report]
```

This workflow demonstrates the collaboration of all three composition mechanisms:

1. **Local function** `validate` -- encapsulates validation logic, callable multiple times within the current file.
2. **Flow import** `payment` -- a complete payment sub-process, merged into the graph as a sub-graph.
3. **Library import** `helpers` -- reuses utility functions, called via namespace.

Each mechanism has its own use case:

| Mechanism | Use Case | Characteristics |
|-----------|----------|-----------------|
| Function definitions | Code reuse within the current file | Simplest approach, defined in place |
| `flows:` | Import complete sub-workflows (with nodes and edges) | Merges sub-graph, namespace-isolated |
| `libs:` | Import pure function libraries | Extracts functions only, no sub-graph merging |

## Summary

- **Function definitions** `[name(params)]: body` -- encapsulate reusable logic; call syntax is identical to built-in tools
- **Single-step functions** `[f(x)]: tool(...)` -- directly bind a single tool call
- **Multi-step functions** `[f(x)]: { step1; step2 }` -- multiple steps executed sequentially inside curly braces
- **Struct + impl** `[Type]: { fields }` + `impl Type { methods }` -- Rust-style data + behavior grouping
- **Traits** `trait Name { methods }` + `impl Trait for Type { ... }` -- behavior contracts without inheritance
- **Associated functions** -- methods without `self`, called via `Type.function()`
- **Flow Import** `flows: { alias: "path.jg" }` -- import complete sub-workflows; nodes are automatically namespace-prefixed
- **Cross-workflow edges** `[local] -> [alias.node]` -- reference sub-workflow nodes in edge definitions
- **Library Import** `libs: ["path.jg"]` -- import function libraries, structs, traits, called via `namespace.func()`
- All mechanisms can be combined, each suited for different scenarios

Next chapter: [Tutorial 9: Full Project](./full-project.md) -- integrate all knowledge from the previous 8 chapters to build a complete AI assistant project from scratch.
