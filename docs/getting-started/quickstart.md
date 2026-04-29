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
[greet]: print(message="Hello, " + input.name + "!")
[info]: print(message="You are " + input.role)
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

**`input`** refers to the JSON data you pass via `--input`. Use dot notation to access fields: `input.name`, `input.role`.

## Step 3: Use Context Variables

Nodes can store data in the workflow **context** and pass it to later nodes. **Replace** the contents of `hello.jg` with:

```juglans
[init]: greeting = "Good morning", count = 3
[show]: print(message=greeting + " — count is " + str(count))
[init] -> [show]
```

```bash
juglans hello.jg
```

Output:

```
Good morning — count is 3
```

Variables are shared across the workflow. Use assignment syntax to set them, and read by name. The `str()` function converts a number to string for concatenation.

## Step 4: Branching

Make your workflow take different paths based on conditions:

```juglans
[check]: score = 85
[pass]: print(message="Passed!")
[fail]: print(message="Failed.")
[done]: print(message="Evaluation complete.")

[check] if score >= 60 -> [pass]
[check] if score < 60 -> [fail]
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
[start]: status = "received"
[validate]: status = "validated"
[process]: print(message="Processing task: " + input.task)
[success]: print(message="Task completed successfully")
[error]: print(message="Task validation failed")
[done]: print(message="Final status: " + status)

[start] -> [validate]
[validate] if input.priority == "high" -> [process]
[validate] if input.priority != "high" -> [error]
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

A real Juglans project typically uses the `src/` layout:

```
src/
├── main.jg                    # Main workflow (with inline agent definitions)
├── agents.jg                  # Shared agent library (imported via libs:)
├── prompts/
│   └── system.jgx        # Prompt templates
└── tools/                     # Tool definitions
```

Note: the `src/` layout is a convention, not enforced — Juglans will execute any `.jg` file in any directory. Fork the [starter template](https://github.com/juglans-ai/juglans-template) to get this structure ready to go.

Other commands you'll use often: `juglans check` validates workflows before running, `juglans test` runs workflow tests, and `juglans serve` exposes workflows as an HTTP API plus auto-starts every configured channel (Telegram / Discord / Feishu / WeChat).

## Step 7: Add an AI Call (optional)

Set one provider API key and call `chat()`:

```bash
export OPENAI_API_KEY="sk-..."
# or ANTHROPIC_API_KEY / DEEPSEEK_API_KEY / GEMINI_API_KEY / QWEN_API_KEY / etc.
```

```juglans
[assistant]: { "model": "gpt-4o-mini", "system_prompt": "You are concise." }

[ask]: chat(agent=assistant, message=input.question)
[show]: print(message=output)

[assistant] -> [ask] -> [show]
```

```bash
juglans hello.jg --input '{"question": "What is Rust good for?"}'
```

You should see a one-paragraph answer. From here you can:

- Turn this into a Telegram / Discord chat by adding `[channels.<kind>.<id>]` to `juglans.toml` and running `juglans serve` — see [Connect AI Models](../guide/connect-ai.md).
- Push outbound notifications using `telegram.send_message` / `discord.send_message` / `wechat.send_message` / `feishu.send_message`.
- Add multi-turn memory automatically via `[history]` config (loads per `chat_id`).

## What's Next?

You've learned the core building blocks:

| Concept | Syntax | You learned |
|---------|--------|-------------|
| **Node** | `[name]: tool(params)` | Step 1 |
| **Edge** | `[a] -> [b]` | Step 1 |
| **Input** | `input.field` | Step 2 |
| **Context** | `key = value` (assignment syntax) | Step 3 |
| **Conditionals** | `[a] if expr -> [b]` | Step 4 |
| **Composition** | Combine nodes into real workflows | Step 5 |
| **Structure** | `src/` layout with agents, prompts, tools | Step 6 |

Continue with the tutorials to learn the language in depth:

- **[Tutorial 1: Hello Workflow](../tutorials/hello-workflow.md)** — Deeper dive into nodes and edges
- **[Tutorial 2: Variables & Data Flow](../tutorials/variables.md)** — Master `input`, `output`, context variables
- **[Tutorial 3: Branching & Routing](../tutorials/branching.md)** — `if`, `switch`, complex routing

Or jump to the reference if you want to explore on your own:

- **[Built-in Tools](../reference/builtins.md)** — All available tools: `chat`, `print`, `bash`, `fetch`, etc.
- **[CLI Commands](../reference/cli.md)** — Everything `juglans` can do
