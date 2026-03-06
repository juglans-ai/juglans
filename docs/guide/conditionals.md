# Conditional Branching and Routing

This guide provides a detailed introduction to conditional branching and routing mechanisms in Juglans workflows.

## Basic Conditional Syntax

```juglans
[node]: set_context(ready=$input.ready)
[target]: print(msg="Condition met")

[node] if $ctx.ready -> [target]
```

When the condition is true, execution flow jumps from `[node]` to `[target]`.

## Conditional Expressions

### Comparison Operations

```juglans
# Equal to
[router]: print(msg="stub")
[admin_panel]: print(msg="stub")
[router] if $ctx.type == "admin" -> [admin_panel]

# Not equal to
[check]: print(msg="stub")
[error_handler]: print(msg="stub")
[check] if $output.status != "success" -> [error_handler]

# Numeric comparison
[score]: print(msg="stub")
[high_tier]: print(msg="stub")
[mid_tier]: print(msg="stub")
[low_tier]: print(msg="stub")
[score] if $output.score > 80 -> [high_tier]
[score] if $output.score >= 60 -> [mid_tier]
[score] if $output.score < 60 -> [low_tier]
```

### String Comparison

```juglans
# Exact match
[lang]: print(msg="stub")
[chinese_handler]: print(msg="stub")
[english_handler]: print(msg="stub")
[lang] if $input.language == "zh" -> [chinese_handler]
[lang] if $input.language == "en" -> [english_handler]

# Contains check (using chat classification)
[classify]: chat(agent="classifier", format="json")
[code_handler]: print(msg="stub")
[classify] if $output.contains_code == true -> [code_handler]
```

### Boolean Values

```juglans
# true/false
[validate]: print(msg="stub")
[proceed]: print(msg="stub")
[reject]: print(msg="stub")
[validate] if $output.is_valid -> [proceed]
[validate] if !$output.is_valid -> [reject]

# Non-empty check
[check]: print(msg="stub")
[process]: print(msg="stub")
[fetch_data]: print(msg="stub")
[check] if $ctx.data -> [process]
[check] if !$ctx.data -> [fetch_data]
```

### Logical Operations

```juglans
[check]: print(msg="stub")
[admin_route]: print(msg="stub")
[premium_route]: print(msg="stub")
[allow]: print(msg="stub")
[auth]: print(msg="stub")
[access]: print(msg="stub")

# AND
[check] if $ctx.logged_in && $ctx.is_admin -> [admin_route]

# OR
[check] if $ctx.is_vip || $ctx.is_admin -> [premium_route]

# NOT
[check] if !$ctx.banned -> [allow]

# Combined
[auth] if ($ctx.logged_in && $ctx.verified) || $ctx.is_admin -> [access]
```

## Multi-Way Branching

### Mutually Exclusive Branches

```juglans
name: "Multi-way Router"

entry: [classify]
exit: [done]

[classify]: chat(
  agent="intent-classifier",
  message=$input.query,
  format="json"
)

[qa_handler]: chat(agent="qa-expert", message=$input.query)
[task_handler]: chat(agent="task-executor", message=$input.query)
[chat_handler]: chat(agent="conversational", message=$input.query)
[fallback]: chat(agent="general", message=$input.query)

[done]: notify(status="Complete")

# Multiple mutually exclusive conditions
[classify] if $output.intent == "question" -> [qa_handler]
[classify] if $output.intent == "task" -> [task_handler]
[classify] if $output.intent == "chat" -> [chat_handler]
[classify] -> [fallback]  # Default path

[qa_handler] -> [done]
[task_handler] -> [done]
[chat_handler] -> [done]
[fallback] -> [done]
```

### Branch Convergence Semantics

**Important:** When multiple conditional branches converge to the same node, the execution engine uses **OR semantics**, meaning:

- The convergence node executes as soon as **any one** predecessor node completes
- It does **not** wait for all predecessor nodes to complete (not AND semantics)

```juglans
# Branching then converging
[router]: print(msg="stub")
[handler_a]: chat(agent="agent-a", message=$input)
[handler_b]: chat(agent="agent-b", message=$input)
[final]: notify(status="Done")

[router] if $ctx.type == "A" -> [handler_a]
[router] if $ctx.type == "B" -> [handler_b]

# final node has two predecessors, but only one will execute
# The execution engine automatically detects unexecuted branches and marks them as unreachable
[handler_a] -> [final]
[handler_b] -> [final]
```

This ensures the intuitive behavior of conditional branches: branch paths are mutually exclusive, and the convergence point does not wait for unexecuted branches.

### Priority Branching

Conditions are evaluated in definition order; the first true branch is executed:

```juglans
[score]: print(msg="stub")
[excellent]: print(msg="stub")
[good]: print(msg="stub")
[pass]: print(msg="stub")
[fail]: print(msg="stub")

# From high to low priority
[score] if $output.score >= 90 -> [excellent]   # Checked first
[score] if $output.score >= 70 -> [good]        # Checked second
[score] if $output.score >= 60 -> [pass]        # Checked third
[score] -> [fail]                                # Default
```

