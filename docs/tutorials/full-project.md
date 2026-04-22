# Tutorial 9: Full Project -- Building an AI Assistant

The previous 8 chapters covered all of Juglans' core features. This chapter brings them all together to build a complete **Intent Router + QA AI Assistant** from scratch.

## 9.1 Project Requirements

We are building an AI assistant that can:

1. Receive user input
2. Classify intent (question / task / chat)
3. Route to different handler agents based on intent
4. Format and output the result

Architecture diagram:

```text
                         ┌─ [handle_qa] ──┐
[receive] → [classify] → switch ─ [handle_task] ─→ [format] → [output]
                         └─ [handle_chat] ┘
                  on error → [error_handler] ──→ [handle_qa]
```

## 9.2 Project Structure

```text
ai-assistant/
├── juglans.toml
├── main.jg
└── prompts/
    ├── classify.jgx
    └── format.jgx
```

Agents are defined as inline JSON map nodes directly in `main.jg` -- no separate agent files needed.

## 9.3 Defining Agents (Inline)

Agents are defined directly in `main.jg` as inline JSON map nodes. We will define two agents:

- **classifier** -- `temperature: 0.0` because classification tasks require deterministic output
- **qa** -- `temperature: 0.3` because technical Q&A favors accuracy

These definitions appear in section 9.5 as part of the complete workflow.

## 9.4 Creating Prompt Files

### classify.jgx -- Classification Prompt Template

```jgx
---
slug: "classify"
name: "Classification Prompt"
inputs:
  message: ""
---
Classify the following user message into one of these categories: question, task, chat.

User message: {{ message }}

Return ONLY a JSON object with "intent" and "confidence" fields.
```

### format.jgx -- Output Formatting Template

```jgx
---
slug: "format"
name: "Response Formatter"
inputs:
  intent: "unknown"
  response: ""
---
## Result

**Intent:** {{ intent }}

**Response:**
{{ response }}
```

## 9.5 Writing the Main Workflow

Create `main.jg`:

```juglans
prompts: ["./prompts/*.jgx"]

# Agent definitions
[classifier]: {
  "model": "deepseek-chat",
  "temperature": 0.0,
  "system_prompt": "You are an intent classifier. Analyze the user's message and classify it.\n\nCategories:\n- question: User is asking for information or explanation\n- task: User wants something done (create, edit, send, etc.)\n- chat: General conversation, greetings, small talk\n\nRespond with ONLY a JSON object:\n{\"intent\": \"question\" | \"task\" | \"chat\", \"confidence\": 0.0-1.0}"
}

[qa]: {
  "model": "deepseek-chat",
  "temperature": 0.3,
  "system_prompt": "You are a knowledgeable QA expert. Answer questions accurately.\nIf you don't know something, say so honestly.\nBe concise and helpful."
}

# Step 1: Receive input and save to context
[receive]: user_message = input.message

# Step 2: Use the classifier agent to determine intent, requiring JSON output
[classify]: chat(
  agent=classifier,
  message=user_message,
  format="json"
)

# Step 3: Save the classification result
[save_intent]: intent = output.intent

# Step 4: Handler nodes -- three branches
[handle_qa]: chat(agent=qa, message=user_message)
[handle_task]: chat(agent=qa, message="Help the user complete this task: " + user_message)
[handle_chat]: chat(agent=qa, message="Respond casually to: " + user_message)

# Step 5: Fallback node (also initializes intent so downstream formatting has a value)
[fallback]: intent = "unknown"

# Step 6: Error handling
[error_handler]: intent = "unknown", print(message="Classification failed: " + error.message)

# Step 7: Format output
[format]: response = output
[output]: print(message="[" + intent + "] " + response)

# --- Edge definitions ---

# Main flow
[classifier] -> [receive]
[qa] -> [receive]
[receive] -> [classify]
[classify] -> [save_intent]

# Error handling: route to QA on classification failure
[classify] on error -> [error_handler]
[error_handler] -> [handle_qa]

# Intent routing
[save_intent] -> switch intent {
    "question": [handle_qa]
    "task": [handle_task]
    "chat": [handle_chat]
    default: [fallback]
}

# Fallback also routes to QA
[fallback] -> [handle_qa]

# All branches converge at formatting
[handle_qa] -> [format]
[handle_task] -> [format]
[handle_chat] -> [format]

# Format -> Output
[format] -> [output]
```

Section-by-section explanation:

**Metadata and agents** (top section)

- `prompts:` loads prompt template files so that `p()` can find them.
- `[classifier]` and `[qa]` are inline agent map nodes defining model configuration.
- The entry nodes are determined by topological sort (nodes with in-degree 0).

**Node definitions**

| Node | Tool | Purpose |
|------|------|---------|
| `[receive]` | assignment | Stores `input.message` into `user_message` |
| `[classify]` | `chat(format="json")` | Uses the classifier agent to classify intent, returns JSON |
| `[save_intent]` | assignment | Stores `output.intent` into `intent` |
| `[handle_qa]` | `chat()` | QA agent answers questions |
| `[handle_task]` | `chat()` | QA agent handles tasks |
| `[handle_chat]` | `chat()` | QA agent handles casual chat |
| `[fallback]` | `print()` | Fallback for unknown intents |
| `[error_handler]` | `print()` | Handles classification errors |
| `[format]` | assignment | Saves the response to context |
| `[output]` | `print()` | Final output |

