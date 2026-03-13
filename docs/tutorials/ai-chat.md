# Tutorial 6: AI Chat

This chapter covers how to call AI models in workflows: using the `chat()` tool to send messages, creating `.jgagent` configuration files, constructing dynamic messages, chaining multi-turn conversations, and obtaining structured JSON output.

## 6.1 chat() Basics

The simplest AI call — one node, one message, one reply:

```juglans
agents: ["./agents/*.jgagent"]

[ask]: chat(agent="assistant", message="What is 2+2?")
[show]: print(message=output)

[ask] -> [show]
```

Line-by-line explanation:

1. `agents: ["./agents/*.jgagent"]` — A metadata declaration that tells the engine to load all `.jgagent` configuration files from the `./agents/` directory. Only after loading an agent can `chat()` find it.
2. `[ask]: chat(agent="assistant", message="What is 2+2?")` — Calls the `chat()` tool, sending the message `"What is 2+2?"` to the agent with slug `"assistant"`. The engine sends the message to the corresponding model and waits for a reply.
3. `[show]: print(message=output)` — The AI's reply is stored in `output`, and `print` outputs it to the console.
4. `[ask] -> [show]` — First call the AI, then print the result.

`chat()` is the most important built-in tool in Juglans. Its minimal usage requires only two parameters:

| Parameter | Purpose |
|------|------|
| `agent` | The agent's slug, corresponding to the `slug` field in the `.jgagent` file |
| `message` | The message content to send to the AI |

## 6.2 Creating an Agent — .jgagent Files

The `chat(agent="assistant", ...)` from the previous section references an agent named `assistant`. This agent is defined in a `.jgagent` file.

Create `agents/assistant.jgagent`:

```jgagent
slug: "assistant"
name: "General Assistant"
model: "deepseek-chat"
temperature: 0.7
system_prompt: "You are a helpful assistant."
```

Field-by-field explanation:

| Field | Purpose | Example Value |
|------|------|--------|
| `slug` | Unique identifier; referenced by `chat()` | `"assistant"` |
| `name` | Display name, used in UI and logs | `"General Assistant"` |
| `model` | The AI model to use | `"deepseek-chat"`, `"gpt-4o"`, `"claude-3-sonnet"` |
| `temperature` | Randomness control (0 = deterministic, 2 = high randomness) | `0.7` (recommended default) |
| `system_prompt` | System prompt that defines the agent's role and behavior | `"You are a helpful assistant."` |

### Temperature Selection Guide

| Value | Use Case |
|----|----------|
| `0.0` | Classification, extraction, JSON output — consistency required |
| `0.3` | Code generation, technical Q&A — accuracy first |
| `0.7` | General conversation — balance creativity and accuracy |
| `1.0+` | Creative writing, brainstorming — encourage diversity |

### Directory Structure

```text
my-project/
├── chat.jg
└── agents/
    └── assistant.jgagent
```

The paths in `agents:` metadata are relative to the directory where the `.jg` file is located. `["./agents/*.jgagent"]` matches all `.jgagent` files under `agents/`.

## 6.3 Dynamic Messages — Constructing with input

Hardcoded messages are only suitable for testing. In real scenarios, messages come from external input:

```juglans
agents: ["./agents/*.jgagent"]

[ask]: chat(agent="assistant", message=input.question)
[result]: print(message=output)

[ask] -> [result]
```

Run:

```bash
juglans chat.jg --input '{"question": "Explain recursion in one sentence."}'
```

`input.question` is replaced at execution time with `"Explain recursion in one sentence."`, which the AI receives and responds to.

### Concatenating Context

Use `+` to concatenate strings, providing the AI with more context:

```juglans
agents: ["./agents/*.jgagent"]

[init]: lang = input.lang
[ask]: chat(agent="assistant", message="Answer in " + lang + ": " + input.question)
[show]: print(message=output)

[init] -> [ask] -> [show]
```

```bash
juglans chat.jg --input '{"question": "What is Rust?", "lang": "Chinese"}'
```

The AI receives the message `"Answer in Chinese: What is Rust?"` and therefore responds in Chinese.

## 6.4 Multi-Turn Conversations — Chaining chat() Calls

Chain multiple `chat()` nodes together, with the previous node's output feeding into the next. This is the most common pattern in AI workflows:

```juglans
agents: ["./agents/*.jgagent"]

[draft]: chat(agent="assistant", message="Write a short poem about the sea.")
[review]: chat(agent="assistant", message="Review this poem and suggest improvements: " + output)
[final]: print(message=output)

[draft] -> [review] -> [final]
```

