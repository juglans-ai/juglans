# Tutorial 2: Variables & Data Flow

Workflows are useful because data flows through them. This chapter teaches you the four ways to access and pass data in juglans: `input`, `output`, context variables, and expression functions.

## 2.1 input -- External Input

`input` is the JSON data passed when a workflow starts. Use dot notation to access fields.

```juglans
[greet]: print(message="Hello, " + input.name + "!")
[done]: print(message="Done")
[greet] -> [done]
```

Run it:

```bash
juglans hello.jg --input '{"name": "Alice"}'
```

Output:

```text
Hello, Alice!
Done
```

Breaking it down:

- `input` is the entire JSON object `{"name": "Alice"}`
- `input.name` drills into the `name` field, returning `"Alice"`
- `"Hello, " + input.name + "!"` concatenates three strings

### Nested Access

`input` supports any depth of nesting:

```juglans
[show]: print(message="User: " + input.user.name)
[done]: print(message="Done")
[show] -> [done]
```

```bash
juglans show.jg --input '{"user": {"name": "Bob"}}'
```

For arrays, use bracket indexing: `input.items[0]` accesses the first element.

### What if input is missing?

If you run the workflow without `--input`, or access a field that doesn't exist, the variable resolves to `null`. No crash -- but you'll get `"null"` in your string output. Always provide the expected input.

## 2.2 output -- Previous Node's Output

Every node produces a return value. After a node executes, its return value is stored in `output`. The next node in the chain can read it.

```juglans
[step1]: print(message="hello from step1")
[step2]: print(message="step1 returned: " + output)
[step1] -> [step2]
```

Output:

```text
hello from step1
step1 returned: hello from step1
```

Here is what happened:

1. `[step1]` calls `print(message="hello from step1")`. The `print` tool returns its message as its output.
2. The engine stores `"hello from step1"` in `output`.
3. `[step2]` reads `output` and uses it.

### output is always the *last* node

A critical rule: `output` is the return value of the most recently executed node, not a specific node. If you have a three-node chain:

```juglans
[a]: print(message="AAA")
[b]: print(message="BBB")
[c]: print(message="output is: " + output)
[a] -> [b] -> [c]
```

Output:

```text
AAA
BBB
output is: BBB
```

When `[c]` runs, `output` is `"BBB"` (from `[b]`), not `"AAA"` (from `[a]`). Each node overwrites `output`.

### Named Node Output

Need to access an earlier node's result? Use `node_id.output`:

```juglans
[step1]: print(message="hello")
[step2]: print(message="world")
[step3]: print(message=step1.output + " " + step2.output)
[step1] -> [step2] -> [step3]
```

Output:

```text
hello
world
hello world
```

Every node's output is also stored at `node_id.output` and persists for the entire workflow. Use this when you need data from a node that isn't the immediate predecessor.

## 2.3 Context Variables

`input` is read-only and `output` gets overwritten every step. When you need persistent, writable storage, use **context variables**.

Use assignment syntax to set, read by name:

```juglans
[init]: greeting = "Good morning", count = 3
[show]: print(message=greeting + " — count is " + str(count))
[init] -> [show]
```

Output:

```text
Good morning — count is 3
```

Line by line:

- `greeting = "Good morning", count = 3` stores two values: a string and a number.
- `greeting` reads the string `"Good morning"`.
- `count` reads the number `3`. Since we need to concatenate it with a string, we use `str()` to convert it first.

### Context Persists Across Nodes

Unlike `output`, context variables survive the whole workflow:

```juglans
[step1]: total = 10
[step2]: total = total + 20
[step3]: print(message="Total: " + str(total))
[step1] -> [step2] -> [step3]
```

Output:

```text
Total: 30
```

`[step1]` sets `total` to `10`. `[step2]` reads `total` (which is `10`), adds `20`, and writes `30` back. `[step3]` reads the final value.

### From Hardcoded to Dynamic

A common pattern: start with hardcoded values, then switch to `input`:

```juglans
[init]: name = input.name, role = input.role
[greet]: print(message="Hello, " + name + "! Role: " + role)
[init] -> [greet]
```

```bash
juglans greet.jg --input '{"name": "Alice", "role": "admin"}'
```

Output:

```text
Hello, Alice! Role: admin
```

This is useful when multiple nodes need the same input values -- store them in context once, read them everywhere.

### Saving output to Context

Since `output` gets overwritten, save important results to context:

