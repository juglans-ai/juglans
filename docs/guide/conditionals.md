# 条件分支与路由

本指南详细介绍 Juglans 工作流中的条件分支和路由机制。

## 基本条件语法

```yaml
[node] if <expression> -> [target]
```

条件为真时，执行流程从 `[node]` 跳转到 `[target]`。

## 条件表达式

### 比较运算

```yaml
# 等于
[router] if $ctx.type == "admin" -> [admin_panel]

# 不等于
[check] if $output.status != "success" -> [error_handler]

# 数值比较
[score] if $output.score > 80 -> [high_tier]
[score] if $output.score >= 60 -> [mid_tier]
[score] if $output.score < 60 -> [low_tier]
```

### 字符串比较

```yaml
# 完全匹配
[lang] if $input.language == "zh" -> [chinese_handler]
[lang] if $input.language == "en" -> [english_handler]

# 包含检查（使用 chat 分类）
[classify]: chat(agent="classifier", format="json")
[classify] if $output.contains_code == true -> [code_handler]
```

### 布尔值

```yaml
# true/false
[validate] if $output.is_valid -> [proceed]
[validate] if !$output.is_valid -> [reject]

# 非空检查
[check] if $ctx.data -> [process]
[check] if !$ctx.data -> [fetch_data]
```

### 逻辑运算

```yaml
# AND
[check] if $ctx.logged_in && $ctx.is_admin -> [admin_route]

# OR
[check] if $ctx.is_vip || $ctx.is_admin -> [premium_route]

# NOT
[check] if !$ctx.banned -> [allow]

# 组合
[auth] if ($ctx.logged_in && $ctx.verified) || $ctx.is_admin -> [access]
```

## 多路分支

### 互斥分支

```yaml
name: "Multi-way Router"

entry: [classify]
exit: [done]

[classify]: chat(
  agent="intent-classifier",
  message=$input.query,
  format="json"
)

# 多个互斥条件
[classify] if $output.intent == "question" -> [qa_handler]
[classify] if $output.intent == "task" -> [task_handler]
[classify] if $output.intent == "chat" -> [chat_handler]
[classify] -> [fallback]  # 默认路径

[qa_handler]: chat(agent="qa-expert", message=$input.query)
[task_handler]: chat(agent="task-executor", message=$input.query)
[chat_handler]: chat(agent="conversational", message=$input.query)
[fallback]: chat(agent="general", message=$input.query)

[done]: notify(status="Complete")

[qa_handler] -> [done]
[task_handler] -> [done]
[chat_handler] -> [done]
[fallback] -> [done]
```

### 分支汇聚语义

**重要：** 当多个条件分支汇聚到同一个节点时，执行引擎使用 **OR 语义**，即：

- 只要**任意一个**前驱节点完成，汇聚节点就会执行
- **不会**等待所有前驱节点都完成（不是 AND 语义）

```yaml
# 分支后汇聚
[router] if $ctx.type == "A" -> [handler_a]
[router] if $ctx.type == "B" -> [handler_b]

[handler_a]: chat(agent="agent-a", message=$input)
[handler_b]: chat(agent="agent-b", message=$input)

[final]: notify(status="Done")

# final 节点有两个前驱，但只有一个会执行
# 执行引擎会自动检测未执行的分支，标记为不可达
[handler_a] -> [final]
[handler_b] -> [final]
```

这确保了条件分支的直觉行为：分支路径是互斥的，汇聚点不会等待未执行的分支。

### 优先级分支

条件按定义顺序评估，第一个为真的分支被执行：

```yaml
# 从高到低优先级
[score] if $output.score >= 90 -> [excellent]   # 先检查
[score] if $output.score >= 70 -> [good]        # 次检查
[score] if $output.score >= 60 -> [pass]        # 再检查
[score] -> [fail]                                # 默认
```

### 默认路径

无条件的边作为默认路径：

```yaml
[router] if $ctx.a -> [path_a]
[router] if $ctx.b -> [path_b]
[router] -> [default]  # 如果 a 和 b 都为 false
```

## 错误处理

### on error 路径

```yaml
[risky_call] -> [success]
[risky_call] on error -> [error_handler]

[error_handler]: notify(status="Error occurred, using fallback")
[fallback]: chat(agent="fallback", message=$input.query)

[error_handler] -> [fallback]
```

### 条件 + 错误处理

