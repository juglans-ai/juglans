# Loop Structures

This guide introduces the loop mechanisms in Juglans workflows.

## Loop Types

Juglans supports two loop structures:

| Type | Purpose | Syntax |
|------|------|------|
| `foreach` | Iterate over a collection | `foreach($item in $collection)` |
| `while` | Conditional loop | `while($condition)` |

## Foreach Loop

Iterates over each element in an array or collection.

### Basic Syntax

```juglans
[loop_node]: foreach($item in $input.items) {
  [process]: some_tool(data=$item)
  [save]: set_context(last=$output)
  [process] -> [save]
}
```

### Complete Example

```juglans
name: "Batch Processing"

entry: [init]
exit: [summary]

[init]: set_context(results=[])

[process_all]: foreach($item in $input.documents) {
  # Process each document
  [analyze]: chat(
    agent="analyzer",
    message="Analyze: " + $item.content,
    format="json"
  )

  # Collect results
  [collect]: set_context(
    results=append($ctx.results, {
      "id": $item.id,
      "analysis": $output
    })
  )

  [analyze] -> [collect]
}

[summary]: chat(
  agent="summarizer",
  message="Summarize results: " + json($ctx.results)
)

[init] -> [process_all] -> [summary]
```

### Loop Variables

Available within the loop body:

| Variable | Type | Description |
|------|------|------|
| `$item` | any | Current element (name is customizable) |
| `loop.index` | number | Current index (1-based, starting from 1) |
| `loop.index0` | number | Current index (0-based, starting from 0) |
| `loop.first` | boolean | Whether this is the first iteration |
| `loop.last` | boolean | Whether this is the last iteration |

```juglans
[process]: foreach($doc in $input.docs) {
  [log]: notify(
    status="[" + (loop.index + 1) + "/" + len($input.docs) + "] " +
           "Processing: " + $doc.name +
           if(loop.first, " (first)", "") +
           if(loop.last, " (last)", "")
  )

  [handle]: chat(agent="processor", message=$doc.content)
  [log] -> [handle]
}
```

### Nested Loops

```juglans
[outer]: foreach($category in $input.categories) {
  [inner]: foreach($item in $category.items) {
    [process]: chat(
      agent="handler",
      message=$category.name + ": " + $item.name
    )
    [save]: set_context(last=$output)
    [process] -> [save]
  }
  [log]: notify(status="Category done: " + $category.name)
  [inner] -> [log]
}
```

## While Loop

A condition-based loop.

### Basic Syntax

```juglans
[loop_node]: while($ctx.count < $ctx.max) {
  [body]: some_tool(data=$ctx.count)
  [update]: set_context(count=$ctx.count + 1)
  [body] -> [update]
}
```

### Complete Example

```juglans
name: "Iterative Refinement"

entry: [init]
exit: [final]

[init]: set_context(
  iteration=0,
  max_iterations=5,
  quality_threshold=8,
  current_quality=0
)

[generate]: chat(agent="writer", message=$input.topic)

[refine_loop]: while($ctx.current_quality < $ctx.quality_threshold && $ctx.iteration < $ctx.max_iterations) {
  # Evaluate quality
  [evaluate]: chat(
    agent="critic",
    message="Rate this (1-10): " + $ctx.content,
    format="json"
  )

  # Update state
  [update_state]: set_context(
    current_quality=$output.score,
    feedback=$output.feedback,
    iteration=$ctx.iteration + 1
  )

  # If quality is insufficient, improve
  [improve]: chat(
    agent="writer",
    message="Improve based on feedback: " + $ctx.feedback + "\n\nOriginal:\n" + $ctx.content
  )

  [save]: set_context(content=$output)

  [evaluate] -> [update_state] -> [improve] -> [save]
}

[final]: notify(
  status="Final quality: " + $ctx.current_quality +
         " after " + $ctx.iteration + " iterations"
)

[init] -> [generate] -> [refine_loop] -> [final]
```

### Avoiding Infinite Loops

Always ensure the loop condition will eventually become false:

```juglans
# Good: has a clear termination condition
[loop1]: while($ctx.count < 10) {
  [inc]: set_context(count=$ctx.count + 1)
  [log]: notify(status="count: " + $ctx.count)
  [inc] -> [log]
}

# Good: has a maximum iteration limit
[loop2]: while($ctx.not_done && $ctx.attempts < 100) {
  [try]: notify(status="attempt " + $ctx.attempts)
  [update]: set_context(
    not_done=!$output.success,
    attempts=$ctx.attempts + 1
  )
  [try] -> [update]
}
```

## Common Patterns

### Batch API Calls

```juglans
name: "Batch API Calls"

entry: [init]
exit: [done]

[init]: set_context(
  results=[],
  errors=[]
)

[batch]: foreach($request in $input.requests) {
  [call]: fetch_url(
    url=$request.url,
    method=$request.method
  )

  [save_result]: set_context(
    results=append($ctx.results, {
      "id": $request.id,
      "response": $output
    })
  )

  [save_error]: set_context(
    errors=append($ctx.errors, {
      "id": $request.id,
      "error": "Request failed"
    })
  )

  [call] -> [save_result]
  [call] on error -> [save_error]
}

[done]: notify(
  status="Completed: " + len($ctx.results) + " success, " + len($ctx.errors) + " errors"
)

[init] -> [batch] -> [done]
```

