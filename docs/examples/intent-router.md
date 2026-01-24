# 意图分类路由

根据用户输入的意图，路由到不同的处理流程。

## 工作流文件

### intent-router.jgflow

```yaml
name: "Intent Router"
description: "Classify user intent and route to appropriate handler"

prompts: ["./prompts/*.jgprompt"]
agents: ["./agents/*.jgagent"]

entry: [classify]
exit: [respond]

# 意图分类
[classify]: chat(
  agent="classifier",
  message=$input.message,
  format="json"
)

# 路由到不同处理器
[classify] if $output.intent == "question" -> [handle_question]
[classify] if $output.intent == "task" -> [handle_task]
[classify] if $output.intent == "greeting" -> [handle_greeting]
[classify] if $output.intent == "feedback" -> [handle_feedback]
[classify] -> [handle_general]

# 问题处理
[handle_question]: chat(
  agent="qa-expert",
  message=$input.message
)

# 任务处理
[handle_task]: chat(
  agent="task-executor",
  message=$input.message,
  format="json"
)

# 问候处理
[handle_greeting]: p(
  slug="greeting-response",
  name=$output.detected_name || "friend"
)

# 反馈处理
[handle_feedback]: chat(
  agent="support",
  message=$input.message
)

# 通用处理
[handle_general]: chat(
  agent="assistant",
  message=$input.message
)

# 汇总响应
[respond]: set_context(
  response=$output,
  intent=$ctx.classified_intent
)

[handle_question] -> [respond]
[handle_task] -> [respond]
[handle_greeting] -> [respond]
[handle_feedback] -> [respond]
[handle_general] -> [respond]
```

## Agent 定义

### agents/classifier.jgagent

```yaml
name: "classifier"
description: "Intent classification agent"

model: "claude-3-haiku"
temperature: 0
max_tokens: 256

system_prompt: |
  You are an intent classifier. Analyze the user message and return JSON:

  {
    "intent": "question" | "task" | "greeting" | "feedback" | "general",
    "confidence": 0.0-1.0,
    "detected_name": "name if mentioned, null otherwise",
    "reasoning": "brief explanation"
  }

  Intent definitions:
  - question: User is asking for information or explanation
  - task: User wants something done (create, edit, send, etc.)
  - greeting: User is saying hello, goodbye, or making small talk
  - feedback: User is providing feedback, complaints, or suggestions
  - general: Anything else
```

### agents/qa-expert.jgagent

```yaml
name: "qa-expert"
description: "Question answering specialist"

model: "claude-3-sonnet"
temperature: 0.3
max_tokens: 2048

system_prompt: |
  You are a knowledgeable expert. Answer questions accurately and thoroughly.
  If you don't know something, say so honestly.
  Provide sources or reasoning when appropriate.
```

### agents/task-executor.jgagent

```yaml
name: "task-executor"
description: "Task execution agent"

model: "claude-3-sonnet"
temperature: 0.2
max_tokens: 1024

system_prompt: |
  You are a task execution assistant. When given a task:
  1. Understand what needs to be done
  2. Break it into steps if complex
  3. Execute or provide clear instructions
  4. Return structured JSON with status and results

  Output format:
  {
    "status": "completed" | "needs_input" | "cannot_complete",
    "result": "task result or output",
    "steps_taken": ["step1", "step2"],
    "next_steps": ["if any"]
  }
```

### agents/support.jgagent

```yaml
name: "support"
description: "Customer support agent"

model: "claude-3-sonnet"
temperature: 0.5
max_tokens: 1024

system_prompt: |
  You are a friendly customer support representative.
  - Acknowledge the user's feedback
  - Show empathy if they're frustrated
  - Provide helpful solutions or next steps
  - Thank them for their input
```

## Prompt 模板

### prompts/greeting-response.jgprompt

```yaml
name: "greeting-response"

template: |
  Hello {{ name }}! 👋

  I'm your AI assistant. I can help you with:
  - Answering questions
  - Completing tasks
  - General conversation

  What would you like to do today?
```

## 运行示例

```bash
# 问题
juglans intent-router.jgflow --input '{"message": "What is the capital of France?"}'
# Intent: question -> qa-expert
# > Paris is the capital of France...

# 任务
juglans intent-router.jgflow --input '{"message": "Create a summary of this article..."}'
# Intent: task -> task-executor
# > {"status": "completed", "result": "..."}

# 问候
juglans intent-router.jgflow --input '{"message": "Hi, I am Bob"}'
# Intent: greeting -> greeting template
# > Hello Bob! 👋 I am your AI assistant...

# 反馈
juglans intent-router.jgflow --input '{"message": "The app keeps crashing when I try to save"}'
# Intent: feedback -> support
# > I am sorry to hear about the crashes...
```

## 高级：多级路由

### advanced-router.jgflow

```yaml
name: "Advanced Multi-level Router"

entry: [classify_primary]
exit: [respond]

# 一级分类
[classify_primary]: chat(
  agent="classifier",
  message=$input.message,
  format="json"
)

[classify_primary] if $output.intent == "question" -> [classify_question_type]
[classify_primary] if $output.intent == "task" -> [classify_task_type]
[classify_primary] -> [handle_general]

# 二级分类：问题类型
[classify_question_type]: chat(
  agent="question-classifier",
  message=$input.message,
  format="json"
)

[classify_question_type] if $output.type == "factual" -> [factual_qa]
[classify_question_type] if $output.type == "opinion" -> [opinion_qa]
[classify_question_type] if $output.type == "how_to" -> [howto_qa]
[classify_question_type] -> [general_qa]

# 二级分类：任务类型
[classify_task_type]: chat(
  agent="task-classifier",
  message=$input.message,
  format="json"
)

[classify_task_type] if $output.type == "create" -> [create_handler]
[classify_task_type] if $output.type == "edit" -> [edit_handler]
[classify_task_type] if $output.type == "search" -> [search_handler]
[classify_task_type] -> [general_task]

# 具体处理器...
[factual_qa]: chat(agent="fact-checker", message=$input.message)
[opinion_qa]: chat(agent="opinion-responder", message=$input.message)
[howto_qa]: chat(agent="tutorial-writer", message=$input.message)
[general_qa]: chat(agent="qa-expert", message=$input.message)

[create_handler]: chat(agent="creator", message=$input.message)
[edit_handler]: chat(agent="editor", message=$input.message)
[search_handler]: chat(agent="searcher", message=$input.message)
[general_task]: chat(agent="task-executor", message=$input.message)

[handle_general]: chat(agent="assistant", message=$input.message)

# 汇总
[respond]: set_context(response=$output)

[factual_qa] -> [respond]
[opinion_qa] -> [respond]
[howto_qa] -> [respond]
[general_qa] -> [respond]
[create_handler] -> [respond]
[edit_handler] -> [respond]
[search_handler] -> [respond]
[general_task] -> [respond]
[handle_general] -> [respond]
```

## 目录结构

```
intent-router/
├── intent-router.jgflow
├── advanced-router.jgflow
├── agents/
│   ├── classifier.jgagent
│   ├── qa-expert.jgagent
│   ├── task-executor.jgagent
│   ├── support.jgagent
│   └── assistant.jgagent
├── prompts/
│   └── greeting-response.jgprompt
└── test-inputs/
    ├── question.json
    ├── task.json
    └── greeting.json
```
