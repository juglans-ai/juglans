# 上下文变量参考

工作流执行时维护一个上下文对象，用于存储和传递数据。

## 变量类型

| 前缀 | 说明 | 可写 |
|------|------|------|
| `$input` | 工作流输入 | 否 |
| `$output` | 当前节点输出 | 否 |
| `$ctx` | 自定义上下文 | 是 |
| `$reply` | AI 回复元数据 | 否 |
| `$alias.node.field` | 子工作流节点输出（由 `flows:` 导入产生） | 否 |

## $input - 输入变量

工作流启动时传入的数据。

### 来源

```bash
# CLI
juglans workflow.jgflow --input '{"query": "hello", "count": 5}'

# API
POST /api/workflows/my-flow/execute
{"query": "hello", "count": 5}
```

### 访问

```yaml
$input              # 整个输入对象
$input.query        # 字符串: "hello"
$input.count        # 数字: 5
$input.nested.field # 嵌套访问
```

### 示例

```yaml
[process]: chat(
  agent="assistant",
  message=$input.query
)

[loop]: foreach($item in $input.items) {
  [handle]: chat(message=$item.content)
}
```

---

## $output - 节点输出

最近一个节点的执行结果。

### 特点

- 每个节点执行后更新
- 仅在当前执行链中有效
- 类型取决于节点返回值

### 不同工具的输出

| 工具 | 输出类型 |
|------|----------|
| `chat()` | string 或 object (format="json") |
| `p()` | string |
| `notify()` | null |
| `set_context()` | null |
| `fetch_url()` | string 或 object |
| `timer()` | null |

### 示例

```yaml
# 字符串输出
[ask]: chat(agent="assistant", message="Hello")
[log]: notify(status="Response: " + $output)

# JSON 输出
[classify]: chat(agent="classifier", format="json")
[route]: notify(status="Category: " + $output.category)

# 链式使用
[render]: p(slug="template", data=$input)
[process]: chat(agent="processor", message=$output)
[save]: set_context(result=$output)
```

---

## $ctx - 自定义上下文

用户定义的变量存储，通过 `set_context()` 设置。

### 设置变量

```yaml
# 简单值
[init]: set_context(count=0, status="ready")

# 对象
[init]: set_context(config={"timeout": 30, "retries": 3})

# 数组
[init]: set_context(results=[], history=[])

# 嵌套路径
[update]: set_context(user.name="Alice", user.score=100)
```

### 读取变量

```yaml
$ctx.count           # 数字
$ctx.status          # 字符串
$ctx.config.timeout  # 嵌套访问
$ctx.results         # 数组
$ctx.user.name       # 嵌套对象
```

### 更新变量

```yaml
# 递增
[inc]: set_context(count=$ctx.count + 1)

# 追加到数组
[add]: set_context(results=append($ctx.results, $output))

# 条件更新
[update]: set_context(
  status=if($ctx.count > 10, "complete", "running")
)
```

### 作用域

`$ctx` 在整个工作流执行期间持久存在：

```yaml
[init]: set_context(total=0)
[step1]: set_context(total=$ctx.total + 10)  # total=10
[step2]: set_context(total=$ctx.total + 20)  # total=30
[final]: notify(status="Total: " + $ctx.total)  # "Total: 30"
```

---

## $reply - 回复元数据

AI 回复的元数据信息。

### 可用字段

| 字段 | 类型 | 说明 |
|------|------|------|
| `$reply.content` | string | 回复内容 |
| `$reply.tokens` | number | 使用的 token 数 |
| `$reply.model` | string | 使用的模型 |
| `$reply.finish_reason` | string | 结束原因 |

### 示例

```yaml
[ask]: chat(agent="assistant", message=$input.query)
[log]: notify(status="Used " + $reply.tokens + " tokens")
[save]: set_context(
  last_response=$reply.content,
  token_count=$reply.tokens
)
```

---

## 命名空间变量（Flow Imports）

当使用 `flows:` 导入子工作流时，子工作流内部的节点引用变量会自动添加命名空间前缀。

### 转换规则

只有第一段匹配子工作流内部节点 ID 的变量才会加前缀。全局变量（`$ctx`、`$input`、`$output`）不受影响：

