# Tutorial 9: Full Project -- Building an AI Assistant

前 8 章涵盖了 Juglans 的所有核心特性。本章将它们组合起来，从零构建一个完整的 **Intent Router + QA AI 助手**。

## 9.1 项目需求

我们要构建一个 AI 助手，它能：

1. 接收用户输入
2. 分类意图（question / task / chat）
3. 根据意图路由到不同处理 agent
4. 格式化并输出结果

架构图：

```text
                         ┌─ [handle_qa] ──┐
[receive] → [classify] → switch ─ [handle_task] ─→ [format] → [output]
                         └─ [handle_chat] ┘
                  on error → [error_handler] ──→ [handle_qa]
```

## 9.2 项目结构

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

## 9.3 创建 Agent 文件

### classifier.jgagent -- 意图分类器

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

`temperature: 0.0` -- 分类任务需要确定性输出，不要随机性。

### qa.jgagent -- 问答专家

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

`temperature: 0.3` -- 技术问答偏向准确性，创造力调低。

### summarizer.jgagent -- 摘要生成器

```jgagent
slug: "summarizer"
name: "Summarizer"
model: "deepseek-chat"
temperature: 0.5
system_prompt: |
  You are a summarization assistant. Condense information into brief, clear summaries.
  Keep the key points. Remove redundancy.
```

## 9.4 创建 Prompt 文件

### classify.jgprompt -- 分类提示词模板

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

### format.jgprompt -- 输出格式化模板

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

## 9.5 编写主工作流

创建 `main.jg`：

```juglans
name: "AI Assistant"
version: "0.1.0"
description: "Intent router + QA assistant"

agents: ["./agents/*.jgagent"]
prompts: ["./prompts/*.jgprompt"]

entry: [receive]
exit: [output]

# Step 1: 接收输入，保存到上下文
[receive]: set_context(user_message=$input.message)

# Step 2: 用分类器 agent 判断意图，要求 JSON 输出
[classify]: chat(
  agent="classifier",
  message=$ctx.user_message,
  format="json"
)

# Step 3: 保存分类结果
[save_intent]: set_context(intent=$output.intent)

# Step 4: 处理节点 -- 三个分支
[handle_qa]: chat(agent="qa", message=$ctx.user_message)
[handle_task]: chat(agent="qa", message="Help the user complete this task: " + $ctx.user_message)
[handle_chat]: chat(agent="qa", message="Respond casually to: " + $ctx.user_message)

# Step 5: 兜底节点
[fallback]: print(message="Unknown intent, routing to QA")

# Step 6: 错误处理
[error_handler]: print(message="Classification failed: " + $error.message)

# Step 7: 格式化输出
[format]: set_context(response=$output)
[output]: print(message="[" + $ctx.intent + "] " + $ctx.response)

# --- 边定义 ---

# 主流程
[receive] -> [classify]
[classify] -> [save_intent]

# 错误处理：分类失败时走 QA
[classify] on error -> [error_handler]
[error_handler] -> [handle_qa]

# 意图路由
[save_intent] -> switch $ctx.intent {
    "question": [handle_qa]
    "task": [handle_task]
    "chat": [handle_chat]
    default: [fallback]
}

# 兜底也走 QA
[fallback] -> [handle_qa]

# 所有分支汇聚到格式化
[handle_qa] -> [format]
[handle_task] -> [format]
[handle_chat] -> [format]

# 格式化 → 输出
[format] -> [output]
```

逐段解释：

**元数据**（第 1-6 行）

- `agents:` 和 `prompts:` 加载资源文件，让 `chat()` 和 `p()` 能找到对应的 agent 和模板。
- `entry: [receive]` 和 `exit: [output]` 声明入口和出口。

**节点定义**

| 节点 | 工具 | 作用 |
|------|------|------|
| `[receive]` | `set_context()` | 将 `$input.message` 存入 `$ctx.user_message` |
| `[classify]` | `chat(format="json")` | 用 classifier agent 分类意图，返回 JSON |
| `[save_intent]` | `set_context()` | 将 `$output.intent` 存入 `$ctx.intent` |
| `[handle_qa]` | `chat()` | QA agent 回答问题 |
| `[handle_task]` | `chat()` | QA agent 处理任务 |
| `[handle_chat]` | `chat()` | QA agent 闲聊 |
| `[fallback]` | `print()` | 未知意图的兜底 |
| `[error_handler]` | `print()` | 分类出错时的处理 |
| `[format]` | `set_context()` | 保存响应到上下文 |
| `[output]` | `print()` | 最终输出 |

