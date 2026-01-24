# 循环结构

本指南介绍 Juglans 工作流中的循环机制。

## 循环类型

Juglans 支持两种循环结构：

| 类型 | 用途 | 语法 |
|------|------|------|
| `foreach` | 遍历集合 | `foreach($item in $collection)` |
| `while` | 条件循环 | `while($condition)` |

## Foreach 循环

遍历数组或集合中的每个元素。

### 基本语法

```yaml
[loop_node]: foreach($item in $input.items) {
  [process]: some_tool(data=$item)
  [process]
}
```

### 完整示例

```yaml
name: "Batch Processing"

entry: [init]
exit: [summary]

[init]: set_context(results=[])

[process_all]: foreach($item in $input.documents) {
  # 处理每个文档
  [analyze]: chat(
    agent="analyzer",
    message="Analyze: " + $item.content,
    format="json"
  )

  # 收集结果
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

### 循环变量

在循环体内可用：

| 变量 | 类型 | 说明 |
|------|------|------|
| `$item` | any | 当前元素（名称可自定义） |
| `loop.index` | number | 当前索引 (1-based，从 1 开始) |
| `loop.index0` | number | 当前索引 (0-based，从 0 开始) |
| `loop.first` | boolean | 是否第一次迭代 |
| `loop.last` | boolean | 是否最后一次迭代 |

```yaml
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

### 嵌套循环

```yaml
[outer]: foreach($category in $input.categories) {
  [inner]: foreach($item in $category.items) {
    [process]: chat(
      agent="handler",
      message=$category.name + ": " + $item.name
    )
    [process]
  }
  [inner]
}
```

## While 循环

基于条件的循环。

### 基本语法

```yaml
[loop_node]: while($ctx.count < $ctx.max) {
  [body]: some_tool(...)
  [update]: set_context(count=$ctx.count + 1)
  [body] -> [update]
}
```

### 完整示例

```yaml
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
  # 评估质量
  [evaluate]: chat(
    agent="critic",
    message="Rate this (1-10): " + $ctx.content,
    format="json"
  )

  # 更新状态
  [update_state]: set_context(
    current_quality=$output.score,
    feedback=$output.feedback,
    iteration=$ctx.iteration + 1
  )

  # 如果质量不够，改进
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

### 避免无限循环

始终确保循环条件会变为 false：

```yaml
# 好：有明确的终止条件
[loop]: while($ctx.count < 10) {
  [inc]: set_context(count=$ctx.count + 1)
  [inc]
}

# 好：有最大迭代限制
[loop]: while($ctx.not_done && $ctx.attempts < 100) {
  [try]: some_operation()
  [update]: set_context(
    not_done=!$output.success,
    attempts=$ctx.attempts + 1
  )
  [try] -> [update]
}
```

## 常见模式

### 批量 API 调用

```yaml
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

  [call] -> [save_result]
  [call] on error -> [save_error]

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
}

[done]: notify(
  status="Completed: " + len($ctx.results) + " success, " + len($ctx.errors) + " errors"
)

[init] -> [batch] -> [done]
```

### 分页获取

```yaml
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

### 递归处理

```yaml
name: "Tree Processing"

entry: [process_root]
exit: [done]

[process_root]: set_context(
  queue=$input.nodes,
  processed=[]
)

[process_queue]: while(len($ctx.queue) > 0) {
  # 取出第一个
  [dequeue]: set_context(
    current=first($ctx.queue),
    queue=rest($ctx.queue)
  )

  # 处理当前节点
  [handle]: chat(
    agent="processor",
    message="Process: " + json($ctx.current)
  )

  # 保存结果，添加子节点到队列
  [update]: set_context(
    processed=append($ctx.processed, {
      "node": $ctx.current,
      "result": $output
    }),
    queue=concat($ctx.queue, $ctx.current.children)
  )

  [dequeue] -> [handle] -> [update]
}

[done]: notify(status="Processed " + len($ctx.processed) + " nodes")

[process_root] -> [process_queue] -> [done]
```

### 收敛迭代

```yaml
name: "Convergence Loop"

entry: [init]
exit: [converged]

[init]: set_context(
  value=0,
  prev_value=-999,
  tolerance=0.01,
  iteration=0
)

[iterate]: while(abs($ctx.value - $ctx.prev_value) > $ctx.tolerance && $ctx.iteration < 1000) {
  [compute]: chat(
    agent="calculator",
    message="Next iteration from: " + $ctx.value,
    format="json"
  )

  [update]: set_context(
    prev_value=$ctx.value,
    value=$output.result,
    iteration=$ctx.iteration + 1
  )

  [log]: notify(
    status="Iteration " + $ctx.iteration +
           ": " + $ctx.value +
           " (delta: " + abs($ctx.value - $ctx.prev_value) + ")"
  )

  [compute] -> [update] -> [log]
}

[converged]: notify(
  status="Converged to " + $ctx.value +
         " after " + $ctx.iteration + " iterations"
)

[init] -> [iterate] -> [converged]
```

### 带进度的批处理

```yaml
name: "Progress Tracking"

entry: [init]
exit: [complete]

[init]: set_context(
  total=len($input.items),
  completed=0,
  results=[]
)

[process]: foreach($item in $input.items) {
  # 显示进度
  [progress]: notify(
    status="Processing " + ($ctx.completed + 1) + "/" + $ctx.total +
           " (" + round(($ctx.completed / $ctx.total) * 100) + "%)"
  )

  # 处理
  [handle]: chat(agent="processor", message=$item)

  # 更新计数
  [update]: set_context(
    completed=$ctx.completed + 1,
    results=append($ctx.results, $output)
  )

  [progress] -> [handle] -> [update]
}

[complete]: notify(status="Completed all " + $ctx.total + " items")

[init] -> [process] -> [complete]
```

## 性能考虑

### 并行 vs 串行

默认循环是串行的。对于独立操作，考虑：

```yaml
# 串行（默认）- 适合有依赖的操作
[process]: foreach($item in $input.items) {
  [handle]: chat(agent="processor", message=$item)
  [handle]
}

# 如需并行，考虑拆分为多个独立工作流
```

### 内存管理

大循环中避免无限累积：

```yaml
# 好：定期清理
[process]: foreach($item in $input.large_list) {
  [handle]: chat(agent="processor", message=$item)

  # 每 100 条保存一次，清理内存
  [maybe_flush] if loop.index % 100 == 99 -> [flush]
  [handle] -> [maybe_flush]

  [flush]: set_context(
    batch_results=[],  # 清空
    total_processed=$ctx.total_processed + 100
  )
}
```

## 最佳实践

1. **明确终止条件** - while 循环必须有终止条件
2. **设置最大迭代** - 防止意外的无限循环
3. **进度追踪** - 长循环中显示进度
4. **错误处理** - 循环体内处理单项错误，避免整体失败
5. **状态更新** - 确保循环变量正确更新
