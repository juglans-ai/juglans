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
| `state` | string | 否 | 消息状态控制（见下表） |
| `stateless` | string | 否 | ⚠️ 已弃用，使用 `state="silent"` 替代 |
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

# 无状态调用（已弃用，使用 state 替代）
[analyze]: chat(
  agent="analyst",
  message=$input.data,
  stateless="true"
)

# 使用 state 参数
[hidden]: chat(
  agent="analyst",
  message=$input.data,
  state="context_hidden"
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

**`state` 参数说明：**

控制 `chat()` 输出的可见性和持久性：

| state | 写入上下文 | SSE 输出 | 说明 |
|-------|-----------|---------|------|
| `context_visible` | ✅ | ✅ | 默认值，正常消息 |
| `context_hidden` | ✅ | ❌ | AI 后续可见，不推送给用户 |
| `display_only` | ❌ | ✅ | 推送给用户，AI 后续不可见 |
| `silent` | ❌ | ❌ | 两者都不 |

- **写入上下文**: 结果是否存入 `$reply.output`，影响后续节点是否能读取
- **SSE 输出**: 生成的 token 是否通过 SSE 流式推送给前端

```yaml
# 后台分析，不显示给用户，但结果供后续节点使用
[bg_analyze]: chat(
  agent="analyst",
  message=$input.data,
  state="context_hidden"
)

# 展示给用户看，但不影响后续 AI 上下文
[greeting]: chat(
  agent="greeter",
  message="Welcome!",
  state="display_only"
)

# 完全静默
[silent_check]: chat(
  agent="validator",
  message=$input.data,
  state="silent"
)
```

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

### sh()

> ⚠️ **已弃用** — `sh()` 现在是 `bash()` 的别名，保持向后兼容。推荐使用 `bash()` 替代。参见[开发者工具 > bash()](#bash)。

**旧语法仍然有效：**

```yaml
[files]: sh(cmd="ls -la")    # 等同于 bash(command="ls -la")
```

---

## 开发者工具

Claude Code 风格的代码操作工具集，注册为 `"devtools"` slug。可在 .jgflow 中直接调用，也可通过 .jgagent 的 `tools: ["devtools"]` 被 LLM 自动使用。

```yaml
# Agent 中启用
slug: "code-agent"
tools: ["devtools"]

# 也可与其他工具集组合
tools: ["devtools", "web-tools"]
```

---

### read_file()

读取文件内容，返回带行号的格式。

**参数：**

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `file_path` | string | 是 | 文件路径（绝对或相对） |
| `offset` | integer | 否 | 起始行号，1-based（默认 1） |
| `limit` | integer | 否 | 最大返回行数（默认 2000） |

**示例：**

```yaml
# 读取整个文件
[read]: read_file(file_path="./src/main.rs")

# 读取指定范围
[read]: read_file(file_path="./src/main.rs", offset=50, limit=100)
```

**输出：**

```json
{
  "content": "     1\tuse std::io;\n     2\tfn main() {...",
  "total_lines": 150,
  "lines_returned": 100,
  "offset": 50
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `content` | string | 带行号的文件内容（cat -n 格式，单行最长 2000 字符） |
| `total_lines` | number | 文件总行数 |
| `lines_returned` | number | 实际返回行数 |
| `offset` | number | 起始行号 |

---

### write_file()

写入文件（覆盖），自动创建父目录。

**参数：**

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `file_path` | string | 是 | 文件路径 |
| `content` | string | 是 | 文件内容 |

**示例：**

```yaml
[write]: write_file(file_path="./output/result.json", content=$ctx.result)
```

**输出：**

```json
{
  "status": "ok",
  "file_path": "./output/result.json",
  "lines_written": 25,
  "bytes_written": 1024
}
```

---

### edit_file()

精确字符串替换。`old_string` 必须在文件中唯一，否则需要 `replace_all=true`。

**参数：**

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `file_path` | string | 是 | 文件路径 |
| `old_string` | string | 是 | 要替换的文本（必须唯一） |
| `new_string` | string | 是 | 替换后的文本 |
| `replace_all` | boolean | 否 | 替换所有匹配（默认 false） |

**示例：**

```yaml
# 精确替换
[edit]: edit_file(
  file_path="./src/config.rs",
  old_string="version = \"1.0\"",
  new_string="version = \"2.0\""
)

# 全局替换
[rename]: edit_file(
  file_path="./src/main.rs",
  old_string="old_name",
  new_string="new_name",
  replace_all="true"
)
```

**输出：**

```json
{
  "status": "ok",
  "file_path": "./src/config.rs",
  "replacements": 1
}
```

**错误情况：**
- `old_string` 未找到 → 报错
- `old_string` 出现多次且 `replace_all=false` → 报错（要求提供更多上下文使匹配唯一）

---

### glob()

文件模式匹配，返回匹配路径列表。

**参数：**

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `pattern` | string | 是 | Glob 模式（如 `**/*.rs`, `src/**/*.json`） |
| `path` | string | 否 | 搜索目录（默认当前目录） |

**示例：**

```yaml
[find]: glob(pattern="**/*.rs")
[find_src]: glob(pattern="*.ts", path="./src")
```

**输出：**

```json
{
  "matches": ["./src/main.rs", "./src/lib.rs"],
  "count": 2,
  "pattern": "./**/*.rs"
}
```

---

### grep()

正则搜索文件内容。递归搜索目录中的文件，返回匹配行和上下文。

**参数：**

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `pattern` | string | 是 | 正则表达式 |
| `path` | string | 否 | 搜索路径（文件或目录，默认当前目录） |
| `include` | string | 否 | 文件过滤 glob（如 `*.rs`, `*.{ts,tsx}`） |
| `context_lines` | integer | 否 | 匹配行前后的上下文行数（默认 0） |
| `max_matches` | integer | 否 | 最大匹配数（默认 50） |

**示例：**

```yaml
# 搜索 TODO
[todos]: grep(pattern="TODO|FIXME", path="./src")

# 搜索特定文件类型
[search]: grep(pattern="fn main", include="*.rs", context_lines=2)
```

**输出：**

```json
{
  "matches": [
    {
      "file": "./src/main.rs",
      "line": 10,
      "match": "fn main() {",
      "context": "     9\tuse std::io;\n    10\tfn main() {\n    11\t    println!(\"hello\");"
    }
  ],
  "total_matches": 1,
  "files_searched": 15,
  "truncated": false
}
```

---

### bash()

执行 Shell 命令，带超时控制和输出截断。替代旧 `sh()` 工具。

**参数：**

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `command` | string | 是 | 要执行的命令（也接受 `cmd` 参数，向后兼容） |
| `timeout` | integer | 否 | 超时毫秒（默认 120000，最大 600000） |
| `description` | string | 否 | 命令描述（用于日志） |

**示例：**

```yaml
# 执行命令
[build]: bash(command="cargo build --release")

# 带超时
[test]: bash(command="cargo test", timeout=300000)

# 向后兼容旧语法
[files]: bash(cmd="ls -la")
```

**输出：**

```json
{
  "stdout": "命令标准输出...",
  "stderr": "错误输出（如有）...",
  "exit_code": 0,
  "ok": true
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `stdout` | string | 标准输出（超过 30000 字符会截断） |
| `stderr` | string | 标准错误输出 |
| `exit_code` | number | 退出码（0 表示成功） |
| `ok` | boolean | 命令是否成功执行 |

**安全提示：**

避免直接执行用户输入的命令，防止命令注入攻击：

```yaml
# 危险：不要这样做
[bad]: bash(command=$input.user_command)

# 安全：使用固定命令，参数验证
[safe]: bash(command="ls " + sanitize($input.directory))
```

> **注意**：`sh()` 是 `bash()` 的别名，`sh(cmd="ls")` 等同于 `bash(command="ls")`。

---

## 网络工具

### fetch()

HTTP 请求工具（推荐使用）。

**参数：**

| 参数 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `url` | string | 是 | 目标 URL |
| `method` | string | 否 | HTTP 方法 (默认 "GET") |
| `body` | object/string | 否 | 请求体（自动 JSON 序列化） |
| `headers` | object | 否 | 自定义请求头 |

**示例：**

```yaml
# GET 请求
[get]: fetch(url="https://api.example.com/data")

# POST 请求
[post]: fetch(
  url="https://api.example.com/submit",
  method="POST",
  body=$input.data
)

# 带请求头
[auth_get]: fetch(
  url="https://api.example.com/protected",
  headers={"Authorization": "Bearer " + $ctx.token}
)

# PUT 请求
[update]: fetch(
  url="https://api.example.com/items/1",
  method="PUT",
  body={"name": "updated", "value": $input.value}
)
```

**输出：**

```json
{
  "status": 200,
  "ok": true,
  "data": { ... }
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `status` | number | HTTP 状态码 |
| `ok` | boolean | 状态码在 200-299 范围内为 true |
| `data` | any | 响应内容（自动解析 JSON，否则返回字符串） |

**错误处理：**

```yaml
[api]: fetch(url=$input.api_url)
[api] -> [process]
[api] on error -> [handle_error]

[handle_error]: notify(message="API 请求失败: " + $error.message)
```

---

### fetch_url()

获取 URL 内容（兼容旧版，推荐使用 `fetch()`）。

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
