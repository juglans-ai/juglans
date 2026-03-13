# Tutorial 3: Branching & Routing

This chapter covers two routing mechanisms: **conditional edges (if)** and **multi-way switching (switch)**, enabling workflows to make decisions based on data.

## Conditional Edges -- if

The most basic branching: take different paths based on a condition.

```juglans
[check]: score = 85
[pass]: print(message="Passed!")
[fail]: print(message="Failed.")
[done]: print(message="Done")

[check] if score >= 60 -> [pass]
[check] if score < 60 -> [fail]
[pass] -> [done]
[fail] -> [done]
```

Line-by-line explanation:

1. The `[check]` node writes `score` to the context with a value of 85.
2. `[check] if score >= 60 -> [pass]` -- if the score >= 60, take the `[pass]` path.
3. `[check] if score < 60 -> [fail]` -- if the score < 60, take the `[fail]` path.
4. Both paths ultimately converge at `[done]`.

Conditional edge syntax:

```text
[source_node] if condition_expression -> [target_node]
```

The edge is only traversed when the condition evaluates to true.

### Supported Operators

**Comparison operators:**

| Operator | Meaning | Example |
|----------|---------|---------|
| `==` | Equal to | `status == "ok"` |
| `!=` | Not equal to | `status != "error"` |
| `>` | Greater than | `score > 80` |
| `<` | Less than | `score < 60` |
| `>=` | Greater than or equal to | `level >= 3` |
| `<=` | Less than or equal to | `count <= 10` |

**Logical operators:**

| Operator | Meaning | Example |
|----------|---------|---------|
| `&&` or `and` | And | `a && b` |
| `\|\|` or `or` | Or | `a \|\| b` |
| `!` or `not` | Not | `!banned` |

### String Comparison

String values are wrapped in double quotes:

```juglans
[input]: type = "question"
[question]: print(message="Handling question")
[task]: print(message="Handling task")
[other]: print(message="Unknown type")

[input] if type == "question" -> [question]
[input] if type == "task" -> [task]
[input] -> [other]
```

Note the last line `[input] -> [other]` -- this is an **unconditional edge**, serving as the default path when none of the preceding conditions are satisfied.

## Multi-Condition Combinations

Combine multiple conditions with logical operators:

```juglans
[check]: role = "admin", level = 5, banned = false
[admin]: print(message="Welcome, admin")
[vip]: print(message="VIP access")
[normal]: print(message="Normal user")
[blocked]: print(message="Access denied")

[check] if banned -> [blocked]
[check] if role == "admin" && level > 3 -> [admin]
[check] if role == "vip" || level > 8 -> [vip]
[check] -> [normal]
```

Conditions are evaluated in the order they are defined. The first condition that evaluates to true wins, and subsequent conditions are **not checked**. Therefore, place the most specific conditions first.

### Branch Convergence

Multiple paths converging at a single node is a common pattern:

```juglans
[evaluate]: grade = "B"
[excellent]: print(message="Outstanding!")
[good]: print(message="Well done")
[average]: print(message="Keep going")
[summary]: print(message="Evaluation complete")

[evaluate] if grade == "A" -> [excellent]
[evaluate] if grade == "B" -> [good]
[evaluate] -> [average]

[excellent] -> [summary]
[good] -> [summary]
[average] -> [summary]
```

`[summary]` has three predecessor nodes, but only one path will actually execute. Juglans uses **OR semantics**: the convergence node executes as soon as any one predecessor completes. Branches that were not taken are automatically marked as unreachable.

## Switch Routing -- Multi-Way Exclusive

When branching is based on the value of a single variable with multiple possible outcomes, `switch` is cleaner than multiple `if` edges:

