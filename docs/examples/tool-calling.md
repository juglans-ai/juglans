# Tool Calling（工具调用）

展示如何在 Agent 配置和工作流中使用工具（Function Calling）。

## 概述

Juglans 支持两种方式配置工具：

1. **Agent 级别** - 在 `.jgagent` 文件中配置默认工具
2. **工作流级别** - 在 `chat()` 调用中动态指定工具

工作流级别的配置会覆盖 Agent 的默认配置。

## 示例：带工具的复杂问题求解器

### 工作流文件

#### tool-router.jgflow

```yaml
name: "AI Router with Tooling"
description: "Route simple vs complex questions, use tools for complex ones"

prompts: ["./prompts/*.jgprompt"]
agents: ["./agents/*.jgagent"]

entry: [init]
exit: [final_notify]

[init]: notify(status="🔍 正在分析您的提问...")

# 第一步：复杂度分析（无状态，不污染对话历史）
[classify]: chat(
  agent="classifier",
  format="json",
  stateless="true",
  message=p(slug="router", user_msg=$input.message)
)

# 简单问题直接回答
[simple_reply]: chat(
  agent="assistant",
  chat_id=$reply.chat_id,
  message="用户刚才问了: '$input.message'。请根据上下文简洁回答。"
)

# 复杂问题提示
[complex_thinking]: notify(status="🧠 复杂问题，启动工具...")

# 复杂问题求解（使用工具）
[complex_solver]: chat(
  agent="tool-agent",
  chat_id=$reply.chat_id,
  message=p(slug="solver", user_msg=$input.message),
  tools=[
    {
      "type": "function",
      "function": {
        "name": "fetch_url",
        "description": "获取网页的源代码或文本内容",
        "parameters": {
          "type": "object",
          "properties": {
            "url": {"type": "string", "description": "完整的网页 URL"},
            "method": {"type": "string", "enum": ["GET", "POST"]}
          },
          "required": ["url"]
        }
      }
    },
    {
      "type": "function",
      "function": {
        "name": "calculate",
        "description": "执行数学计算",
        "parameters": {
          "type": "object",
          "properties": {
            "expression": {"type": "string", "description": "数学表达式，如 '2 + 2'"}
          },
          "required": ["expression"]
        }
      }
    }
  ]
)

[final_notify]: notify(status="✅ 处理完毕")

# 路由逻辑
[init] -> [classify]

[classify] if $ctx.classify.output.complexity == "simple" -> [simple_reply]
[classify] if $ctx.classify.output.complexity == "complex" -> [complex_thinking]

[complex_thinking] -> [complex_solver]

[simple_reply] -> [final_notify]
[complex_solver] -> [final_notify]
```

## Agent 定义

### agents/classifier.jgagent

```yaml
slug: "classifier"
name: "Intent Classifier"
model: "gpt-3.5-turbo"
temperature: 0.0

system_prompt: |
  You are a question complexity classifier.

  Analyze the user's question and classify it as "simple" or "complex".

  - simple: General knowledge, greetings, basic questions
  - complex: Requires web search, calculations, or multi-step reasoning

  Respond with JSON:
  {"complexity": "simple" | "complex", "reason": "brief explanation"}
```

### agents/tool-agent.jgagent（带默认工具）

```yaml
slug: "tool-agent"
name: "Tool-enabled Agent"
model: "gpt-4o"
temperature: 0.7

system_prompt: |
  You are a helpful assistant with access to tools.

  When you need information from the web, use fetch_url.
  When you need to perform calculations, use calculate.

  Always explain your reasoning and the results from tool calls.

# 默认工具配置（可被工作流覆盖）
tools: [
  {
    "type": "function",
    "function": {
      "name": "search_knowledge",
      "description": "搜索知识库",
      "parameters": {
        "type": "object",
        "properties": {
          "query": {"type": "string", "description": "搜索关键词"}
        },
        "required": ["query"]
      }
    }
  }
]
```

### agents/assistant.jgagent

```yaml
slug: "assistant"
name: "General Assistant"
model: "gpt-3.5-turbo"
temperature: 0.7

system_prompt: |
  You are a helpful, friendly AI assistant.
  Answer questions clearly and concisely.
```

## Prompt 模板

### prompts/router.jgprompt

```yaml
slug: "router"
name: "Complexity Router Prompt"

template: |
  User question: {{ user_msg }}

  Classify the complexity of this question.
```

### prompts/solver.jgprompt

```yaml
slug: "solver"
name: "Complex Problem Solver Prompt"

template: |
  User asked a complex question: {{ user_msg }}

  Please analyze and solve this problem step by step.
  Use available tools when needed.
```

## 运行示例

### 简单问题

```bash
juglans tool-router.jgflow --input '{"message": "你是谁？"}'
```

输出：
```
🔍 正在分析您的提问...
[classify] complexity: simple
用户刚才问了: '你是谁？'。我是一个AI助手...
✅ 处理完毕
```

### 复杂问题（需要工具）

```bash
juglans tool-router.jgflow --input '{"message": "帮我查一下 juglans.ai 的最新更新"}'
```

输出：
```
🔍 正在分析您的提问...
[classify] complexity: complex
🧠 复杂问题，启动工具...
[tool-agent] Calling fetch_url(url="https://juglans.ai")
[tool-agent] 根据网站内容，最新更新包括...
✅ 处理完毕
```

## 工具配置优先级

### 场景 1：使用 Agent 默认工具

```yaml
# Agent 配置了默认工具
[step]: chat(
  agent="tool-agent",
  message=$input
  # 未指定 tools，使用 Agent 的默认工具
)
```

### 场景 2：工作流覆盖工具

```yaml
# 工作流指定的工具覆盖 Agent 默认配置
[step]: chat(
  agent="tool-agent",
  message=$input,
  tools=[
    {
      "type": "function",
      "function": {
        "name": "custom_tool",
        "description": "自定义工具"
      }
    }
  ]
  # 这里的 tools 会替代 Agent 的默认配置
)
```

### 场景 3：无工具调用

```yaml
# Agent 没有默认工具，工作流也没指定
[step]: chat(
  agent="assistant",
  message=$input
  # 纯文本对话，无工具调用
)
```

## 工具定义格式

工具定义遵循 OpenAI Function Calling 格式：

```json
{
  "type": "function",
  "function": {
    "name": "tool_name",
    "description": "清晰描述工具的功能",
    "parameters": {
      "type": "object",
      "properties": {
        "param1": {
          "type": "string",
          "description": "参数说明"
        },
        "param2": {
          "type": "number",
          "enum": [1, 2, 3],
          "description": "枚举值参数"
        }
      },
      "required": ["param1"]
    }
  }
}
```

## 最佳实践

1. **Agent 默认工具** - 为特定领域的 Agent 配置常用工具
2. **工作流覆盖** - 针对特定任务动态调整可用工具
3. **工具描述** - 写清晰的工具描述，帮助模型理解何时使用
4. **参数验证** - 使用 `required` 和类型定义确保参数正确
5. **无状态分类** - 用 `stateless="true"` 避免分类器污染对话历史

## 目录结构

```
tool-calling/
├── tool-router.jgflow
├── agents/
│   ├── classifier.jgagent
│   ├── tool-agent.jgagent
│   └── assistant.jgagent
└── prompts/
    ├── router.jgprompt
    └── solver.jgprompt
```

## 调试工具调用

启用详细日志查看工具调用过程：

```bash
DEBUG=true juglans tool-router.jgflow --input '{"message": "问题"}'
```

输出会包含：
- 工具调用请求
- 工具执行结果
- 模型响应
