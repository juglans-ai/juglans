# Tutorial 4: Loops

This chapter covers two loop constructs: **foreach** for iterating over lists, and **while** for conditional loops, enabling workflows to repeatedly execute a set of operations.

## foreach Basics

Iterate over a list, executing the loop body for each element:

```juglans
[init]: set_context(items=["apple", "banana", "cherry"])

[loop]: foreach($item in $ctx.items) {
  [show]: print(message="Fruit: " + $item)
}

[done]: print(message="All done")

[init] -> [loop] -> [done]
```

Line-by-line explanation:

1. `[init]` stores a string array into `$ctx.items`.
2. `[loop]` is the loop node. `foreach($item in $ctx.items)` means: iterate over `$ctx.items`, binding the current element to `$item` on each iteration.
3. The braces `{ ... }` contain the **loop body**, which executes once per iteration. Here there is only one node `[show]`, which prints the current fruit name.
4. After the loop finishes, execution follows the edge to `[done]`.

foreach syntax:

```text
[node_name]: foreach($variable in $collection) {
    loop body (nodes + edges)
}
```

The `$` prefix on `$item` is optional — `foreach(item in $ctx.items)` is equally valid. However, keeping the `$` prefix is recommended for consistency with variable references.

## Multiple Nodes Inside a foreach Body

The loop body is not limited to a single node. You can define multiple nodes connected by edges:

```juglans
[init]: set_context(
  tasks=["compile", "test", "deploy"],
  completed=0
)

[process]: foreach($task in $ctx.tasks) {
  [start]: print(message="Starting: " + $task)
  [run]: notify(status="Running " + $task + "...")
  [finish]: print(message="Finished: " + $task)
  [start] -> [run] -> [finish]
}

[report]: print(message="All tasks processed")

[init] -> [process] -> [report]
```

The three nodes `[start] -> [run] -> [finish]` inside the loop body form a chain, executing in order on each iteration. The loop body is essentially a nested subgraph — node names have their own scope within the loop body and will not conflict with external nodes.

## foreach with $input

The most common use of foreach is iterating over external input:

```juglans
[loop]: foreach($user in $input.users) {
  [greet]: print(message="Hello, " + $user)
}
```

Run:

```bash
juglans greet.jg --input '{"users": ["Alice", "Bob", "Charlie"]}'
```

## while Loops

When you need to "repeat until a condition is no longer met" rather than iterate over a list, use `while`:

```juglans
[init]: set_context(count=0)

[loop]: while($ctx.count < 5) {
  [step]: set_context(count=$ctx.count + 1)
  [log]: print(message="Count: " + str($ctx.count))
  [step] -> [log]
}

[done]: print(message="Loop finished")

[init] -> [loop] -> [done]
```

Line-by-line explanation:

1. `[init]` initializes the counter `count` to 0.
2. `[loop]` is the while loop node. Before each iteration, it checks the condition `$ctx.count < 5` — if true, the loop body executes.
3. Inside the loop body, `[step]` increments `count` by 1, and `[log]` prints the current value.
4. When `$ctx.count` reaches 5, the condition becomes false, the loop ends, and `[done]` executes.

while syntax:

```text
[node_name]: while(condition_expression) {
    loop body (nodes + edges)
}
```

The loop body **must modify the condition variable**, otherwise it will produce an infinite loop. Juglans has a built-in maximum iteration limit — exceeding it will automatically terminate the loop and report an error.

### Supported while Condition Expressions

while condition expressions use the same syntax as `if` conditional edges, supporting all comparison and logical operators:

| Expression | Meaning |
|--------|------|
| `$ctx.count < 10` | Less than |
| `$ctx.status != "done"` | Not equal to |
| `$ctx.active && $ctx.count < 100` | Logical AND |

## Using Context Within Loops

One of the core capabilities of loops is **accumulating results** across iterations. The foreach loop variable is overwritten on each iteration, but `$ctx` persists throughout the entire workflow — you can leverage this to collect data within loops:

```juglans
[init]: set_context(total=0, items=[10, 20, 30, 40])

[sum]: foreach($n in $ctx.items) {
  [add]: set_context(total=$ctx.total + $n)
}

[result]: print(message="Sum: " + str($ctx.total))

[init] -> [sum] -> [result]
```

Execution trace:

| Iteration | $n | $ctx.total (after iteration) |
|------|----|---------------------|
| 1    | 10 | 10                  |
| 2    | 20 | 30                  |
| 3    | 30 | 60                  |
| 4    | 40 | 100                 |

`[result]` prints `Sum: 100`.

The same pattern applies to while loops. The following example builds a list within a while loop:

```juglans
[init]: set_context(i=0, squares=[])

[build]: while($ctx.i < 5) {
  [calc]: set_context(
    squares=append($ctx.squares, $ctx.i * $ctx.i),
    i=$ctx.i + 1
  )
}

[show]: print(message="Squares: " + json($ctx.squares))

[init] -> [build] -> [show]
```

The `append()` function appends a new element to the end of an array and returns the new array. Each iteration appends the square of `i` to `squares`.

## Comprehensive Example

A data processing pipeline: receive a batch of records, filter and summarize them.

```juglans
[init]: set_context(
  records=[
    {"name": "Alice", "score": 85},
    {"name": "Bob", "score": 42},
    {"name": "Charlie", "score": 91},
    {"name": "Diana", "score": 67},
    {"name": "Eve", "score": 55}
  ],
  passed=0,
  total=0
)

[process]: foreach($record in $ctx.records) {
  [count]: set_context(total=$ctx.total + 1)
  [check]: set_context(
    passed=$ctx.passed + 1
  )
  [count] -> [check]
}

[report]: print(
  message="Results: " + str($ctx.passed) + "/" + str($ctx.total) + " processed"
)

[init] -> [process] -> [report]
```

This workflow demonstrates a typical use of loops in real-world scenarios:

1. **Initialization** — Prepare data and accumulators.
2. **foreach iteration** — Process records one by one, updating counters in the context.
3. **Summary output** — Read the accumulated results after the loop completes.

## Summary

- **foreach** `[node]: foreach($item in $list) { ... }` — Iterate over a list, executing the loop body once per element
- **while** `[node]: while(condition) { ... }` — Repeat the loop body while the condition is true
- The loop body is a nested subgraph that can contain multiple nodes and edges
- Use `$ctx` to accumulate data across iterations (counting, summing, building lists)
- The while loop body must modify the condition variable; the engine has a maximum iteration limit for protection
- The `append()` function appends elements to an array

Next chapter: [Tutorial 5: Error Handling](./error-handling.md) -- Learn about `on error` edges and error recovery patterns to handle failures gracefully in workflows.
