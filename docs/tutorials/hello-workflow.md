# Tutorial 1: Hello Workflow

By the end of this chapter, you will understand four core concepts: **Nodes**, **Edges**, **Tools**, and the basic structure of a `.jg` file.

## Minimal Workflow

Create a file called `hello.jg` with the following content:

```juglans
[greet]: print(message="Hello!")
[done]: print(message="Done.")
[greet] -> [done]
```

Run it:

```bash
juglans hello.jg
```

Output:

```
Hello!
Done.
```

Three lines of code, three concepts:

- `[greet]` and `[done]` are **nodes**. Square brackets wrap a unique name, representing a single execution unit in the workflow.
- `print(message="Hello!")` is a **tool call**. `print` is the tool name, `message` is the parameter. A node binds a tool call using `:`.
- `[greet] -> [done]` is an **edge**. The arrow `->` defines execution order: `done` runs after `greet` completes.

A workflow is simply "connecting nodes with edges."

## Multi-Node Chain Execution

Nodes can be chained into a sequence of any length using `->`:

```juglans
[step1]: print(message="Step 1: Preparing data")
[step2]: print(message="Step 2: Processing")
[step3]: print(message="Step 3: Formatting output")
[step4]: print(message="Step 4: Complete")
[step1] -> [step2] -> [step3] -> [step4]
```

Execution order strictly follows the direction of the edges. Internally, Juglans builds all nodes and edges into a **DAG (Directed Acyclic Graph)** and determines execution order via **topological sort** -- in simple terms, "do things with no dependencies first, then do things that depend on them."

In a linear chain, the topological sort result matches the order you wrote: step1, step2, step3, step4.

## Entry Nodes

All examples so far have worked -- but you might wonder: how does Juglans know which node to start from?

The answer is **topological sort**. Juglans analyzes all edges and identifies nodes with **in-degree 0** (no incoming edges) as entry points. In the example below, `greet` has no node pointing to it, so it automatically becomes the entry node:

```juglans
[greet]: print(message="Hello, Juglans!")
[log]: print(message="Workflow is running...")
[done]: print(message="Goodbye!")

[greet] -> [log] -> [done]
```

No extra declarations needed -- just define your nodes and edges clearly, and the execution order is determined naturally.

## Exploring More Tools

`print` is great for debugging, but Juglans comes with more built-in tools. Here are the three most commonly used ones:

### print()

The simplest output tool. Prints the value of the `message` parameter to the console.

```juglans
[hello]: print(message="Hello, World!")
```

### notify()

Sends a status notification. Accepts a `status` parameter, used to display workflow progress in the console or UI.

```juglans
[start]: notify(status="Workflow started")
[process]: print(message="Processing...")
[done]: notify(status="Workflow completed")

[start] -> [process] -> [done]
```

The difference between `print` and `notify`: `print` produces plain text output, while `notify` carries semantic meaning (it is a status notification) and renders with a different style in the Web UI.

### Assignment Syntax

Sets **context variables** using `key = value` pairs. Once stored in the context, subsequent nodes can read them by name.

```juglans
[start]: print(message="Starting workflow")
[save]: user = "Alice", score = 100
[report]: notify(status="User saved: Alice")
[done]: print(message="All done")

[start] -> [save] -> [report] -> [done]
```

Assignment produces no visible output, but it changes the workflow's internal state. The variable system is the topic of the next chapter; for now, just know that assignment means "writing something into the workflow's memory."

## Combining Tools

Put the tools you have learned together:

```juglans
[init]: notify(status="Pipeline starting...")
[setup]: stage = "prepared"
[work]: print(message="Doing the real work here")
[report]: notify(status="Work complete")
[finish]: print(message="Pipeline finished")

[init] -> [setup] -> [work] -> [report] -> [finish]
```

This workflow demonstrates a typical pattern: use `notify` to mark key milestones, assignment syntax to record intermediate state, and `print` for debug output.

## Common Errors

### Duplicate Node Names

```juglans,ignore
[step]: print(message="first")
[step]: print(message="second")
[step] -> [step]
```

Two nodes in the same workflow use the same name `step`. The parser will report an error:

```
Error: Duplicate node ID: step
```

Every node name must be unique within the entire workflow.

### Referencing an Undefined Node

```juglans,ignore
[start]: print(message="Hello")
[start] -> [end]
```

The edge `[start] -> [end]` references node `end`, which was never defined. The validator will report an error:

```
Error: Edge references undefined node: end
```

All nodes referenced by edges must be defined first. Define nodes first, then write edges -- this is a fundamental convention of `.jg` files.

## Summary

This chapter covered the fundamentals of Juglans workflows:

- **Nodes** `[name]` are execution units, bound to tool calls via `:`
- **Edges** `->` define execution order
- **Tools** are the actual behavior of nodes: `print` outputs text, `notify` sends notifications, assignment syntax writes to the context
- **Entry nodes** are automatically determined by topological sort -- nodes with in-degree 0 are entry points, no extra declarations needed

Next chapter: [Tutorial 2: Variables & Data Flow](./variables.md) -- learn about `input`, `output`, context variables, and how to pass data between nodes.
