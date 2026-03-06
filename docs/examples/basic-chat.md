# Basic Chat Workflow

The simplest chat workflow example.

## Workflow File

### chat.jg

```juglans
name: "Basic Chat"
description: "Simple chat workflow"

entry: [chat]
exit: [respond]

[chat]: chat(
  agent="assistant",
  message=$input.message
)

[respond]: set_context(response=$output)

[chat] -> [respond]
```

### src/agents/assistant.jgagent

```jgagent
name: "assistant"
description: "A helpful assistant"

model: "claude-3-sonnet"
temperature: 0.7
max_tokens: 1024

system_prompt: |
  You are a helpful AI assistant. Be concise and friendly.
```

## Running

```bash
# Basic run
juglans chat.jg --input '{"message": "Hello!"}'

# Output
# > Hello! How can I help you today?

# A more complex question
juglans chat.jg --input '{"message": "Explain quantum computing in simple terms"}'
```

## Variant: With System Prompt

### chat-with-persona.jg

```juglans
name: "Chat with Persona"

entry: [chat]
exit: [respond]

[chat]: chat(
  agent="assistant",
  message=$input.message,
  system_prompt=$input.persona
)

[respond]: set_context(response=$output)

[chat] -> [respond]
```

```bash
juglans chat-with-persona.jg --input '{
  "message": "Tell me a joke",
  "persona": "You are a pirate. Always speak like a pirate."
}'

# > Arrr, matey! Why did the pirate go to school?
# > To improve his arrrticulation! Har har har!
```

## Variant: With Formatted Output

### chat-json.jg

```juglans
name: "Chat with JSON Output"

entry: [analyze]
exit: [result]

[analyze]: chat(
  agent="analyzer",
  message=$input.text,
  format="json"
)

[result]: set_context(
  analysis=$output
)

[analyze] -> [result]
```

### src/agents/analyzer.jgagent

```jgagent
name: "analyzer"
model: "claude-3-sonnet"

system_prompt: |
  Analyze the given text and return a JSON object with:
  - sentiment: positive, negative, or neutral
  - topics: array of main topics
  - summary: one sentence summary
```

```bash
juglans chat-json.jg --input '{
  "text": "I love this product! The quality is amazing and shipping was fast."
}'

# Output:
# {
#   "sentiment": "positive",
#   "topics": ["product quality", "shipping"],
#   "summary": "Customer expresses satisfaction with product quality and delivery speed."
# }
```

## Variant: Multi-turn Chat

### multi-turn.jg

```juglans
name: "Multi-turn Chat"

entry: [load_history]
exit: [save_and_respond]

# Load conversation history
[load_history]: set_context(
  history=$input.history || []
)

# Build messages (including history)
[build_messages]: set_context(
  full_context=concat($ctx.history, [{"role": "user", "content": $input.message}])
)

# Call AI
[chat]: chat(
  agent="assistant",
  messages=$ctx.full_context
)

# Save new message to history
[save_and_respond]: set_context(
  response=$output,
  updated_history=concat($ctx.full_context, [{"role": "assistant", "content": $output}])
)

[load_history] -> [build_messages] -> [chat] -> [save_and_respond]
```

```bash
# First turn
juglans multi-turn.jg --input '{
  "message": "My name is Alice",
  "history": []
}'

# Second turn (with history)
juglans multi-turn.jg --input '{
  "message": "What is my name?",
  "history": [
    {"role": "user", "content": "My name is Alice"},
    {"role": "assistant", "content": "Nice to meet you, Alice!"}
  ]
}'

# > Your name is Alice!
```

## Directory Structure

```
basic-chat/
├── chat.jg
├── chat-with-persona.jg
├── chat-json.jg
├── multi-turn.jg
└── agents/
    ├── assistant.jgagent
    └── analyzer.jgagent
```
