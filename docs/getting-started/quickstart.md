# Quick Start

By the end of this page, you'll have written and run your first Juglans workflow. It takes about 5 minutes.

## Prerequisites

- Juglans installed ([Installation Guide](./installation.md))

That's it. No API keys needed for this tutorial — we'll use only offline tools.

## Step 1: Your First Workflow

Create a file called `hello.jg`:

```juglans
[greet]: print(message="Hello, Juglans!")
[done]: print(message="Workflow complete.")
[greet] -> [done]
```

Run it:

```bash
juglans hello.jg
```

You should see:

```
Hello, Juglans!
Workflow complete.
```

**What just happened?** You defined two **nodes** (`[greet]` and `[done]`), each calling the `print()` tool. The arrow `->` is an **edge** that says "run `[done]` after `[greet]`".

## Step 2: Add Input

Workflows become useful when they accept input. Edit `hello.jg`:

```juglans
[greet]: print(message="Hello, " + $input.name + "!")
[info]: print(message="You are " + $input.role)
[greet] -> [info]
```

Run with `--input`:

```bash
juglans hello.jg --input '{"name": "Alice", "role": "developer"}'
```

Output:

```
Hello, Alice!
You are developer
```

**`$input`** refers to the JSON data you pass via `--input`. Use dot notation to access fields: `$input.name`, `$input.role`.

## Step 3: Use Context Variables

Nodes can store data in the workflow **context** and pass it to later nodes:

```juglans
[init]: set_context(greeting="Good morning", count=3)
[show]: print(message=$ctx.greeting + " — count is " + str($ctx.count))
[init] -> [show]
```

```bash
juglans hello.jg
```

Output:

```
Good morning — count is 3
```

**`$ctx`** is the shared context. `set_context()` writes to it, `$ctx.key` reads from it. The `str()` function converts a number to string for concatenation.

## Step 4: Branching

Make your workflow take different paths based on conditions:

```juglans
[check]: set_context(score=85)
[pass]: print(message="Passed!")
[fail]: print(message="Failed.")
[done]: print(message="Evaluation complete.")

[check] if $ctx.score >= 60 -> [pass]
[check] if $ctx.score < 60 -> [fail]
[pass] -> [done]
[fail] -> [done]
```

```bash
juglans hello.jg
```

Output:

```
Passed!
Evaluation complete.
```

Try changing the score to `50` and run again — you'll see `Failed.` instead.

**`if` conditions** on edges control which path the workflow takes. Only edges whose conditions are true will be followed.

## Step 5: A Realistic Workflow

Let's combine everything into a workflow that processes a task:

```juglans
[start]: set_context(status="received")
[validate]: set_context(status="validated")
[process]: print(message="Processing task: " + $input.task)
[success]: print(message="Task completed successfully")
[error]: print(message="Task validation failed")
[done]: print(message="Final status: " + $ctx.status)

[start] -> [validate]
[validate] if $input.priority == "high" -> [process]
[validate] if $input.priority != "high" -> [error]
[process] -> [success]
[success] -> [done]
[error] -> [done]
```

```bash
juglans hello.jg --input '{"task": "deploy v2.0", "priority": "high"}'
```

Output:

```
Processing task: deploy v2.0
Task completed successfully
Final status: validated
```

## Step 6: Project Structure

A real Juglans project uses the `src/` layout:

```
src/
├── main.jg                    # Main workflow
├── agents/
│   └── assistant.jgagent      # Workflow-bound agent (source: "../main.jg")
├── pure-agents/
│   └── helper.jgagent         # Pure agent (used inside workflows)
├── prompts/
│   └── system.jgprompt        # Prompt templates
└── tools/                     # Tool definitions
```

Fork the [starter template](https://github.com/juglans-ai/juglans-template) to get this structure ready to go.

## What's Next?

You've learned the core building blocks:

| Concept | Syntax | You learned |
|---------|--------|-------------|
| **Node** | `[name]: tool(params)` | Step 1 |
| **Edge** | `[a] -> [b]` | Step 1 |
| **Input** | `$input.field` | Step 2 |
| **Context** | `set_context()` / `$ctx.key` | Step 3 |
| **Conditionals** | `[a] if expr -> [b]` | Step 4 |
| **Composition** | Combine nodes into real workflows | Step 5 |
| **Structure** | `src/` layout with agents, prompts, tools | Step 6 |

Continue with the tutorials to learn the language in depth:

- **[Tutorial 1: Hello Workflow](../tutorials/hello-workflow.md)** — Deeper dive into nodes and edges
- **[Tutorial 2: Variables & Data Flow](../tutorials/variables.md)** — Master `$input`, `$output`, `$ctx`
- **[Tutorial 3: Branching & Routing](../tutorials/branching.md)** — `if`, `switch`, complex routing

Or jump to the reference if you want to explore on your own:

- **[Built-in Tools](../reference/builtins.md)** — All available tools: `chat`, `print`, `bash`, `fetch`, etc.
- **[CLI Commands](../reference/cli.md)** — Everything `juglans` can do
