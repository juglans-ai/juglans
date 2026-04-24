# Tutorial 6: AI Chat

This chapter covers how to call AI models in workflows: using the `chat()` tool to send messages, defining inline agents, constructing dynamic messages, chaining multi-turn conversations, and obtaining structured JSON output. Juglans is local-first — it calls LLM providers directly using your API keys; no remote backend required.

## 6.1 chat() Basics

The simplest AI call — one agent, one message, one reply:

```juglans
[assistant]: {
  "model": "deepseek-chat",
  "temperature": 0.7,
  "system_prompt": "You are a helpful assistant."
}

[ask]: chat(agent=assistant, message="What is 2+2?")
[show]: print(message=output)

[assistant] -> [ask] -> [show]
```

Line-by-line explanation:

1. `[assistant]: { ... }` — Defines an inline agent as a JSON map node. The node ID `assistant` becomes the agent's identifier.
2. `[ask]: chat(agent=assistant, message="What is 2+2?")` — Calls the `chat()` tool, sending the message `"What is 2+2?"` to the agent referenced by `assistant`. The engine sends the message to the corresponding model and waits for a reply.
3. `[show]: print(message=output)` — The AI's reply is stored in `output`, and `print` outputs it to the console.
4. `[assistant] -> [ask] -> [show]` — First initialize the agent, then call the AI, then print the result.

`chat()` is the most important built-in tool in Juglans. Its minimal usage requires only two parameters:

| Parameter | Purpose |
|------|------|
| `agent` | A reference to an inline agent map node |
| `message` | The message content to send to the AI |

## 6.2 Defining an Agent — Inline JSON Map Nodes

The `chat(agent=assistant, ...)` from the previous section references an agent node named `assistant`. Agents are defined as inline JSON map nodes directly in `.jg` files.

The agent from section 6.1:

```juglans
[assistant]: {
  "model": "deepseek-chat",
  "temperature": 0.7,
  "system_prompt": "You are a helpful assistant."
}
```

Field-by-field explanation:

| Field | Purpose | Example Value |
|------|------|--------|
| `model` | The AI model to use | `"deepseek-chat"`, `"gpt-4o-mini"`, `"claude-haiku-4-5"`, `"gemini-2.0-flash"` |
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
└── chat.jg          # Contains both agent definitions and workflow
```

Everything lives in one `.jg` file. For reuse across files, put agents in a library and import with `libs:`.

## 6.3 Dynamic Messages — Constructing with input

Hardcoded messages are only suitable for testing. In real scenarios, messages come from external input:

```juglans
[assistant]: {
  "model": "deepseek-chat",
  "temperature": 0.7,
  "system_prompt": "You are a helpful assistant."
}

[ask]: chat(agent=assistant, message=input.question)
[result]: print(message=output)

[assistant] -> [ask] -> [result]
```

Run:

```bash
juglans chat.jg --input '{"question": "Explain recursion in one sentence."}'
```

`input.question` is replaced at execution time with `"Explain recursion in one sentence."`, which the AI receives and responds to.

### Concatenating Context

Use `+` to concatenate strings, providing the AI with more context:

```juglans
[assistant]: {
  "model": "deepseek-chat",
  "temperature": 0.7,
  "system_prompt": "You are a helpful assistant."
}

[init]: lang = input.lang
[ask]: chat(agent=assistant, message="Answer in " + lang + ": " + input.question)
[show]: print(message=output)

[assistant] -> [init] -> [ask] -> [show]
```

```bash
juglans chat.jg --input '{"question": "What is Rust?", "lang": "Chinese"}'
```

The AI receives the message `"Answer in Chinese: What is Rust?"` and therefore responds in Chinese.

## 6.4 Multi-Turn Conversations — Chaining chat() Calls

Chain multiple `chat()` nodes together, with the previous node's output feeding into the next. This is the most common pattern in AI workflows:

```juglans
[assistant]: {
  "model": "deepseek-chat",
  "temperature": 0.7,
  "system_prompt": "You are a helpful assistant."
}

[draft]: chat(agent=assistant, message="Write a short poem about the sea.")
[review]: chat(agent=assistant, message="Review this poem and suggest improvements: " + output)
[final]: print(message=output)

[assistant] -> [draft] -> [review] -> [final]
```

Execution flow:

1. `[draft]` — The AI writes a short poem about the sea; the result is stored in `output`.
2. `[review]` — Reads `output` (the poem from the previous step) and asks the AI to review and improve it. The AI's improvement suggestions overwrite `output`.
3. `[final]` — Prints the final review result.

### Saving Intermediate Results

If you still need the original poem later, save it to a variable:

```juglans
[assistant]: {
  "model": "deepseek-chat",
  "temperature": 0.7,
  "system_prompt": "You are a helpful assistant."
}

[draft]: chat(agent=assistant, message="Write a haiku about mountains.")
[save]: poem = output
[review]: chat(agent=assistant, message="Critique this haiku: " + poem)
[show]: print(message="Original: " + poem + " | Review: " + output)

[assistant] -> [draft] -> [save] -> [review] -> [show]
```

`poem` persists throughout the entire workflow and will not be overwritten by subsequent `output` values.

### Using Different Agents

Each node in the chain can use a different agent, leveraging their respective strengths:

```juglans
[translator]: {
  "model": "deepseek-chat",
  "system_prompt": "You are a professional translator."
}

[summarizer]: {
  "model": "deepseek-chat",
  "system_prompt": "You are a summarization expert."
}