Execution flow:

1. `[draft]` — The AI writes a short poem about the sea; the result is stored in `output`.
2. `[review]` — Reads `output` (the poem from the previous step) and asks the AI to review and improve it. The AI's improvement suggestions overwrite `output`.
3. `[final]` — Prints the final review result.

### Saving Intermediate Results

If you still need the original poem later, save it to a variable:

```juglans
agents: ["./agents/*.jgagent"]

[draft]: chat(agent="assistant", message="Write a haiku about mountains.")
[save]: poem = output
[review]: chat(agent="assistant", message="Critique this haiku: " + poem)
[show]: print(message="Original: " + poem + " | Review: " + output)

[draft] -> [save] -> [review] -> [show]
```

`poem` persists throughout the entire workflow and will not be overwritten by subsequent `output` values.

### Using Different Agents

Each node in the chain can use a different agent, leveraging their respective strengths:

```juglans
agents: ["./agents/*.jgagent"]

[translate]: chat(agent="translator", message="Translate to English: " + input.text)
[summarize]: chat(agent="summarizer", message="Summarize in one sentence: " + output)
[result]: print(message=output)

[translate] -> [summarize] -> [result]
```

First translate, then summarize — two agents, each with its own responsibility.

## 6.5 JSON Format Output — format="json"

By default, `chat()` returns free-form text. Adding the `format="json"` parameter forces the AI to return structured JSON:

```juglans
agents: ["./agents/*.jgagent"]

[analyze]: chat(agent="assistant", message="Analyze the sentiment: " + input.text, format="json")
[show]: print(message=output)

[analyze] -> [show]
```

```bash
juglans analyze.jg --input '{"text": "I love this product!"}'
```

Example AI JSON response:

```json
{"sentiment": "positive", "confidence": 0.95}
```

What `format="json"` does:

- Adds a JSON output constraint (response_format) to the model
- The return value is a JSON object, and internal fields can be accessed via paths like `output.sentiment`

### JSON Output + Conditional Routing

The most powerful use of JSON output is combining it with conditional routing, letting the AI make decisions:

```juglans
agents: ["./agents/*.jgagent"]

[classify]: chat(agent="assistant", message="Classify this as positive or negative: " + input.text, format="json")
[pos]: print(message="Positive feedback detected!")
[neg]: print(message="Negative feedback — escalating.")
[done]: print(message="Classification complete.")

[classify] if output.sentiment == "positive" -> [pos]
[classify] if output.sentiment == "negative" -> [neg]
[classify] -> [done]
[pos] -> [done]
[neg] -> [done]
```

The AI returns `{"sentiment": "positive"}`, and the workflow automatically routes to `[pos]` or `[neg]` based on `output.sentiment`.

## 6.6 Configuration Notes

`chat()` communicates with AI models through the Jug0 backend service. For `chat()` to work, you need to create a `juglans.toml` in the project root:

```toml
[account]
id = "your_user_id"
api_key = "jug0_sk_..."

[jug0]
base_url = "http://localhost:3000"
```

| Field | Purpose |
|------|------|
| `account.id` | Jug0 account ID |
| `account.api_key` | API key |
| `jug0.base_url` | Jug0 service URL |

Without this configuration, `chat()` will return a connection error. See the [Jug0 Integration Guide](../integrations/jug0.md) for details.

## Summary

| Concept | Syntax | Purpose |
|------|------|------|
| AI call | `chat(agent="slug", message=...)` | Send a message to an agent and get a reply |
| Agent configuration | `.jgagent` file | Define model, temperature, and system prompt |
| Loading agents | `agents: ["./agents/*.jgagent"]` | Import agent files into a workflow |
| Dynamic messages | `message=input.question` | Construct message content using variables |
| Multi-turn chaining | `[a] -> [b]` with multiple chat nodes in sequence | Previous output serves as the next input |
| JSON output | `format="json"` | Force structured output; can be combined with conditional routing |

Key rules:

1. Before using `chat()`, you must load agent files via `agents:` metadata.
2. The return value of `chat()` is stored in `output`, which the next node can read directly.
3. `format="json"` makes the AI return a JSON object whose fields can be accessed via `output.field`.
4. Multiple `chat()` nodes can use the same or different agents.

## Next Chapter

**[Tutorial 7: Prompt Templates](./prompt-templates.md)** — Learn the `.jgprompt` template syntax and the `p()` tool to manage complex prompts with Jinja-style templates.
