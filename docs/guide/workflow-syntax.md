# 工作流语法 (.jgflow)

`.jgflow` 文件定义了工作流的结构和执行逻辑。

## 文件结构

```yaml
# 元数据
name: "Workflow Name"
version: "0.1.0"
author: "Author Name"
description: "Workflow description"

# 资源导入
prompts: ["./prompts/*.jgprompt"]
agents: ["./agents/*.jgagent"]

# 入口和出口
entry: [start_node]
exit: [end_node]

# 节点定义
[node_id]: tool_call(params...)

# 边定义
[A] -> [B]
```

## 元数据

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `name` | string | 是 | 工作流名称 |
| `version` | string | 否 | 版本号 |
| `author` | string | 否 | 作者 |
| `description` | string | 否 | 描述 |

## 资源导入

工作流可以导入本地的 Prompt 和 Agent 文件，也可以引用远程资源。

### 本地资源导入

使用 glob 模式导入本地文件：

```yaml
# 相对路径（相对于 .jgflow 文件所在目录）
prompts: ["./prompts/*.jgprompt"]
agents: ["./agents/*.jgagent"]

# 多个路径
prompts: [
  "./local/*.jgprompt",
  "./shared/*.jgprompt"
]

# 单个文件
agents: ["./agents/main-agent.jgagent"]

# 绝对路径
prompts: ["/absolute/path/to/prompts/*.jgprompt"]
```

**路径解析规则：**
- 相对路径：相对于 `.jgflow` 文件所在目录
- 绝对路径：以 `/` 开头的路径
- Glob 通配符：`*` 匹配文件名，`**` 匹配子目录

### 本地 vs 远程资源

导入的本地资源可以通过 slug 引用：

```yaml
# 导入本地 Agent
agents: ["./agents/*.jgagent"]

# 引用本地 Agent（通过 slug）
[chat]: chat(agent="my-local-agent", message=$input)
```

如果需要引用远程（Jug0）资源，使用 `owner/slug` 格式：

```yaml
# 无需导入，直接引用远程资源
[chat]: chat(agent="juglans/premium-agent", message=$input)
[render]: p(slug="owner/shared-prompt", data=$input)
```

### 混合使用

```yaml
# 导入本地资源
prompts: ["./prompts/*.jgprompt"]
agents: ["./agents/*.jgagent"]

entry: [start]
exit: [end]

# 使用本地 Agent
[local_chat]: chat(agent="my-agent", message=$input)

# 使用远程 Agent
[remote_chat]: chat(agent="juglans/cloud-agent", message=$output)

[start] -> [local_chat] -> [remote_chat] -> [end]
```

## 入口和出口

定义工作流的起点和终点：

```yaml
entry: [start]           # 单一入口
exit: [end]              # 单一出口

# 多出口（用于分支结果）
exit: [success, failure]
```

## 节点定义

### 基本语法

```yaml
[node_id]: tool_name(param1=value1, param2=value2)
```

### 节点 ID 规则

- 使用字母、数字、下划线、连字符
- 必须用方括号包围
- 区分大小写

```yaml
[start]              # 有效
[process_data]       # 有效
[step-1]             # 有效
[MyNode]             # 有效
```

### 工具调用

```yaml
# 字符串参数
[node]: notify(status="Processing...")

# 变量引用
[node]: chat(message=$input.text)

# 嵌套对象
[node]: chat(
  agent="assistant",
  message=$input.question,
  format="json"
)

# 数组参数
[node]: some_tool(items=["a", "b", "c"])
```

### 字面量节点

```yaml
# 字符串字面量
[message]: "Hello, World!"

# JSON 字面量
[config]: {
  "model": "gpt-4",
  "temperature": 0.7
}
```

## 边定义

### 简单连接

```yaml
[A] -> [B]              # A 完成后执行 B
[A] -> [B] -> [C]       # 链式连接
```

### 条件分支

```yaml
# 基于表达式的条件
[router] if $ctx.type == "simple" -> [simple_handler]
[router] if $ctx.type == "complex" -> [complex_handler]

# 比较运算符
[node] if $output.score > 0.8 -> [high_score]
[node] if $output.score <= 0.8 -> [low_score]

# 布尔值
[check] if $ctx.is_valid -> [proceed]
[check] if !$ctx.is_valid -> [reject]
```