**Edge definitions**

- `[classify] on error -> [error_handler]` -- on classification failure, instead of terminating, route to error handling, then to QA.
- `switch intent { ... }` -- three-way mutually exclusive routing; unmatched cases go to `default: [fallback]`.
- All branches ultimately converge at `[format] -> [output]`.

## 9.6 Running and Testing

### Syntax Check

```bash
juglans check ai-assistant/
```

Expected output:

```text
Finished checking 1 workflow(s) - 1 valid
```

### Running -- Question Type

```bash
juglans main.jg --input '{"message": "What is Rust programming language?"}'
```

Expected output (example):

```text
[question] Rust is a systems programming language focused on safety, speed, and concurrency...
```

### Running -- Task Type

```bash
juglans main.jg --input '{"message": "Write a haiku about the ocean"}'
```

Expected output (example):

```text
[task] Waves crash on the shore / Salt air fills the twilight sky / Ocean never sleeps
```

### Running -- Chat Type

```bash
juglans main.jg --input '{"message": "Hey, how are you?"}'
```

Expected output (example):

```text
[chat] Hey! I'm doing well, thanks for asking. How can I help you today?
```

## 9.7 Extension Ideas

This project is a starting point. Here are several directions for enhancement:

**Adding More Agents**

Create dedicated agents for each intent type (instead of reusing the qa agent for everything), so each agent's system_prompt is more targeted:

```juglans
[qa]: { "model": "deepseek-chat", "system_prompt": "You are a QA expert." }
[task_executor]: { "model": "deepseek-chat", "system_prompt": "You help users complete tasks." }
[chat_companion]: { "model": "deepseek-chat", "system_prompt": "You are a friendly conversationalist." }

[handle_qa]: chat(agent=qa, message=user_message)
[handle_task]: chat(agent=task_executor, message=user_message)
[handle_chat]: chat(agent=chat_companion, message=user_message)
[format]: print(message=output)

[handle_qa] -> [format]
[handle_task] -> [format]
[handle_chat] -> [format]
```

**Using Prompt Templates to Construct Messages**

Use `p()` to render a prompt, then pass the result to `chat()`:

```juglans
prompts: ["./prompts/*.jgx"]

[classifier]: {
  "model": "deepseek-chat",
  "temperature": 0.0,
  "system_prompt": "Classify user intent. Return JSON."
}

[build_prompt]: p(slug="classify", message=input.message)
[classify]: chat(agent=classifier, message=output, format="json")
[show]: print(message=output)

[classifier] -> [build_prompt] -> [classify] -> [show]
```

**Adding a Summarization Step**

Have the summarizer agent refine the answer before output:

```juglans
[qa]: { "model": "deepseek-chat", "temperature": 0.3, "system_prompt": "You are a QA expert." }
[summarizer]: { "model": "deepseek-chat", "temperature": 0.5, "system_prompt": "Condense information into brief summaries." }

[answer]: chat(agent=qa, message=input.message)
[save]: raw_answer = output
[summarize]: chat(agent=summarizer, message="Summarize: " + raw_answer)
[done]: print(message=output)

[qa] -> [answer]
[summarizer] -> [summarize]
[answer] -> [save] -> [summarize] -> [done]
```

**Web Server Mode**

Use `juglans web` to expose the workflow as an HTTP API, with the frontend receiving streaming responses via SSE:

```bash
juglans web --port 8080
```

```bash
curl -X POST http://localhost:8080/api/chat \
  -H "Content-Type: application/json" \
  -d '{"message": "What is Rust?"}'
```

Note: `serve()` is fallback-based. Any request path is routed into the workflow unless you pattern-match on `input.path`, so the `/api/chat` path above is illustrative -- any URL under the host will reach the same workflow entry.

## 9.8 Tutorial Series Review

Nine chapters, each focused on a core topic:

| Chapter | Topic | Key Concepts |
|---------|-------|--------------|
| Tutorial 1 | Hello Workflow | Nodes, edges, `print()`, `notify()`, entry nodes |
| Tutorial 2 | Variables & Data Flow | `input`, `output`, assignment syntax, `str()` |
| Tutorial 3 | Branching & Routing | `if` conditional edges, `switch` multi-way routing, branch convergence |
| Tutorial 4 | Loops | `foreach`, `while`, accumulating context in loops |
| Tutorial 5 | Error Handling | `on error`, `error`, fallback patterns |
| Tutorial 6 | AI Chat | `chat()`, inline agent maps, `format="json"`, multi-turn conversations |
| Tutorial 7 | Prompt Templates | `.jgx`, `p()`, Jinja templates, combining prompts with chat |
| Tutorial 8 | Workflow Composition | `flows:` import, namespaced nodes, cross-file routing |
| Tutorial 9 | Full Project | Putting it all together: multiple agents, prompts, switch, on error |

From the `print("Hello!")` in Chapter 1 to the multi-agent intent routing system in Chapter 9, you have now mastered the complete toolchain for building AI workflows with Juglans.

Next steps: read the [How-to Guides](../guide/concepts.md) to dive deeper into specific topics, or check the [Reference](../reference/cli.md) for complete CLI and built-in tool documentation.