[translate]: chat(agent=translator, message="Translate to English: " + input.text)
[summarize]: chat(agent=summarizer, message="Summarize in one sentence: " + output)
[result]: print(message=output)

[translator] -> [translate]
[summarizer] -> [summarize]
[translate] -> [summarize] -> [result]
```

First translate, then summarize — two agents, each with its own responsibility.

## 6.5 JSON Format Output — format="json"

By default, `chat()` returns free-form text. Adding the `format="json"` parameter forces the AI to return structured JSON:

```juglans
[assistant]: {
  "model": "deepseek-chat",
  "temperature": 0.7,
  "system_prompt": "You are a helpful assistant."
}

[analyze]: chat(agent=assistant, message="Analyze the sentiment: " + input.text, format="json")
[show]: print(message=output)

[assistant] -> [analyze] -> [show]
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
[assistant]: {
  "model": "deepseek-chat",
  "temperature": 0.7,
  "system_prompt": "You are a helpful assistant."
}

[classify]: chat(agent=assistant, message="Classify this as positive or negative: " + input.text, format="json")
[pos]: print(message="Positive feedback detected!")
[neg]: print(message="Negative feedback — escalating.")
[done]: print(message="Classification complete.")

[classify] -> switch output.sentiment {
    "positive": [pos]
    "negative": [neg]
    default:    [done]
}
[pos] -> [done]
[neg] -> [done]
```

The AI returns `{"sentiment": "positive"}`, and `switch` takes exactly one branch based on the value — use `switch` (not conditional edges) when branches are mutually exclusive, so you don't have to worry about multiple edges firing.

## 6.6 Configuration Notes

`chat()` calls LLM providers directly using their API keys. The simplest setup is to set the relevant `*_API_KEY` env var:

```bash
export OPENAI_API_KEY="sk-..."
# or ANTHROPIC_API_KEY, DEEPSEEK_API_KEY, GEMINI_API_KEY, QWEN_API_KEY, etc.
```

Or define providers in `juglans.toml`:

```toml
[ai.providers.openai]
api_key = "sk-..."

[ai.providers.deepseek]
api_key = "sk-..."
```

Without any provider configured, `chat()` will fail with `No API-key provided`. See [Connect AI Models](../guide/connect-ai.md) and [Configuration Reference](../reference/config.md) for full details.

## 6.7 Conversation History (Multi-Turn Memory)

When a `chat_id` is resolved, Juglans automatically loads the tail of that thread before the LLM call and appends the user / assistant turn afterwards. You don't thread a history array by hand:

```juglans
[reply]: chat(message = input.text, chat_id = input.user_id)
```

Now the bot remembers what the user said two messages ago — try it: ask the bot its name, then ask "what did you just call yourself?" and it should answer correctly.

`chat_id` resolves in priority order:

1. Explicit `chat_id="..."` parameter (highest)
2. `reply.chat_id` (chained from a prior `chat()` in the same run)
3. `input.chat_id` (auto-injected by bot adapters as `"{platform}:{user}:{agent}"`)
4. None — call is stateless

Inside a bot workflow you usually need zero arguments — the adapter sets `input.chat_id` for you:

```juglans
[reply]: chat(message = input.text)   # auto-history when running via juglans bot ...
```

Backends are configured once in `juglans.toml`:

```toml
[history]
backend = "jsonl"          # or "sqlite", "memory", "none"
max_messages = 20
max_tokens = 8000
```

See [Conversation History in connect-ai.md](../guide/connect-ai.md#conversation-history) for the full story.

## 6.8 Message State (`state=`)

`chat()` accepts a `state=` parameter that controls the message lifecycle on two axes: whether the message persists into history (`context`), and whether it streams to the user via SSE (`display`). Four canonical values:

| state | Persist? | Stream? | When to use |
|---|---|---|---|
| `context_visible` (default) | ✓ | ✓ | Normal turn |
| `context_hidden` | ✓ | ✗ | Internal AI thinking — feeds future turns but user doesn't see it |
| `display_only` | ✗ | ✓ | One-off notice that shouldn't pollute history |
| `silent` | ✗ | ✗ | Diagnostic / classification calls that shouldn't show or persist |

```juglans
# Classify intent without polluting history or streaming to the user
[classify]: chat(message = input.text, state = "silent",
                 system_prompt = "Reply with one word: question | task | smalltalk")

# Save it to context for later branches
[save]: intent = output

# The actual user-facing reply persists normally
[reply]: chat(message = input.text)
```

Compound form `state="input:output"` controls input and output independently — for fine-grained cases like "store the user message but don't stream the response".

## Summary

| Concept | Syntax | Purpose |
|------|------|------|
| AI call | `chat(agent=my_agent, message=...)` | Send a message to an agent and get a reply |
| Agent definition | `[name]: { "model": "...", ... }` | Inline JSON map node defining model, temperature, system prompt |
| Dynamic messages | `message=input.question` | Construct message content using variables |
| Multi-turn chaining | `[a] -> [b]` with multiple chat nodes in sequence | Previous output serves as the next input |
| JSON output | `format="json"` | Force structured output; can be combined with conditional routing |

Key rules:

1. Before using `chat()`, define the agent as an inline JSON map node in the same `.jg` file (or import via `libs:`).
2. The return value of `chat()` is stored in `output`, which the next node can read directly.
3. `format="json"` makes the AI return a JSON object whose fields can be accessed via `output.field`.
4. Multiple `chat()` nodes can use the same or different agents.

## Next Chapter

**[Prompt Templates](./prompts.md)** — Learn the `.jgx` template syntax and the `p()` tool to manage complex prompts with Jinja-style templates.