```yaml
[api_call]: fetch_url(url=$input.api_url)
[api_call] if $output.status == "ok" -> [process]
[api_call] if $output.status == "rate_limited" -> [wait_retry]
[api_call] on error -> [error_handler]
```

## 常见模式

### 意图路由器

```yaml
name: "Intent Router"

entry: [classify]
exit: [response]

[classify]: chat(
  agent="router",
  message=$input.message,
  format="json",
  system_prompt="Classify intent: question, command, feedback, other"
)

[question]: chat(agent="qa", message=$input.message)
[command]: chat(agent="executor", message=$input.message)
[feedback]: chat(agent="support", message=$input.message)
[other]: chat(agent="general", message=$input.message)

[classify] if $output.intent == "question" -> [question]
[classify] if $output.intent == "command" -> [command]
[classify] if $output.intent == "feedback" -> [feedback]
[classify] -> [other]

[response]: set_context(final_response=$output)

[question] -> [response]
[command] -> [response]
[feedback] -> [response]
[other] -> [response]
```

### 验证流程

```yaml
name: "Validation Pipeline"

entry: [validate_input]
exit: [success, failure]

[validate_input]: chat(
  agent="validator",
  message=$input.data,
  format="json"
)

[validate_input] if !$output.valid -> [reject]
[validate_input] if $output.needs_review -> [human_review]
[validate_input] -> [process]

[reject]: set_context(error=$output.reason)
[human_review]: notify(status="Needs human review")
[process]: chat(agent="processor", message=$input.data)

[success]: notify(status="Validation passed")
[failure]: notify(status="Validation failed: " + $ctx.error)

[reject] -> [failure]
[human_review] -> [success]  # 假设人工审核通过
[process] -> [success]
```

### 重试逻辑

```yaml
name: "Retry Pattern"

entry: [init]
exit: [success, give_up]

[init]: set_context(attempt=0, max_attempts=3)

[try]: fetch_url(url=$input.url)
[try] if $output.ok -> [success]
[try] on error -> [check_retry]

[check_retry]: set_context(attempt=$ctx.attempt + 1)
[check_retry] if $ctx.attempt < $ctx.max_attempts -> [wait]
[check_retry] -> [give_up]

[wait]: timer(ms=1000)
[wait] -> [try]

[success]: notify(status="Success!")
[give_up]: notify(status="Failed after " + $ctx.max_attempts + " attempts")

[init] -> [try]
```

### 质量检查

```yaml
name: "Quality Check"

entry: [generate]
exit: [output]

[generate]: chat(agent="writer", message=$input.topic)

[check_quality]: chat(
  agent="reviewer",
  message="Review this content:\n" + $output,
  format="json"
)

[check_quality] if $output.score < 7 -> [regenerate]
[check_quality] -> [output]

[regenerate]: chat(
  agent="writer",
  message="Improve this: " + $ctx.content + "\nFeedback: " + $output.feedback
)
[regenerate] -> [check_quality]

[output]: set_context(final=$output)

[generate] -> [check_quality]
```

### A/B 测试

```yaml
name: "A/B Test"

entry: [assign_group]
exit: [collect]

[assign_group]: set_context(
  group=if(random() > 0.5, "A", "B")
)

[assign_group] if $ctx.group == "A" -> [variant_a]
[assign_group] if $ctx.group == "B" -> [variant_b]

[variant_a]: chat(agent="model-a", message=$input.query)
[variant_b]: chat(agent="model-b", message=$input.query)

[collect]: set_context(
  result=$output,
  variant=$ctx.group
)

[variant_a] -> [collect]
[variant_b] -> [collect]
```

## 调试条件

### 打印条件值

```yaml
[debug]: notify(status="Type: " + $ctx.type + ", Score: " + $ctx.score)
[debug] -> [router]

[router] if $ctx.type == "a" -> [path_a]
...
```

### 记录路由决策

```yaml
[router]: chat(agent="classifier", format="json")
[log_decision]: set_context(
  route_log=append($ctx.route_log, {
    "input": $input.query,
    "decision": $output,
    "timestamp": now()
  })
)

[router] -> [log_decision]
[log_decision] if $output.intent == "a" -> [path_a]
...
```

## 最佳实践

1. **明确默认路径** - 始终提供一个无条件的默认分支
2. **互斥条件** - 确保分支条件互不重叠
3. **优先级清晰** - 把更具体的条件放在前面
4. **错误处理** - 为可能失败的节点添加 `on error`
5. **日志记录** - 在复杂路由中记录决策过程