```juglans
[step1]: print(message="important data")
[save]: saved = output
[step2]: print(message="other work")
[step3]: print(message="saved value: " + saved)
[step1] -> [save] -> [step2] -> [step3]
```

Output:

```text
important data
other work
saved value: important data
```

Even though `[step2]` overwrote `output`, `saved` still holds the value from `[step1]`.

## 2.4 Expressions & Built-in Functions

Juglans has a built-in expression language. You've already seen string concatenation with `+`. Here are the most useful functions.

### String Concatenation

Use `+` to join strings:

```juglans
[greet]: print(message="Hello, " + input.name + "! Welcome.")
[done]: print(message="Done")
[greet] -> [done]
```

### str() -- Number to String

Numbers and strings cannot be concatenated directly. Use `str()` to convert:

```juglans
[init]: count = 42
[show]: print(message="The answer is " + str(count))
[init] -> [show]
```

Output:

```text
The answer is 42
```

### len() -- Get Length

Works on strings, arrays, and objects:

```juglans
[show]: print(message="length of 'juglans' is " + str(len("juglans")))
[done]: print(message="Done")
[show] -> [done]
```

Output:

```text
length of 'juglans' is 7
```

### upper() / lower() -- Case Conversion

```juglans
[a]: print(message=upper("hello world"))
[b]: print(message=lower("HELLO WORLD"))
[a] -> [b]
```

Output:

```text
HELLO WORLD
hello world
```

### Combining Functions

Functions can be nested and combined with variables:

```juglans
[init]: name = "alice"
[show]: print(message="Hello, " + upper(name) + "! (length: " + str(len(name)) + ")")
[init] -> [show]
```

Output:

```text
Hello, ALICE! (length: 5)
```

### Quick Reference

| Function | Description | Example |
|----------|-------------|---------|
| `str(x)` | Convert to string | `str(42)` -> `"42"` |
| `int(x)` | Convert to integer | `int("42")` -> `42` |
| `len(x)` | Length of string/array/object | `len("hi")` -> `2` |
| `upper(x)` | Uppercase | `upper("hi")` -> `"HI"` |
| `lower(x)` | Lowercase | `lower("HI")` -> `"hi"` |
| `type(x)` | Type name | `type(42)` -> `"number"` |
| `round(x, n)` | Round to n digits | `round(3.14159, 2)` -> `3.14` |
| `default(x, fallback)` | Use fallback if x is null/empty | `default(name, "anon")` |
| `json(x)` | Serialize to JSON / parse from JSON | `json(data)` |

## 2.5 Putting It All Together

Here is a complete workflow that combines all four concepts: `input` for external data, context variables for shared state, `output` for chaining, and expression functions for transformations.

```juglans
[init]: greeting = "Welcome", visit_count = 0

[personalize]: greeting = greeting + ", " + input.name, visit_count = visit_count + 1

[format]: print(message=upper(greeting) + "! (visit #" + str(visit_count) + ")")

[save]: last_message = output

[summary]: print(message="Saved: " + last_message)

[init] -> [personalize] -> [format] -> [save] -> [summary]
```

```bash
juglans pipeline.jg --input '{"name": "Alice"}'
```

Output:

```text
WELCOME, ALICE! (visit #1)
Saved: WELCOME, ALICE! (visit #1)
```

Data flow through this workflow:

```text
input.name = "Alice"
       |
  [init]  sets greeting = "Welcome", visit_count = 0
       |
  [personalize]  reads greeting + input, writes greeting = "Welcome, Alice"
       |
  [format]  reads greeting, applies upper(), prints, stores to output
       |
  [save]  reads output, saves to last_message
       |
  [summary]  reads last_message, prints final result
```

## Summary

| Variable | Purpose | Scope | Writable |
|----------|---------|-------|----------|
| `input` | External JSON data | Entire workflow | No |
| `output` | Last node's return value | Overwritten each step | No |
| `node_id.output` | Specific node's return value | Entire workflow | No |
| context variables | Shared context storage | Entire workflow | Yes (via assignment syntax) |

Key rules:

1. **input** is set once at startup. Access nested fields with dot notation.
2. **output** is the most recent node's return value. It gets overwritten every step.
3. **node_id.output** persists -- use it to reach back to earlier nodes.
4. **Context variables** are your scratch space. Use assignment syntax to set, read by name.
5. Use **str()** when concatenating numbers with strings.

## Next Up

**[Tutorial 3: Branching & Routing](./branching.md)** -- Learn `if` conditions and `switch` routing to build workflows that make decisions.
