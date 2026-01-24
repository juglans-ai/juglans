# 错误处理

本指南介绍 Juglans 工作流中的错误处理机制。

## 错误类型

| 类型 | 说明 | 示例 |
|------|------|------|
| 执行错误 | 工具执行失败 | API 超时、网络错误 |
| 验证错误 | 输入/输出不符合预期 | 缺少必填字段 |
| 逻辑错误 | 业务规则不满足 | 余额不足、权限不够 |
| 系统错误 | 运行时异常 | 内存不足、服务不可用 |

## on error 路径

### 基本语法

```yaml
[node] -> [success_path]
[node] on error -> [error_handler]
```

当 `[node]` 执行失败时，流程跳转到 `[error_handler]`。

### 简单示例

```yaml
name: "Error Handling Demo"

entry: [start]
exit: [done]

[start]: fetch_url(url=$input.api_url)
[start] -> [process]
[start] on error -> [handle_error]

[process]: chat(agent="processor", message=$output)
[handle_error]: notify(status="API call failed")

[process] -> [done]
[handle_error] -> [done]

[done]: notify(status="Complete")
```

## 错误信息访问

### $error 变量

在错误处理节点中，可通过 `$error` 访问错误信息：

```yaml
[api_call]: fetch_url(url=$input.url)
[api_call] on error -> [log_error]

[log_error]: notify(
  status="Error: " + $error.message + " (code: " + $error.code + ")"
)
```

### 错误对象结构

```yaml
$error = {
  "code": "NETWORK_ERROR",      # 错误代码
  "message": "Connection refused",  # 错误消息
  "node": "api_call",           # 发生错误的节点
  "details": { ... }            # 额外详情
}
```

## 常见错误处理模式

### 重试模式

```yaml
name: "Retry Pattern"

entry: [init]
exit: [success, give_up]

[init]: set_context(
  attempt=0,
  max_attempts=3,
  backoff_ms=1000
)

[try]: fetch_url(url=$input.url)
[try] -> [success]
[try] on error -> [check_retry]

[check_retry]: set_context(attempt=$ctx.attempt + 1)
[check_retry] if $ctx.attempt < $ctx.max_attempts -> [wait]
[check_retry] -> [give_up]

# 指数退避
[wait]: timer(ms=$ctx.backoff_ms * $ctx.attempt)
[wait] -> [try]

[success]: notify(status="Success after " + $ctx.attempt + " attempts")
[give_up]: notify(status="Failed after " + $ctx.max_attempts + " attempts")

[init] -> [try]
```

### 回退模式

主服务失败时使用备用服务：

```yaml
name: "Fallback Pattern"

entry: [primary]
exit: [done]

# 主服务
[primary]: fetch_url(url=$input.primary_api)
[primary] -> [process]
[primary] on error -> [fallback]

# 备用服务
[fallback]: fetch_url(url=$input.fallback_api)
[fallback] -> [process]
[fallback] on error -> [use_cache]

# 使用缓存
[use_cache]: set_context(data=$ctx.cached_data)
[use_cache] -> [process]

[process]: chat(agent="processor", message=json($output))
[process] -> [done]

[done]: notify(status="Complete")
```

### 熔断模式

连续失败后暂停调用：

```yaml
name: "Circuit Breaker"

entry: [check_circuit]
exit: [done]

[check_circuit]: set_context(
  circuit_open=$ctx.failures >= 5,
  now=timestamp()
)

# 熔断器打开，检查是否可以半开
[check_circuit] if $ctx.circuit_open -> [check_half_open]
[check_circuit] -> [call_api]

[check_half_open] if $ctx.now - $ctx.last_failure > 30000 -> [call_api]  # 30秒后尝试
[check_half_open] -> [circuit_open_response]

[circuit_open_response]: set_context(
  response={"error": "Service temporarily unavailable"}
)
[circuit_open_response] -> [done]

[call_api]: fetch_url(url=$input.api_url)
[call_api] -> [reset_failures]
[call_api] on error -> [increment_failures]

[reset_failures]: set_context(failures=0)
[reset_failures] -> [process]

[increment_failures]: set_context(
  failures=$ctx.failures + 1,
  last_failure=timestamp()
)
[increment_failures] -> [handle_error]

[process]: chat(agent="processor", message=$output)
[handle_error]: notify(status="API failed, failures: " + $ctx.failures)

[process] -> [done]
[handle_error] -> [done]

[done]: notify(status="Complete")
```

### 补偿模式

失败时撤销已完成的操作：

```yaml
name: "Compensation Pattern"

entry: [step1]
exit: [success, compensated]

# 步骤 1
[step1]: create_order(data=$input.order)
[step1] -> [step2]
[step1] on error -> [fail_early]

# 步骤 2
[step2]: reserve_inventory(order_id=$output.order_id)
[step2] -> [step3]
[step2] on error -> [compensate_step1]

# 步骤 3
[step3]: charge_payment(order_id=$ctx.order_id, amount=$ctx.amount)
[step3] -> [success]
[step3] on error -> [compensate_step2]

# 补偿操作
[compensate_step2]: release_inventory(order_id=$ctx.order_id)
[compensate_step2] -> [compensate_step1]

[compensate_step1]: cancel_order(order_id=$ctx.order_id)
[compensate_step1] -> [compensated]

[fail_early]: notify(status="Failed to create order")
[fail_early] -> [compensated]

[success]: notify(status="Order completed: " + $ctx.order_id)
[compensated]: notify(status="Transaction rolled back")
```