### Default Path

An unconditional edge serves as the default path:

```juglans
[router]: print(msg="stub")
[path_a]: print(msg="stub")
[path_b]: print(msg="stub")
[default]: print(msg="stub")

[router] if $ctx.a -> [path_a]
[router] if $ctx.b -> [path_b]
[router] -> [default]  # If both a and b are false
```

## Error Handling

### on error Path

```juglans
[risky_call]: print(msg="stub")
[success]: print(msg="stub")
[error_handler]: notify(status="Error occurred, using fallback")
[fallback]: chat(agent="fallback", message=$input.query)

[risky_call] -> [success]
[risky_call] on error -> [error_handler]
[error_handler] -> [fallback]
```

### Conditional + Error Handling

```juglans
[api_call]: fetch_url(url=$input.api_url)
[process]: print(msg="stub")
[wait_retry]: print(msg="stub")
[error_handler]: print(msg="stub")

[api_call] if $output.status == "ok" -> [process]
[api_call] if $output.status == "rate_limited" -> [wait_retry]
[api_call] on error -> [error_handler]
```

## Common Patterns

### Intent Router

```juglans
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

### Validation Pipeline

```juglans
name: "Validation Pipeline"

entry: [validate_input]
exit: [success, failure]

[validate_input]: chat(
  agent="validator",
  message=$input.data,
  format="json"
)

[reject]: set_context(error=$output.reason)
[human_review]: notify(status="Needs human review")
[process]: chat(agent="processor", message=$input.data)
[success]: notify(status="Validation passed")
[failure]: notify(status="Validation failed: " + $ctx.error)

[validate_input] if !$output.valid -> [reject]
[validate_input] if $output.needs_review -> [human_review]
[validate_input] -> [process]

[reject] -> [failure]
[human_review] -> [success]  # Assuming human review passes
[process] -> [success]
```

### Retry Logic

```juglans
name: "Retry Pattern"

entry: [init]
exit: [success, give_up]

[init]: set_context(attempt=0, max_attempts=3)
[try]: fetch_url(url=$input.url)
[success]: notify(status="Success!")
[check_retry]: set_context(attempt=$ctx.attempt + 1)
[wait]: timer(ms=1000)
[give_up]: notify(status="Failed after " + $ctx.max_attempts + " attempts")

[init] -> [try]
[try] if $output.ok -> [success]
[try] on error -> [check_retry]
[check_retry] if $ctx.attempt < $ctx.max_attempts -> [wait]
[check_retry] -> [give_up]
[wait] -> [try]
```

### Quality Check

```juglans
name: "Quality Check"

entry: [generate]
exit: [output]

[generate]: chat(agent="writer", message=$input.topic)

[check_quality]: chat(
  agent="reviewer",
  message="Review this content:\n" + $output,
  format="json"
)

[regenerate]: chat(
  agent="writer",
  message="Improve this: " + $ctx.content + "\nFeedback: " + $output.feedback
)

[output]: set_context(final=$output)

[generate] -> [check_quality]
[check_quality] if $output.score < 7 -> [regenerate]
[check_quality] -> [output]
[regenerate] -> [check_quality]
```

### A/B Testing

```juglans
name: "A/B Test"

entry: [assign_group]
exit: [collect]

[assign_group]: set_context(
  group=if(random() > 0.5, "A", "B")
)

[variant_a]: chat(agent="model-a", message=$input.query)
[variant_b]: chat(agent="model-b", message=$input.query)

[collect]: set_context(
  result=$output,
  variant=$ctx.group
)

[assign_group] if $ctx.group == "A" -> [variant_a]
[assign_group] if $ctx.group == "B" -> [variant_b]
[variant_a] -> [collect]
[variant_b] -> [collect]
```

## Debugging Conditions

### Printing Condition Values

```juglans
[debug]: notify(status="Type: " + $ctx.type + ", Score: " + $ctx.score)
[router]: print(msg="stub")
[path_a]: print(msg="stub")

[debug] -> [router]
[router] if $ctx.type == "a" -> [path_a]
```

### Logging Routing Decisions

```juglans
[router]: chat(agent="classifier", format="json")
[log_decision]: set_context(
  route_log=append($ctx.route_log, {
    "input": $input.query,
    "decision": $output,
    "timestamp": now()
  })
)
[path_a]: print(msg="stub")

[router] -> [log_decision]
[log_decision] if $output.intent == "a" -> [path_a]
```

## Best Practices

1. **Define a clear default path** - Always provide an unconditional default branch
2. **Mutually exclusive conditions** - Ensure branch conditions do not overlap
3. **Clear priority order** - Place more specific conditions first
4. **Error handling** - Add `on error` for nodes that may fail
5. **Logging** - Log the decision process in complex routing scenarios
