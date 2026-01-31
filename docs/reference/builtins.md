# 内置工具参考

Juglans 提供多个内置工具，用于工作流中的各种操作。

## AI 工具

### chat()

与 AI Agent 进行对话。

**参数：**

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `agent` | string | 是 | Agent 的 slug |
| `message` | string | 是 | 发送的消息 |
| `format` | string | 否 | 输出格式 ("text", "json") |
| `stateless` | string | 否 | "true" 则不保存历史 |
| `chat_id` | string | 否 | 对话 ID，用于复用会话上下文 |
| `tools` | array | 否 | 自定义工具定义（覆盖 Agent 默认配置） |

**示例：**

```yaml
# 基本对话
[chat]: chat(agent="assistant", message="Hello!")

# 使用变量
[chat]: chat(agent="assistant", message=$input.question)

# JSON 输出
[classify]: chat(
  agent="classifier",
  message=$input.text,
  format="json"
)

# 无状态调用
[analyze]: chat(
  agent="analyst",
  message=$input.data,
  stateless="true"
)

# 复用对话上下文
[reply]: chat(
  agent="assistant",
  chat_id=$reply.chat_id,
  message=$input.followup
)

# 附加工具
[solver]: chat(
  agent="assistant",
  message=$input.question,
  tools=[
    {
      "type": "function",
      "function": {
        "name": "search_web",
        "description": "搜索互联网内容",
        "parameters": {
          "type": "object",
          "properties": {
            "query": {"type": "string", "description": "搜索关键词"}
          },
          "required": ["query"]
        }
      }
    }
  ]
)
```

**输出：**

返回 AI 的响应文本。如果 `format="json"`，返回解析后的 JSON 对象。

**工具配置说明：**

- 如果工作流中指定了 `tools` 参数，使用工作流中的配置
- 否则，如果 Agent 配置中有 `tools` 字段，使用 Agent 的默认工具
- 工具配置遵循 OpenAI Function Calling 格式

---

### p()

渲染 Prompt 模板。

**参数：**

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `slug` | string | 是 | Prompt 的 slug |
| `...` | any | 否 | 模板变量 |

**示例：**

```yaml
# 基本渲染
[prompt]: p(slug="greeting")

# 传递变量
[prompt]: p(slug="greeting", name="Alice", language="Chinese")

# 使用输入变量
[prompt]: p(
  slug="analysis",
  topic=$input.topic,
  data=$ctx.collected_data
)
```

**输出：**

返回渲染后的 Prompt 文本。

---

### memory_search()

在记忆存储中搜索相关内容（RAG）。

**参数：**

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `query` | string | 是 | 搜索查询 |
| `limit` | number | 否 | 返回数量限制 |
| `threshold` | number | 否 | 相似度阈值 |

**示例：**

```yaml
[search]: memory_search(
  query=$input.question,
  limit=5,
  threshold=0.7
)
```

**输出：**

返回匹配的记忆条目数组。

---

## 系统工具

### notify()

发送状态通知。

**参数：**

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `status` | string | 是 | 通知消息 |

**示例：**

```yaml
[start]: notify(status="Starting workflow...")
[progress]: notify(status="Processing item " + $ctx.index)
[done]: notify(status="Completed!")
```

**输出：**

无返回值。消息会显示在控制台或 UI 中。

---

### set_context()

设置上下文变量。

**参数：**

任意键值对，支持嵌套路径。

**示例：**

```yaml
# 简单设置
[init]: set_context(count=0)

# 多个变量
[setup]: set_context(
  status="running",
  items=[],
  config={"timeout": 30}
)

# 嵌套路径
[update]: set_context(user.name="Alice", user.score=100)

# 使用表达式
[increment]: set_context(count=$ctx.count + 1)

# 追加到数组
[collect]: set_context(
  results=append($ctx.results, $output)
)
```

**输出：**

无返回值。变量可通过 `$ctx.*` 访问。

---

### timer()

延迟执行。

**参数：**

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `ms` | number | 是 | 延迟毫秒数 |

**示例：**

```yaml
# 等待 1 秒
[wait]: timer(ms=1000)

# 动态延迟
[delay]: timer(ms=$ctx.delay_time)
```

**输出：**

无返回值。执行会暂停指定时间。

---

## 网络工具

### fetch_url()

获取 URL 内容。

**参数：**

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `url` | string | 是 | 目标 URL |
| `method` | string | 否 | HTTP 方法 (默认 "GET") |
| `headers` | object | 否 | 请求头 |
| `body` | string | 否 | 请求体 |

**示例：**

```yaml
# GET 请求
[fetch]: fetch_url(url="https://api.example.com/data")

# POST 请求
[post]: fetch_url(
  url="https://api.example.com/submit",
  method="POST",
  headers={"Content-Type": "application/json"},
  body=json($input.data)
)

# 带认证
[api]: fetch_url(
  url="https://api.example.com/protected",
  headers={"Authorization": "Bearer " + $ctx.token}
)
```

**输出：**

返回响应内容。如果是 JSON，自动解析为对象。

---

## 工具函数

在参数中可以使用以下函数：

### 数据转换

```yaml
# JSON 序列化
json($ctx.data)              # 对象 -> JSON 字符串

# 字符串拼接
"Hello, " + $input.name      # 拼接字符串

# 数组追加
append($ctx.list, $item)     # 追加元素到数组
```

### 数学运算

```yaml
$ctx.count + 1               # 加法
$ctx.total - $ctx.used       # 减法
$ctx.price * $ctx.quantity   # 乘法
$ctx.total / $ctx.count      # 除法
```

### 比较运算

```yaml
$ctx.score > 80              # 大于
$ctx.count <= 10             # 小于等于
$ctx.status == "done"        # 等于
$ctx.value != null           # 不等于
```

### 逻辑运算

```yaml
$ctx.a && $ctx.b             # AND
$ctx.a || $ctx.b             # OR
!$ctx.flag                   # NOT
```

---

## 在工作流中组合使用

### 完整示例

```yaml
name: "Data Processing"
version: "0.1.0"

prompts: ["./prompts/*.jgprompt"]
agents: ["./agents/*.jgagent"]

entry: [init]
exit: [done]

# 初始化
[init]: set_context(results=[], processed=0)
[start_notify]: notify(status="Starting data processing...")

# 获取数据
[fetch_data]: fetch_url(url=$input.data_url)

# 处理每条数据
[process]: foreach($item in $output.items) {
  # 渲染分析 Prompt
  [render]: p(slug="analyze-item", item=$item)

  # 调用 AI 分析
  [analyze]: chat(
    agent="analyst",
    message=$output,
    format="json"
  )

  # 收集结果
  [collect]: set_context(
    results=append($ctx.results, $output),
    processed=$ctx.processed + 1
  )

  # 进度通知
  [progress]: notify(status="Processed: " + $ctx.processed)

  [render] -> [analyze] -> [collect] -> [progress]
}

# 汇总
[summarize]: chat(
  agent="summarizer",
  message="Summarize: " + json($ctx.results)
)

# 完成
[done]: notify(status="Done! Processed " + $ctx.processed + " items")

# 执行流程
[init] -> [start_notify] -> [fetch_data] -> [process] -> [summarize] -> [done]
```

---

## 自定义工具

通过 MCP 集成可以添加自定义工具。参见 [MCP 集成指南](../integrations/mcp.md)。