### 错误处理

```yaml
# 错误时跳转
[risky_operation] on error -> [error_handler]

# 组合使用
[api_call] -> [process]
[api_call] on error -> [fallback]
```

### 默认路径

```yaml
# 条件都不满足时的默认路径
[router] if $ctx.a == 1 -> [path_a]
[router] if $ctx.b == 1 -> [path_b]
[router] -> [default_path]          # 默认
```

## 循环结构

### While 循环

```yaml
[loop]: while($ctx.count < 10) {
  [increment]: set_context(count=$ctx.count + 1)
  [process]: chat(agent="worker", message="Item " + $ctx.count)
  [increment] -> [process]
}
```

### Foreach 循环

```yaml
[process_items]: foreach($item in $input.items) {
  [handle]: chat(agent="processor", message=$item.content)
  [save]: set_context(results=append($ctx.results, $output))
  [handle] -> [save]
}
```

### 循环上下文变量

在循环内部可用：

| 变量 | 说明 |
|------|------|
| `loop.index` | 当前索引 (0-based) |
| `loop.first` | 是否第一次迭代 |
| `loop.last` | 是否最后一次迭代 |

## 变量引用

### 路径语法

```yaml
$input.field           # 输入变量
$output                # 当前节点输出
$output.nested.field   # 嵌套访问
$ctx.variable          # 上下文变量
$reply.content         # 最后回复内容
```

### 在工具调用中使用

```yaml
[step1]: chat(message=$input.question)
[step2]: p(slug="template", data=$output)
[step3]: notify(status="Result: " + $output.summary)
```

## 完整示例

### 简单对话

```yaml
name: "Simple Chat"
version: "0.1.0"

agents: ["./agents/*.jgagent"]

entry: [start]
exit: [end]

[start]: notify(status="Chat started")
[chat]: chat(agent="assistant", message=$input.message)
[end]: notify(status="Chat ended")

[start] -> [chat] -> [end]
```

### 带路由的工作流

```yaml
name: "Smart Router"
version: "0.1.0"

prompts: ["./prompts/*.jgprompt"]
agents: ["./agents/*.jgagent"]

entry: [classify]
exit: [done]

# 分类节点
[classify]: chat(
  agent="classifier",
  message=$input.query,
  format="json"
)

# 处理分支
[technical]: chat(agent="tech-expert", message=$input.query)
[general]: chat(agent="general-assistant", message=$input.query)
[creative]: chat(agent="creative-writer", message=$input.query)

# 完成节点
[done]: notify(status="Query processed")

# 路由逻辑
[classify] if $output.category == "technical" -> [technical]
[classify] if $output.category == "creative" -> [creative]
[classify] -> [general]

[technical] -> [done]
[general] -> [done]
[creative] -> [done]
```

### 批量处理

```yaml
name: "Batch Processor"
version: "0.1.0"

agents: ["./agents/*.jgagent"]

entry: [init]
exit: [summary]

[init]: set_context(results=[])

[process]: foreach($item in $input.items) {
  [analyze]: chat(
    agent="analyzer",
    message=$item.content
  )
  [collect]: set_context(
    results=append($ctx.results, {
      "id": $item.id,
      "result": $output
    })
  )
  [analyze] -> [collect]
}

[summary]: chat(
  agent="summarizer",
  message="Summarize these results: " + json($ctx.results)
)

[init] -> [process] -> [summary]
```

### 带错误处理

```yaml
name: "Robust Workflow"
version: "0.1.0"

entry: [start]
exit: [success, failure]

[start]: notify(status="Starting...")

[risky_call]: chat(
  agent="external-api",
  message=$input.data
)

[process]: p(slug="process-result", data=$output)

[success]: notify(status="Completed successfully")
[failure]: notify(status="Failed, using fallback")

[start] -> [risky_call] -> [process] -> [success]
[risky_call] on error -> [failure]
```

## 最佳实践

1. **命名清晰** - 使用描述性的节点 ID
2. **模块化** - 将复杂逻辑拆分为多个工作流
3. **错误处理** - 为关键节点添加 `on error` 路径
4. **注释** - 使用 `#` 添加注释说明
5. **版本控制** - 使用 `version` 字段跟踪变更