```yaml
# 假设 auth 子工作流内有 verify、extract 两个节点

# 子工作流内部写法           →  合并后实际变量
$verify.output              →  $auth.verify.output
$extract.output.intent      →  $auth.extract.output.intent
$ctx.some_var               →  $ctx.some_var          # 不变
$input.message              →  $input.message          # 不变
$output                     →  $output                 # 不变
```

### 在父工作流中使用

```yaml
flows: {
  auth: "./workflows/auth.jgflow"
}

# 通过命名空间路径访问子工作流节点输出
[next]: chat(message=$auth.verify.output)

# 条件中使用
[check] if $auth.extract.output.intent == "trade" -> [trade]
```

详见[工作流组合指南](../guide/workflow-composition.md)。

---

## 循环上下文

在 `foreach` 和 `while` 循环中可用：

| 变量 | 类型 | 说明 |
|------|------|------|
| `loop.index` | number | 当前索引 (0-based) |
| `loop.first` | boolean | 是否第一次迭代 |
| `loop.last` | boolean | 是否最后一次迭代 |

### 示例

```yaml
[process]: foreach($item in $input.items) {
  [log]: notify(
    status="Processing " + (loop.index + 1) + "/" + len($input.items)
  )

  {% if loop.first %}
  [init]: set_context(results=[])
  {% endif %}

  [handle]: chat(agent="processor", message=$item)
  [collect]: set_context(results=append($ctx.results, $output))

  {% if loop.last %}
  [summary]: notify(status="All items processed!")
  {% endif %}
}
```

---

## 表达式语法

### 算术运算

```yaml
$ctx.a + $ctx.b      # 加法
$ctx.a - $ctx.b      # 减法
$ctx.a * $ctx.b      # 乘法
$ctx.a / $ctx.b      # 除法
$ctx.a % $ctx.b      # 取模
```

### 比较运算

```yaml
$ctx.a == $ctx.b     # 等于
$ctx.a != $ctx.b     # 不等于
$ctx.a > $ctx.b      # 大于
$ctx.a < $ctx.b      # 小于
$ctx.a >= $ctx.b     # 大于等于
$ctx.a <= $ctx.b     # 小于等于
```

### 逻辑运算

```yaml
$ctx.a && $ctx.b     # AND
$ctx.a || $ctx.b     # OR
!$ctx.a              # NOT
```

### 字符串操作

```yaml
"Hello, " + $input.name              # 拼接
$input.text + " (length: " + len($input.text) + ")"
```

### 内置函数

| 函数 | 说明 | 示例 |
|------|------|------|
| `len(x)` | 长度 | `len($ctx.items)` |
| `json(x)` | 转 JSON | `json($ctx.data)` |
| `append(arr, item)` | 追加 | `append($ctx.list, $output)` |
| `if(cond, a, b)` | 条件 | `if($ctx.ok, "yes", "no")` |

---

## 完整示例

```yaml
name: "Context Demo"
version: "0.1.0"

entry: [init]
exit: [summary]

# 初始化上下文
[init]: set_context(
  processed=0,
  successes=0,
  failures=0,
  results=[]
)

# 处理输入项
[process]: foreach($item in $input.items) {
  [log_start]: notify(
    status="[" + (loop.index + 1) + "/" + len($input.items) + "] Processing: " + $item.name
  )

  [analyze]: chat(
    agent="analyzer",
    message=$item.content,
    format="json"
  )

  # 根据结果更新计数
  [update]: set_context(
    processed=$ctx.processed + 1,
    successes=$ctx.successes + if($output.success, 1, 0),
    failures=$ctx.failures + if(!$output.success, 1, 0),
    results=append($ctx.results, {
      "name": $item.name,
      "result": $output
    })
  )

  [log_start] -> [analyze] -> [update]
}

# 汇总
[summary]: notify(
  status="Complete! Processed: " + $ctx.processed +
         ", Successes: " + $ctx.successes +
         ", Failures: " + $ctx.failures
)

[init] -> [process] -> [summary]
```

---

## 调试技巧

### 打印上下文

```yaml
[debug]: notify(status="Context: " + json($ctx))
```

### 检查变量类型

```yaml
[check]: notify(status="Type: " + type($ctx.value))
```

### 条件断点

```yaml
[breakpoint] if $ctx.count > 100 -> [error_handler]
```