### 部分成功模式

批量操作中记录单项失败：

```yaml
name: "Partial Success"

entry: [init]
exit: [summary]

[init]: set_context(
  successes=[],
  failures=[]
)

[process_batch]: foreach($item in $input.items) {
  [process_item]: some_api_call(data=$item)
  [process_item] -> [record_success]
  [process_item] on error -> [record_failure]

  [record_success]: set_context(
    successes=append($ctx.successes, {
      "id": $item.id,
      "result": $output
    })
  )

  [record_failure]: set_context(
    failures=append($ctx.failures, {
      "id": $item.id,
      "error": $error.message
    })
  )
}

[summary]: notify(
  status="Completed: " + len($ctx.successes) + " success, " +
         len($ctx.failures) + " failed"
)

[init] -> [process_batch] -> [summary]
```

## 验证与守卫

### 输入验证

```yaml
name: "Input Validation"

entry: [validate]
exit: [result, error]

[validate]: chat(
  agent="validator",
  message="Validate: " + json($input),
  format="json"
)

[validate] if !$output.valid -> [reject]
[validate] -> [process]

[reject]: set_context(
  error={
    "code": "VALIDATION_ERROR",
    "message": $output.reason,
    "fields": $output.invalid_fields
  }
)
[reject] -> [error]

[process]: chat(agent="processor", message=$input.data)
[process] -> [result]

[result]: set_context(response=$output)
[error]: set_context(response=$ctx.error)
```

### 条件守卫

```yaml
[check_permission]: set_context(
  has_permission=$ctx.user.role == "admin" || $ctx.user.id == $input.owner_id
)

[check_permission] if !$ctx.has_permission -> [unauthorized]
[check_permission] -> [proceed]

[unauthorized]: set_context(
  error={"code": "FORBIDDEN", "message": "Permission denied"}
)
```

## 错误传播

### 显式传播

```yaml
[inner_call]: some_tool(...)
[inner_call] on error -> [propagate]

[propagate]: set_context(
  error={
    "code": "INNER_ERROR",
    "message": "Inner call failed: " + $error.message,
    "cause": $error
  }
)
[propagate] on error -> [outer_handler]  # 向上传播
```

### 错误聚合

```yaml
[collect_errors]: set_context(
  all_errors=concat($ctx.all_errors, [$error])
)

# 最后汇总
[report]: notify(
  status="Errors: " + json($ctx.all_errors)
)
```

## 日志与监控

### 错误日志

```yaml
[handle_error]: notify(
  status="[ERROR] " + $error.code + ": " + $error.message +
         " | Node: " + $error.node +
         " | Time: " + now()
)
```

### 告警通知

```yaml
[critical_error]: chat(
  agent="alerter",
  message="Critical error in workflow: " + json($error)
)

# 或发送到外部服务
[alert]: mcp_slack_send_message(
  channel="#alerts",
  text="Workflow error: " + $error.message
)
```

## 调试技巧

### 详细日志模式

```bash
juglans workflows/my-flow.jgflow --verbose
```

### 错误断点

```yaml
[debug_error]: notify(
  status="DEBUG: Error at " + $error.node + "\n" +
         "Input: " + json($ctx.last_input) + "\n" +
         "Error: " + json($error)
)
# 可以在这里暂停查看状态
```

### 模拟错误

测试错误处理逻辑：

```yaml
[simulate_error]: set_context(
  should_fail=$input.test_mode && $input.simulate_error
)

[maybe_fail] if $ctx.should_fail -> [force_error]
[maybe_fail] -> [real_call]

[force_error]: fail(message="Simulated error for testing")
```

## 最佳实践

### 1. 总是处理错误

```yaml
# 好：显式处理错误
[api] -> [success]
[api] on error -> [handle]

# 不好：忽略错误
[api] -> [next]
```

### 2. 提供有意义的错误信息

```yaml
# 好
[error]: set_context(error={
  "code": "PAYMENT_FAILED",
  "message": "Payment processing failed: insufficient funds",
  "details": {"balance": $ctx.balance, "required": $input.amount}
})

# 不好
[error]: set_context(error="Error")
```

### 3. 使用错误代码

```yaml
# 定义标准错误代码
# VALIDATION_ERROR - 输入验证失败
# AUTH_ERROR - 认证/授权失败
# NOT_FOUND - 资源不存在
# RATE_LIMITED - 请求过多
# INTERNAL_ERROR - 内部错误
```

### 4. 限制重试次数

```yaml
# 好：有限制
[retry] if $ctx.attempts < 3 -> [try_again]

# 不好：无限重试
[retry] -> [try_again]
```

### 5. 记录错误上下文

```yaml
[log_error]: set_context(
  error_log=append($ctx.error_log, {
    "timestamp": now(),
    "node": $error.node,
    "error": $error,
    "input": $ctx.current_input,
    "context_snapshot": {
      "user": $ctx.user,
      "session": $ctx.session_id
    }
  })
)
```