```juglans
[classify]: intent = "question"
[handle_q]: print(message="Question handler")
[handle_t]: print(message="Task handler")
[handle_c]: print(message="Chat handler")
[fallback]: print(message="Unknown intent")
[done]: print(message="Done")

[classify] -> switch intent {
    "question": [handle_q]
    "task": [handle_t]
    "chat": [handle_c]
    default: [fallback]
}
[handle_q] -> [done]
[handle_t] -> [done]
[handle_c] -> [done]
[fallback] -> [done]
```

Syntax structure:

```text
[source_node] -> switch variable {
    "value1": [target1]
    "value2": [target2]
    default: [fallback_node]
}
```

Rules:

- The variable's value is matched against each case in order. Only the **first** matching branch is taken.
- `default` handles all unmatched cases.
- `default` is not required, but it is strongly recommended to always include one to avoid dead-end situations where no path is taken.

### switch vs if

When should you use which?

| Scenario | Recommended | Reason |
|----------|-------------|--------|
| Multi-way selection based on a single variable's value | `switch` | Semantically clear, one block handles everything |
| Binary choice | `if` | Concise, two lines are enough |
| Complex conditions (ranges, logical combinations) | `if` | switch only does equality matching |
| Need a default path | Either | switch uses `default`, if uses an unconditional edge |

The key difference: `switch` guarantees that only one branch is taken, while `if` conditional edges can theoretically satisfy multiple conditions simultaneously (though the engine only takes the first true one, in order).

## Mixing Unconditional and Conditional Edges

A node can have both unconditional and conditional edges:

```juglans
[start]: priority = "high"
[log]: print(message="Logging request...")
[fast_track]: print(message="Priority routing!")
[done]: print(message="Complete")

[start] -> [log]
[start] if priority == "high" -> [fast_track]
[log] -> [done]
[fast_track] -> [done]
```

Execution behavior:

- The unconditional edge `[start] -> [log]` is **always** executed.
- The conditional edge `[start] if ... -> [fast_track]` is only executed when the condition is true.
- If the condition is true, **both** `[log]` and `[fast_track]` will execute, and both will converge at `[done]`.

This differs from `switch`'s "only one branch" behavior. If you need strict mutual exclusivity, use `switch` or ensure your `if` conditions are mutually exclusive.

## Comprehensive Example

A workflow that routes based on message type and priority:

```juglans
[receive]: type = "task", priority = "high"

[done]: print(message="Routing complete")

# Layer 1: Route by message type
[route_type]: print(message="Routing by type...")
[handle_question]: print(message="Answering question")
[route_task]: print(message="Processing task...")
[handle_other]: print(message="General handler")

[receive] -> [route_type]

[route_type] -> switch type {
    "question": [handle_question]
    "task": [route_task]
    default: [handle_other]
}

# Layer 2: Route tasks by priority
[urgent]: print(message="URGENT: handling immediately")
[normal]: print(message="Queued for processing")

[route_task] if priority == "high" -> [urgent]
[route_task] -> [normal]

# All paths converge
[handle_question] -> [done]
[urgent] -> [done]
[normal] -> [done]
[handle_other] -> [done]
```

This example demonstrates two-layer routing composition:

1. The first layer uses `switch` to route by type.
2. The second layer uses `if` to further split the task path by priority.
3. All branches ultimately converge at `[done]`.

This is the most common routing pattern in real projects: coarse-grained routing first, fine-grained routing second, then merge all paths.

## Summary

- **Conditional edges** `[node] if expr -> [target]` -- the edge is taken when the condition is true
- **switch** `[node] -> switch var { "val": [target], default: [fb] }` -- multi-way exclusive selection based on a variable's value
- Conditions are evaluated in definition order; the first true condition wins
- Unconditional edges `[node] -> [target]` can serve as a default path
- Branch convergence uses OR semantics: the convergence node triggers when any predecessor completes
- `switch` is best for equality-based multi-way selection; `if` is best for complex conditions and binary choices

Next chapter: [Tutorial 4: Loops](./loops.md) -- learn `foreach` and `while` to make workflows repeat execution.