### Paginated Fetching

```juglans
name: "Paginated Fetch"

entry: [init]
exit: [done]

[init]: set_context(
  page=1,
  all_items=[],
  has_more=true
)

[fetch_pages]: while($ctx.has_more && $ctx.page <= 100) {
  [fetch]: fetch_url(
    url=$input.api_url + "?page=" + $ctx.page
  )

  [process]: set_context(
    all_items=concat($ctx.all_items, $output.items),
    has_more=$output.has_next_page,
    page=$ctx.page + 1
  )

  [log]: notify(status="Fetched page " + ($ctx.page - 1))

  [fetch] -> [process] -> [log]
}

[done]: notify(status="Total items: " + len($ctx.all_items))

[init] -> [fetch_pages] -> [done]
```

### Recursive Processing

```juglans
name: "Tree Processing"

entry: [process_root]
exit: [done]

[process_root]: set_context(
  queue=$input.nodes,
  processed=[],
  has_items=true
)

[process_queue]: while($ctx.has_items) {
  # Dequeue the first element
  [dequeue]: set_context(
    current=first($ctx.queue),
    queue=rest($ctx.queue)
  )

  # Process the current node
  [handle]: chat(
    agent="processor",
    message="Process: " + json($ctx.current)
  )

  # Save result, add child nodes to the queue
  [update]: set_context(
    processed=append($ctx.processed, {
      "node": $ctx.current,
      "result": $output
    }),
    queue=concat($ctx.queue, $ctx.current.children),
    has_items=$ctx.queue != []
  )

  [dequeue] -> [handle] -> [update]
}

[done]: notify(status="Processed " + len($ctx.processed) + " nodes")

[process_root] -> [process_queue] -> [done]
```

### Convergence Iteration

```juglans
name: "Convergence Loop"

entry: [init]
exit: [converged]

[init]: set_context(
  value=0,
  prev_value=-999,
  tolerance=0.01,
  iteration=0,
  converging=true
)

[iterate]: while($ctx.converging && $ctx.iteration < 1000) {
  [compute]: chat(
    agent="calculator",
    message="Next iteration from: " + $ctx.value,
    format="json"
  )

  [update]: set_context(
    prev_value=$ctx.value,
    value=$output.result,
    iteration=$ctx.iteration + 1,
    converging=$ctx.value != $ctx.prev_value
  )

  [log]: notify(
    status="Iteration " + $ctx.iteration +
           ": " + $ctx.value
  )

  [compute] -> [update] -> [log]
}

[converged]: notify(
  status="Converged to " + $ctx.value +
         " after " + $ctx.iteration + " iterations"
)

[init] -> [iterate] -> [converged]
```

### Batch Processing with Progress Tracking

```juglans
name: "Progress Tracking"

entry: [init]
exit: [complete]

[init]: set_context(
  total=len($input.items),
  completed=0,
  results=[]
)

[process]: foreach($item in $input.items) {
  # Display progress
  [progress]: notify(
    status="Processing " + ($ctx.completed + 1) + "/" + $ctx.total +
           " (" + round(($ctx.completed / $ctx.total) * 100) + "%)"
  )

  # Process
  [handle]: chat(agent="processor", message=$item)

  # Update count
  [update]: set_context(
    completed=$ctx.completed + 1,
    results=append($ctx.results, $output)
  )

  [progress] -> [handle] -> [update]
}

[complete]: notify(status="Completed all " + $ctx.total + " items")

[init] -> [process] -> [complete]
```

## Performance Considerations

### Parallel vs Sequential

Loops are sequential by default. For independent operations, consider:

```juglans
# Sequential (default) - suitable for operations with dependencies
[process]: foreach($item in $input.items) {
  [handle]: chat(agent="processor", message=$item)
  [save]: set_context(last=$output)
  [handle] -> [save]
}

# For parallel execution, consider splitting into multiple independent workflows
```

### Memory Management

Avoid unbounded accumulation in large loops:

```juglans
# Good: periodic cleanup
[process]: foreach($item in $input.large_list) {
  [handle]: chat(agent="processor", message=$item)

  [collect]: set_context(
    batch_results=append($ctx.batch_results, $output)
  )

  # Save every 100 items and clear memory
  [flush]: set_context(
    batch_results=[],
    total_processed=$ctx.total_processed + 100
  )

  [handle] -> [collect]
  [collect] if $ctx.total_processed % 100 == 99 -> [flush]
}
```

## Best Practices

1. **Clear termination conditions** - While loops must have a termination condition
2. **Set maximum iterations** - Prevent accidental infinite loops
3. **Progress tracking** - Display progress in long-running loops
4. **Error handling** - Handle individual item errors within the loop body to avoid overall failure
5. **State updates** - Ensure loop variables are updated correctly
