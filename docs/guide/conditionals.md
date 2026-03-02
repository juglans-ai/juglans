# Conditional Branching and Routing

This guide provides a detailed introduction to conditional branching and routing mechanisms in Juglans workflows.

## Basic Conditional Syntax

```yaml
[node] if <expression> -> [target]
```

When the condition is true, execution flow jumps from `[node]` to `[target]`.

## Conditional Expressions

### Comparison Operations

```yaml
# Equal to
[router] if $ctx.type == "admin" -> [admin_panel]

# Not equal to
[check] if $output.status != "success" -> [error_handler]

# Numeric comparison
[score] if $output.score > 80 -> [high_tier]
[score] if $output.score >= 60 -> [mid_tier]
[score] if $output.score < 60 -> [low_tier]
```

### String Comparison

```yaml
# Exact match
[lang] if $input.language == "zh" -> [chinese_handler]
[lang] if $input.language == "en" -> [english_handler]

# Contains check (using chat classification)
[classify]: chat(agent="classifier", format="json")
[classify] if $output.contains_code == true -> [code_handler]
```

### Boolean Values

```yaml
# true/false
[validate] if $output.is_valid -> [proceed]
[validate] if !$output.is_valid -> [reject]

# Non-empty check
[check] if $ctx.data -> [process]
[check] if !$ctx.data -> [fetch_data]
```

### Logical Operations

```yaml
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

```yaml
name: "Multi-way Router"

entry: [classify]
exit: [done]

[classify]: chat(
  agent="intent-classifier",
  message=$input.query,
  format="json"
)

# Multiple mutually exclusive conditions
[classify] if $output.intent == "question" -> [qa_handler]
[classify] if $output.intent == "task" -> [task_handler]
[classify] if $output.intent == "chat" -> [chat_handler]
[classify] -> [fallback]  # Default path

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

### Branch Convergence Semantics

**Important:** When multiple conditional branches converge to the same node, the execution engine uses **OR semantics**, meaning:

- The convergence node executes as soon as **any one** predecessor node completes
- It does **not** wait for all predecessor nodes to complete (not AND semantics)

```yaml
# Branching then converging
[router] if $ctx.type == "A" -> [handler_a]
[router] if $ctx.type == "B" -> [handler_b]

[handler_a]: chat(agent="agent-a", message=$input)
[handler_b]: chat(agent="agent-b", message=$input)

[final]: notify(status="Done")

# final node has two predecessors, but only one will execute
# The execution engine automatically detects unexecuted branches and marks them as unreachable
[handler_a] -> [final]
[handler_b] -> [final]
```

This ensures the intuitive behavior of conditional branches: branch paths are mutually exclusive, and the convergence point does not wait for unexecuted branches.

### Priority Branching

Conditions are evaluated in definition order; the first true branch is executed:

```yaml
# From high to low priority
[score] if $output.score >= 90 -> [excellent]   # Checked first
[score] if $output.score >= 70 -> [good]        # Checked second
[score] if $output.score >= 60 -> [pass]        # Checked third
[score] -> [fail]                                # Default
```

### Default Path

An unconditional edge serves as the default path:

```yaml
[router] if $ctx.a -> [path_a]
[router] if $ctx.b -> [path_b]
[router] -> [default]  # If both a and b are false
```

## Error Handling

### on error Path

```yaml
[risky_call] -> [success]
[risky_call] on error -> [error_handler]

[error_handler]: notify(status="Error occurred, using fallback")
[fallback]: chat(agent="fallback", message=$input.query)

[error_handler] -> [fallback]
```

### Conditional + Error Handling

```yaml
[api_call]: fetch_url(url=$input.api_url)
[api_call] if $output.status == "ok" -> [process]
[api_call] if $output.status == "rate_limited" -> [wait_retry]
[api_call] on error -> [error_handler]
```

## Common Patterns

### Intent Router

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

### Validation Pipeline

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
[human_review] -> [success]  # Assuming human review passes
[process] -> [success]
```

### Retry Logic

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

### Quality Check

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

### A/B Testing

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

## Debugging Conditions

### Printing Condition Values

```yaml
[debug]: notify(status="Type: " + $ctx.type + ", Score: " + $ctx.score)
[debug] -> [router]

[router] if $ctx.type == "a" -> [path_a]
...
```

### Logging Routing Decisions

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

## Best Practices

1. **Define a clear default path** - Always provide an unconditional default branch
2. **Mutually exclusive conditions** - Ensure branch conditions do not overlap
3. **Clear priority order** - Place more specific conditions first
4. **Error handling** - Add `on error` for nodes that may fail
5. **Logging** - Log the decision process in complex routing scenarios
