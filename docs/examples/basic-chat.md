# 基础对话工作流

最简单的对话工作流示例。

## 工作流文件

### chat.jgflow

```yaml
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

### agents/assistant.jgagent

```yaml
name: "assistant"
description: "A helpful assistant"

model: "claude-3-sonnet"
temperature: 0.7
max_tokens: 1024

system_prompt: |
  You are a helpful AI assistant. Be concise and friendly.
```

## 运行

```bash
# 基本运行
juglans chat.jgflow --input '{"message": "Hello!"}'

# 输出
# > Hello! How can I help you today?

# 更复杂的问题
juglans chat.jgflow --input '{"message": "Explain quantum computing in simple terms"}'
```

## 变体：带系统提示

### chat-with-persona.jgflow

```yaml
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
juglans chat-with-persona.jgflow --input '{
  "message": "Tell me a joke",
  "persona": "You are a pirate. Always speak like a pirate."
}'

# > Arrr, matey! Why did the pirate go to school?
# > To improve his arrrticulation! Har har har!
```

## 变体：带格式化输出

### chat-json.jgflow

```yaml
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

### agents/analyzer.jgagent

```yaml
name: "analyzer"
model: "claude-3-sonnet"

system_prompt: |
  Analyze the given text and return a JSON object with:
  - sentiment: positive, negative, or neutral
  - topics: array of main topics
  - summary: one sentence summary
```

```bash
juglans chat-json.jgflow --input '{
  "text": "I love this product! The quality is amazing and shipping was fast."
}'

# Output:
# {
#   "sentiment": "positive",
#   "topics": ["product quality", "shipping"],
#   "summary": "Customer expresses satisfaction with product quality and delivery speed."
# }
```

## 变体：多轮对话

### multi-turn.jgflow

```yaml
name: "Multi-turn Chat"

entry: [load_history]
exit: [save_and_respond]

# 加载对话历史
[load_history]: set_context(
  history=$input.history || []
)

# 构建消息（包含历史）
[build_messages]: set_context(
  full_context=concat($ctx.history, [{"role": "user", "content": $input.message}])
)

# 调用 AI
[chat]: chat(
  agent="assistant",
  messages=$ctx.full_context
)

# 保存新消息到历史
[save_and_respond]: set_context(
  response=$output,
  updated_history=concat($ctx.full_context, [{"role": "assistant", "content": $output}])
)

[load_history] -> [build_messages] -> [chat] -> [save_and_respond]
```

```bash
# 第一轮
juglans multi-turn.jgflow --input '{
  "message": "My name is Alice",
  "history": []
}'

# 第二轮（带历史）
juglans multi-turn.jgflow --input '{
  "message": "What is my name?",
  "history": [
    {"role": "user", "content": "My name is Alice"},
    {"role": "assistant", "content": "Nice to meet you, Alice!"}
  ]
}'

# > Your name is Alice!
```

## 目录结构

```
basic-chat/
├── chat.jgflow
├── chat-with-persona.jgflow
├── chat-json.jgflow
├── multi-turn.jgflow
└── agents/
    ├── assistant.jgagent
    └── analyzer.jgagent
```