**边定义**

- `[classify] on error -> [error_handler]` -- 分类失败时不终止，走错误处理，然后转 QA。
- `switch $ctx.intent { ... }` -- 三路互斥路由，未匹配走 `default: [fallback]`。
- 所有分支最终汇聚到 `[format] -> [output]`。

## 9.6 运行和测试

### 语法检查

```bash
juglans check ai-assistant/
```

预期输出：

```text
Finished checking 1 workflow(s) - 1 valid
```

### 运行 -- 问题类型

```bash
juglans main.jg --input '{"message": "What is Rust programming language?"}'
```

预期输出（示例）：

```text
[question] Rust is a systems programming language focused on safety, speed, and concurrency...
```

### 运行 -- 任务类型

```bash
juglans main.jg --input '{"message": "Write a haiku about the ocean"}'
```

预期输出（示例）：

```text
[task] Waves crash on the shore / Salt air fills the twilight sky / Ocean never sleeps
```

### 运行 -- 闲聊类型

```bash
juglans main.jg --input '{"message": "Hey, how are you?"}'
```

预期输出（示例）：

```text
[chat] Hey! I'm doing well, thanks for asking. How can I help you today?
```

## 9.7 扩展思路

这个项目是一个起点。以下是几种增强方向：

**增加更多 Agent**

为每种意图创建专用 agent（而不是都用 qa agent），让每个 agent 的 system_prompt 针对性更强：

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

**用 Prompt 模板构造消息**

用 `p()` 渲染 prompt，再把结果传给 `chat()`：

```juglans
prompts: ["./prompts/*.jgprompt"]
agents: ["./agents/*.jgagent"]

[build_prompt]: p(slug="classify", message=$input.message)
[classify]: chat(agent="classifier", message=$output, format="json")
[show]: print(message=$output)

[build_prompt] -> [classify] -> [show]
```

**添加摘要步骤**

在输出前让 summarizer agent 精炼回答：

```juglans
agents: ["./agents/*.jgagent"]

[answer]: chat(agent="qa", message=$input.message)
[save]: set_context(raw_answer=$output)
[summarize]: chat(agent="summarizer", message="Summarize: " + $ctx.raw_answer)
[done]: print(message=$output)

[answer] -> [save] -> [summarize] -> [done]
```

**Web Server 模式**

用 `juglans web` 将 workflow 暴露为 HTTP API，前端通过 SSE 接收流式响应：

```bash
juglans web --port 8080
```

```bash
curl -X POST http://localhost:8080/api/chat \
  -H "Content-Type: application/json" \
  -d '{"message": "What is Rust?"}'
```

## 9.8 教程系列回顾

九章教程，每章一个核心主题：

| 章节 | 主题 | 核心概念 |
|------|------|----------|
| Tutorial 1 | Hello Workflow | 节点、边、`print()`、`notify()`、metadata |
| Tutorial 2 | Variables & Data Flow | `$input`、`$output`、`$ctx`、`set_context()`、`str()` |
| Tutorial 3 | Branching & Routing | `if` 条件边、`switch` 多路路由、分支汇聚 |
| Tutorial 4 | Loops | `foreach`、`while`、循环中累积 `$ctx` |
| Tutorial 5 | Error Handling | `on error`、`$error`、fallback 模式 |
| Tutorial 6 | AI Chat | `chat()`、`.jgagent`、`format="json"`、多轮对话 |
| Tutorial 7 | Prompt Templates | `.jgprompt`、`p()`、Jinja 模板、prompt 与 chat 组合 |
| Tutorial 8 | Workflow Composition | `flows:` 导入、命名空间节点、跨文件路由 |
| Tutorial 9 | Full Project | 综合运用：多 agent、prompt、switch、on error |

从第 1 章的 `print("Hello!")` 到第 9 章的多 agent 意图路由系统，你已经掌握了用 Juglans 构建 AI 工作流的完整工具链。

下一步：阅读 [How-to Guides](../guide/concepts.md) 深入了解特定主题，或查看 [Reference](../reference/cli.md) 获取完整的 CLI 和内置工具文档。
