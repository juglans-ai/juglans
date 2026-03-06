# Tutorial 4: Loops

本章学习两种循环结构：**foreach** 遍历列表，**while** 条件循环，让 workflow 重复执行一组操作。

## foreach 基础

遍历一个列表，对每个元素执行循环体：

```juglans
[init]: set_context(items=["apple", "banana", "cherry"])

[loop]: foreach($item in $ctx.items) {
  [show]: print(message="Fruit: " + $item)
}

[done]: print(message="All done")

[init] -> [loop] -> [done]
```

逐行解释：

1. `[init]` 将一个字符串数组存入 `$ctx.items`。
2. `[loop]` 是循环节点，`foreach($item in $ctx.items)` 表示：遍历 `$ctx.items`，每次迭代将当前元素绑定到 `$item`。
3. 花括号 `{ ... }` 内是**循环体**，每次迭代执行一遍。这里只有一个节点 `[show]`，打印当前水果名。
4. 循环结束后，沿边走到 `[done]`。

foreach 的语法：

```text
[节点名]: foreach($变量 in $集合) {
    循环体（节点 + 边）
}
```

`$item` 的 `$` 前缀可省略——`foreach(item in $ctx.items)` 同样合法。但建议保留 `$` 以保持变量引用的一致性。

## foreach 循环体内多个节点

循环体不限于单个节点。可以定义多个节点并用边连接：

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

循环体内的三个节点 `[start] -> [run] -> [finish]` 构成一条链，每次迭代按顺序执行。循环体本质上是一个嵌套的子图——节点名在循环体内部有自己的作用域，不会与外部节点冲突。

## foreach 与 $input

foreach 最常见的用法是遍历外部输入：

```juglans
[loop]: foreach($user in $input.users) {
  [greet]: print(message="Hello, " + $user)
}
```

运行：

```bash
juglans greet.jg --input '{"users": ["Alice", "Bob", "Charlie"]}'
```

## while 循环

当你不是遍历列表，而是需要"重复直到条件不满足"时，用 `while`：

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

逐行解释：

1. `[init]` 将计数器 `count` 初始化为 0。
2. `[loop]` 是 while 循环节点。每次迭代前检查条件 `$ctx.count < 5`，为真则执行循环体。
3. 循环体内，`[step]` 将 `count` 加 1，`[log]` 打印当前值。
4. 当 `$ctx.count` 达到 5 时，条件为假，循环结束，执行 `[done]`。

while 的语法：

```text
[节点名]: while(条件表达式) {
    循环体（节点 + 边）
}
```

循环体内**必须修改条件变量**，否则会产生无限循环。Juglans 内置了循环次数上限保护——超过上限会自动终止并报错。

### while 条件支持的表达式

while 的条件表达式和 `if` 条件边使用相同的语法，支持所有比较和逻辑运算符：

| 表达式 | 含义 |
|--------|------|
| `$ctx.count < 10` | 小于 |
| `$ctx.status != "done"` | 不等于 |
| `$ctx.active && $ctx.count < 100` | 逻辑与 |

## 循环中使用上下文

循环的核心能力之一是在迭代间**累积结果**。foreach 的循环变量每次迭代都会被覆盖，但 `$ctx` 在整个 workflow 中持久存在——利用这一点，可以在循环中收集数据：

```juglans
[init]: set_context(total=0, items=[10, 20, 30, 40])

[sum]: foreach($n in $ctx.items) {
  [add]: set_context(total=$ctx.total + $n)
}

[result]: print(message="Sum: " + str($ctx.total))

[init] -> [sum] -> [result]
```

执行过程：

| 迭代 | $n | $ctx.total (迭代后) |
|------|----|---------------------|
| 1    | 10 | 10                  |
| 2    | 20 | 30                  |
| 3    | 30 | 60                  |
| 4    | 40 | 100                 |

`[result]` 打印 `Sum: 100`。

同样的模式也适用于 while 循环。下面的例子在 while 循环中构建一个列表：

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

`append()` 函数将新元素追加到数组末尾，返回新数组。每次迭代将 `i` 的平方追加到 `squares` 中。

## 综合示例

一个数据处理管线：接收一批记录，过滤并汇总。

```juglans
name: "Batch Score Processor"
version: "0.1.0"

entry: [init]
exit: [report]

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

这个 workflow 展示了循环在实际场景中的典型用法：

1. **初始化** — 准备数据和累加器。
2. **foreach 遍历** — 逐条处理记录，更新上下文中的计数器。
3. **汇总输出** — 循环结束后读取累积的结果。

## 小结

- **foreach** `[node]: foreach($item in $list) { ... }` — 遍历列表，每个元素执行一次循环体
- **while** `[node]: while(condition) { ... }` — 条件为真时重复执行循环体
- 循环体是一个嵌套子图，内部可以有多个节点和边
- 用 `$ctx` 在迭代间累积数据（计数、求和、构建列表）
- while 循环体内必须修改条件变量，引擎有最大迭代次数保护
- `append()` 函数可向数组追加元素

下一章：[Tutorial 5: 错误处理](./error-handling.md) -- 学习 `on error` 边和错误恢复模式，让 workflow 优雅处理失败。
