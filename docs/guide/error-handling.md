# Error Handling

This guide introduces the error handling mechanisms in Juglans workflows.

## Error Types

| Type | Description | Example |
|------|------|------|
| Execution error | Tool execution failure | API timeout, network error |
| Validation error | Input/output does not match expectations | Missing required field |
| Logic error | Business rule not satisfied | Insufficient balance, insufficient permissions |
| System error | Runtime exception | Out of memory, service unavailable |

## on error Path

### Basic Syntax

```juglans
[node]: print(msg="do work")
[success_path]: print(msg="ok")
[error_handler]: print(msg="error")

[node] -> [success_path]
[node] on error -> [error_handler]
```

When `[node]` execution fails, the flow jumps to `[error_handler]`.

### Simple Example

```juglans
name: "Error Handling Demo"

entry: [start]
exit: [done]

[start]: fetch_url(url=$input.api_url)
[process]: chat(agent="processor", message=$output)
[handle_error]: notify(status="API call failed")
[done]: notify(status="Complete")

[start] -> [process]
[start] on error -> [handle_error]
[process] -> [done]
[handle_error] -> [done]
```

## Accessing Error Information

### The $error Variable

In error handling nodes, you can access error information via `$error`:

```juglans
[api_call]: fetch_url(url=$input.url)
[log_error]: notify(
  status="Error: " + $error.message + " (code: " + $error.code + ")"
)

[api_call] on error -> [log_error]
```

### Error Object Structure

```json
{
  "code": "NETWORK_ERROR",
  "message": "Connection refused",
  "node": "api_call",
  "details": {}
}
```

## Common Error Handling Patterns

### Retry Pattern

```juglans
name: "Retry Pattern"

entry: [init]
exit: [success, give_up]

[init]: set_context(
  attempt=0,
  max_attempts=3,
  backoff_ms=1000
)
[try]: fetch_url(url=$input.url)
[check_retry]: set_context(attempt=$ctx.attempt + 1)
[wait]: timer(ms=$ctx.backoff_ms * $ctx.attempt)
[success]: notify(status="Success after " + $ctx.attempt + " attempts")
[give_up]: notify(status="Failed after " + $ctx.max_attempts + " attempts")

[init] -> [try]
[try] -> [success]
[try] on error -> [check_retry]
[check_retry] if $ctx.attempt < $ctx.max_attempts -> [wait]
[check_retry] -> [give_up]
[wait] -> [try]
```

### Fallback Pattern

Use a backup service when the primary service fails:

```juglans
name: "Fallback Pattern"

entry: [primary]
exit: [done]

# Primary service
[primary]: fetch_url(url=$input.primary_api)
# Backup service
[fallback]: fetch_url(url=$input.fallback_api)
# Use cache
[use_cache]: set_context(data=$ctx.cached_data)
[process]: chat(agent="processor", message=json($output))
[done]: notify(status="Complete")

[primary] -> [process]
[primary] on error -> [fallback]
[fallback] -> [process]
[fallback] on error -> [use_cache]
[use_cache] -> [process]
[process] -> [done]
```

### Circuit Breaker Pattern

Pause calls after consecutive failures:

```juglans
name: "Circuit Breaker"

entry: [check_circuit]
exit: [done]

[check_circuit]: set_context(
  circuit_open=$ctx.failures >= 5,
  now=timestamp()
)
# Circuit breaker is open, check if it can be half-open
[check_half_open]: set_context(half_open=true)
[circuit_open_response]: set_context(
  response={"error": "Service temporarily unavailable"}
)
[call_api]: fetch_url(url=$input.api_url)
[reset_failures]: set_context(failures=0)
[increment_failures]: set_context(
  failures=$ctx.failures + 1,
  last_failure=timestamp()
)
[process]: chat(agent="processor", message=$output)
[handle_error]: notify(status="API failed, failures: " + $ctx.failures)
[done]: notify(status="Complete")

[check_circuit] if $ctx.circuit_open -> [check_half_open]
[check_circuit] -> [call_api]
[check_half_open] if $ctx.now - $ctx.last_failure > 30000 -> [call_api]
[check_half_open] -> [circuit_open_response]
[circuit_open_response] -> [done]
[call_api] -> [reset_failures]
[call_api] on error -> [increment_failures]
[reset_failures] -> [process]
[increment_failures] -> [handle_error]
[process] -> [done]
[handle_error] -> [done]
```

### Compensation Pattern

Undo completed operations upon failure:

```juglans
name: "Compensation Pattern"

entry: [step1]
exit: [success, compensated]

# Step 1
[step1]: create_order(data=$input.order)
# Step 2
[step2]: reserve_inventory(order_id=$output.order_id)
# Step 3
[step3]: charge_payment(order_id=$ctx.order_id, amount=$ctx.amount)
# Compensation operations
[compensate_step2]: release_inventory(order_id=$ctx.order_id)
[compensate_step1]: cancel_order(order_id=$ctx.order_id)
[fail_early]: notify(status="Failed to create order")
[success]: notify(status="Order completed: " + $ctx.order_id)
[compensated]: notify(status="Transaction rolled back")

[step1] -> [step2]
[step1] on error -> [fail_early]
[step2] -> [step3]
[step2] on error -> [compensate_step1]
[step3] -> [success]
[step3] on error -> [compensate_step2]
[compensate_step2] -> [compensate_step1]
[compensate_step1] -> [compensated]
[fail_early] -> [compensated]
```

