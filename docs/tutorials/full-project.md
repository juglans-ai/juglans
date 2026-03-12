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
├── agents/
│   ├── classifier.jgagent
│   ├── qa.jgagent
│   └── summarizer.jgagent
└── prompts/
    ├── classify.jgprompt
    └── format.jgprompt
```

## 9.3 Creating Agent Files

### classifier.jgagent -- Intent Classifier

```jgagent
slug: "classifier"
name: "Intent Classifier"
model: "deepseek-chat"
temperature: 0.0
system_prompt: |
  You are an intent classifier. Analyze the user's message and classify it.

  Categories:
  - question: User is asking for information or explanation
  - task: User wants something done (create, edit, send, etc.)
  - chat: General conversation, greetings, small talk

  Respond with ONLY a JSON object:
  {"intent": "question" | "task" | "chat", "confidence": 0.0-1.0}
```

`temperature: 0.0` -- classification tasks require deterministic output, no randomness.

### qa.jgagent -- QA Expert

```jgagent
slug: "qa"
name: "QA Expert"
model: "deepseek-chat"
temperature: 0.3
system_prompt: |
  You are a knowledgeable QA expert. Answer questions accurately.
  If you don't know something, say so honestly.
  Be concise and helpful.
```

`temperature: 0.3` -- technical Q&A favors accuracy, so creativity is turned down.

### summarizer.jgagent -- Summary Generator

```jgagent
slug: "summarizer"
name: "Summarizer"
model: "deepseek-chat"
temperature: 0.5
system_prompt: |
  You are a summarization assistant. Condense information into brief, clear summaries.
  Keep the key points. Remove redundancy.
```

## 9.4 Creating Prompt Files

### classify.jgprompt -- Classification Prompt Template

```jgprompt
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

### format.jgprompt -- Output Formatting Template

```jgprompt
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
agents: ["./agents/*.jgagent"]
prompts: ["./prompts/*.jgprompt"]

# Step 1: Receive input and save to context
[receive]: set_context(user_message=$input.message)

# Step 2: Use the classifier agent to determine intent, requiring JSON output
[classify]: chat(
  agent="classifier",
  message=$ctx.user_message,
  format="json"
)

# Step 3: Save the classification result
[save_intent]: set_context(intent=$output.intent)

# Step 4: Handler nodes -- three branches
[handle_qa]: chat(agent="qa", message=$ctx.user_message)
[handle_task]: chat(agent="qa", message="Help the user complete this task: " + $ctx.user_message)
[handle_chat]: chat(agent="qa", message="Respond casually to: " + $ctx.user_message)

# Step 5: Fallback node
[fallback]: print(message="Unknown intent, routing to QA")

# Step 6: Error handling
[error_handler]: print(message="Classification failed: " + $error.message)

# Step 7: Format output
[format]: set_context(response=$output)
[output]: print(message="[" + $ctx.intent + "] " + $ctx.response)

# --- Edge definitions ---

# Main flow
[receive] -> [classify]
[classify] -> [save_intent]

# Error handling: route to QA on classification failure
[classify] on error -> [error_handler]
[error_handler] -> [handle_qa]

# Intent routing
[save_intent] -> switch $ctx.intent {
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

**Metadata** (lines 1-2)

- `agents:` and `prompts:` load resource files so that `chat()` and `p()` can find the corresponding agents and templates.
- The entry node is automatically determined by topological sort (the node with in-degree 0, which is `[receive]`).

**Node definitions**

| Node | Tool | Purpose |
|------|------|---------|
| `[receive]` | `set_context()` | Stores `$input.message` into `$ctx.user_message` |
| `[classify]` | `chat(format="json")` | Uses the classifier agent to classify intent, returns JSON |
| `[save_intent]` | `set_context()` | Stores `$output.intent` into `$ctx.intent` |
| `[handle_qa]` | `chat()` | QA agent answers questions |
| `[handle_task]` | `chat()` | QA agent handles tasks |
| `[handle_chat]` | `chat()` | QA agent handles casual chat |
| `[fallback]` | `print()` | Fallback for unknown intents |
| `[error_handler]` | `print()` | Handles classification errors |
| `[format]` | `set_context()` | Saves the response to context |
| `[output]` | `print()` | Final output |

**Edge definitions**

- `[classify] on error -> [error_handler]` -- on classification failure, instead of terminating, route to error handling, then to QA.
- `switch $ctx.intent { ... }` -- three-way mutually exclusive routing; unmatched cases go to `default: [fallback]`.
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
agents: ["./agents/*.jgagent"]

[handle_qa]: chat(agent="qa", message=$ctx.user_message)
[handle_task]: chat(agent="task-executor", message=$ctx.user_message)
[handle_chat]: chat(agent="chat-companion", message=$ctx.user_message)
[format]: print(message=$output)

[handle_qa] -> [format]
[handle_task] -> [format]
[handle_chat] -> [format]
```

**Using Prompt Templates to Construct Messages**

Use `p()` to render a prompt, then pass the result to `chat()`:

```juglans
prompts: ["./prompts/*.jgprompt"]
agents: ["./agents/*.jgagent"]

[build_prompt]: p(slug="classify", message=$input.message)
[classify]: chat(agent="classifier", message=$output, format="json")
[show]: print(message=$output)

[build_prompt] -> [classify] -> [show]
```

**Adding a Summarization Step**

Have the summarizer agent refine the answer before output:

```juglans
agents: ["./agents/*.jgagent"]

[answer]: chat(agent="qa", message=$input.message)
[save]: set_context(raw_answer=$output)
[summarize]: chat(agent="summarizer", message="Summarize: " + $ctx.raw_answer)
[done]: print(message=$output)

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

## 9.8 Tutorial Series Review

Nine chapters, each focused on a core topic:

| Chapter | Topic | Key Concepts |
|---------|-------|--------------|
| Tutorial 1 | Hello Workflow | Nodes, edges, `print()`, `notify()`, entry nodes |
| Tutorial 2 | Variables & Data Flow | `$input`, `$output`, `$ctx`, `set_context()`, `str()` |
| Tutorial 3 | Branching & Routing | `if` conditional edges, `switch` multi-way routing, branch convergence |
| Tutorial 4 | Loops | `foreach`, `while`, accumulating `$ctx` in loops |
| Tutorial 5 | Error Handling | `on error`, `$error`, fallback patterns |
| Tutorial 6 | AI Chat | `chat()`, `.jgagent`, `format="json"`, multi-turn conversations |
| Tutorial 7 | Prompt Templates | `.jgprompt`, `p()`, Jinja templates, combining prompts with chat |
| Tutorial 8 | Workflow Composition | `flows:` import, namespaced nodes, cross-file routing |
| Tutorial 9 | Full Project | Putting it all together: multiple agents, prompts, switch, on error |

From the `print("Hello!")` in Chapter 1 to the multi-agent intent routing system in Chapter 9, you have now mastered the complete toolchain for building AI workflows with Juglans.

Next steps: read the [How-to Guides](../guide/concepts.md) to dive deeper into specific topics, or check the [Reference](../reference/cli.md) for complete CLI and built-in tool documentation.