### Partial Success Pattern

Record individual failures in batch operations:

```juglans
name: "Partial Success"

entry: [init]
exit: [summary]

[init]: set_context(
  successes=[],
  failures=[]
)

[process_batch]: foreach($item in $input.items) {
  [process_item]: fetch_url(url=$item.url)
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

  [process_item] -> [record_success]
  [process_item] on error -> [record_failure]
}

[summary]: notify(
  status="Completed: " + len($ctx.successes) + " success, " +
         len($ctx.failures) + " failed"
)

[init] -> [process_batch] -> [summary]
```

## Validation and Guards

### Input Validation

```juglans
name: "Input Validation"

entry: [validate]
exit: [result, error]

[validate]: chat(
  agent="validator",
  message="Validate: " + json($input),
  format="json"
)
[reject]: set_context(
  error={
    "code": "VALIDATION_ERROR",
    "message": $output.reason,
    "fields": $output.invalid_fields
  }
)
[process]: chat(agent="processor", message=$input.data)
[result]: set_context(response=$output)
[error]: set_context(response=$ctx.error)

[validate] if !$output.valid -> [reject]
[validate] -> [process]
[reject] -> [error]
[process] -> [result]
```

### Conditional Guards

```juglans
[check_permission]: set_context(
  has_permission=$ctx.user.role == "admin" || $ctx.user.id == $input.owner_id
)
[unauthorized]: set_context(
  error={"code": "FORBIDDEN", "message": "Permission denied"}
)
[proceed]: print(msg="Access granted")

[check_permission] if !$ctx.has_permission -> [unauthorized]
[check_permission] -> [proceed]
```

## Error Propagation

### Explicit Propagation

```juglans
[inner_call]: fetch_url(url=$input.url)
[propagate]: set_context(
  error={
    "code": "INNER_ERROR",
    "message": "Inner call failed: " + $error.message,
    "cause": $error
  }
)
[outer_handler]: notify(status="Outer error handler reached")

[inner_call] on error -> [propagate]
[propagate] on error -> [outer_handler]
```

### Error Aggregation

```juglans
[collect_errors]: set_context(
  all_errors=concat($ctx.all_errors, [$error])
)

# Summarize at the end
[report]: notify(
  status="Errors: " + json($ctx.all_errors)
)
```

## Logging and Monitoring

### Error Logging

```juglans
[handle_error]: notify(
  status="[ERROR] " + $error.code + ": " + $error.message +
         " | Node: " + $error.node +
         " | Time: " + now()
)
```

### Alert Notifications

```juglans
[critical_error]: chat(
  agent="alerter",
  message="Critical error in workflow: " + json($error)
)

# Or send to an external service
[alert]: mcp_slack_send_message(
  channel="#alerts",
  text="Workflow error: " + $error.message
)
```

## Debugging Tips

### Verbose Logging Mode

```bash
juglans src/my-flow.jg --verbose
```

### Error Breakpoints

```juglans
[debug_error]: notify(
  status="DEBUG: Error at " + $error.node + "\n" +
         "Input: " + json($ctx.last_input) + "\n" +
         "Error: " + json($error)
)
# You can pause here to inspect the state
```

### Simulating Errors

Test error handling logic:

```juglans
[simulate_error]: set_context(
  should_fail=$input.test_mode && $input.simulate_error
)
[maybe_fail]: print(msg="Checking if should fail")
[force_error]: fail(message="Simulated error for testing")
[real_call]: fetch_url(url=$input.url)

[simulate_error] -> [maybe_fail]
[maybe_fail] if $ctx.should_fail -> [force_error]
[maybe_fail] -> [real_call]
```

## Best Practices

### 1. Always Handle Errors

```juglans
# Good: explicitly handle errors
[api]: fetch_url(url=$input.url)
[success]: notify(status="ok")
[handle]: notify(status="error")
[next]: print(msg="next")

[api] -> [success]
[api] on error -> [handle]
```

### 2. Provide Meaningful Error Messages

```juglans
# Good
[error]: set_context(error={
  "code": "PAYMENT_FAILED",
  "message": "Payment processing failed: insufficient funds",
  "details": {"balance": $ctx.balance, "required": $input.amount}
})

# Bad
[error]: set_context(error="Error")
```

### 3. Use Error Codes

```juglans
# Define standard error codes
# VALIDATION_ERROR - Input validation failed
# AUTH_ERROR - Authentication/authorization failed
# NOT_FOUND - Resource does not exist
# RATE_LIMITED - Too many requests
# INTERNAL_ERROR - Internal error

[handle]: set_context(error_code="VALIDATION_ERROR")
```

### 4. Limit Retry Attempts

```juglans
# Good: has a limit
[retry]: set_context(attempts=$ctx.attempts + 1)
[try_again]: fetch_url(url=$input.url)

[retry] if $ctx.attempts < 3 -> [try_again]
```

### 5. Record Error Context

```juglans
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
