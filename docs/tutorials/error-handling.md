# Tutorial 5: Error Handling

本章学习如何让 workflow 优雅地处理失败：**on error 边**将错误路径显式化，**$error 变量**让你在 fallback 节点中获取错误详情。

## 没有错误处理时

先看一个会失败的 workflow——读取一个不存在的文件：

```juglans
[read]: read_file(file_path="/nonexistent/file.txt")
[show]: print(message="Content: " + $output)
[read] -> [show]
```

`[read]` 执行时会报错（文件不存在），整个 workflow 终止，`[show]` 永远不会执行。控制台输出类似：

```text
Error: Node [read] failed: No such file or directory
```

这在很多场景下是不可接受的。比如你调用外部 API、读取用户文件、执行 AI 推理——任何一步都可能失败，但你希望 workflow 能继续运行，走一条备用路径。

## on error 基础

`on error` 是边的修饰符。当源节点执行出错时，不终止 workflow，而是跳转到指定的 fallback 节点。

```juglans
[read]: read_file(file_path="/nonexistent/file.txt")
[fallback]: print(message="File not found, using default")
[show]: print(message="Reached the end")

[read] -> [show]
[read] on error -> [fallback]
[fallback] -> [show]
```

逐行解释：

1. `[read]` 尝试读取文件。如果成功，走 `[read] -> [show]`。
2. 如果失败，`on error` 边生效，跳转到 `[fallback]`。
3. `[fallback]` 输出提示信息，然后走 `[fallback] -> [show]`。
4. 无论成功还是失败，最终都到达 `[show]`。

语法：

```text
[源节点] on error -> [fallback 节点]
```

规则：

- `on error` 边**只在源节点执行出错时**才走。正常情况下它被忽略。
- 一个节点可以同时拥有正常边和 `on error` 边。正常边在成功时走，`on error` 边在失败时走。
- 有了 `on error` 边的节点，出错后**不会**终止 workflow。

### 成功路径 vs 错误路径

用 `set_context` 来标记走了哪条路径：

```juglans
[step]: read_file(file_path="/nonexistent/file.txt")
[ok]: set_context(status="success")
[err]: set_context(status="failed")
[report]: print(message="Status: " + $ctx.status)

[step] -> [ok]
[step] on error -> [err]
[ok] -> [report]
[err] -> [report]
```

运行时 `[step]` 会失败，所以 `[err]` 执行，`$ctx.status` 被设为 `"failed"`，最终输出：

```text
Status: failed
```

如果把 `file_path` 改成一个存在的文件，`[ok]` 执行，输出 `Status: success`。两条路径，一个出口——这是错误处理的基本模式。

## $error 变量

当 `on error` 触发时，引擎自动设置 `$error` 变量，包含两个字段：

| 字段 | 类型 | 内容 |
|------|------|------|
| `$error.node` | string | 出错节点的 ID |
| `$error.message` | string | 错误信息 |

在 fallback 节点中读取它：

```juglans
[read]: read_file(file_path="/nonexistent/file.txt")
[handle]: print(message="Error in [" + $error.node + "]: " + $error.message)

[read] on error -> [handle]
```

输出类似：

```text
Error in [read]: No such file or directory
```

### $node_id.error

除了全局 `$error`，每个出错节点的错误信息也会存储在 `$node_id.error` 中：

```juglans
[read]: read_file(file_path="/nonexistent/file.txt")
[handle]: print(message="read node error: " + $read.error)

[read] on error -> [handle]
```

`$error` 是全局的，始终指向最近一次出错的节点。`$node_id.error` 是节点级的，在多节点错误处理场景中更精确。

## 多层错误处理

不同节点可以有不同的 fallback：

```juglans
[load_config]: read_file(file_path="/etc/app/config.json")
[load_data]: read_file(file_path="/tmp/data.csv")
[process]: print(message="Processing data...")
[config_fallback]: set_context(config="default")
[data_fallback]: set_context(data="empty")
[done]: print(message="Workflow complete")

[load_config] -> [load_data]
[load_config] on error -> [config_fallback]
[config_fallback] -> [load_data]

[load_data] -> [process]
[load_data] on error -> [data_fallback]
[data_fallback] -> [process]

[process] -> [done]
```

执行流程：

1. `[load_config]` 尝试读取配置。失败则跳到 `[config_fallback]`，设置默认值，然后继续到 `[load_data]`。
2. `[load_data]` 尝试读取数据。失败则跳到 `[data_fallback]`，设置空数据，然后继续到 `[process]`。
3. 无论哪一步失败，workflow 都能走到 `[done]`。

每个"可能失败"的节点有自己的 fallback，互不干扰。

## 错误处理 + 条件分支

`on error` 可以和条件边组合使用：

```juglans
[init]: set_context(mode="strict")
[work]: read_file(file_path="/tmp/important.txt")
[ok]: print(message="File loaded")
[warn]: print(message="File missing, but mode is lenient")
[abort]: print(message="File missing in strict mode, aborting")
[done]: print(message="Done")

[init] -> [work]
[work] -> [ok]
[work] on error -> [warn]
[work] on error -> [abort]
[ok] -> [done]
[warn] -> [done]
[abort] -> [done]
```

这里对同一个节点定义了两条 `on error` 边。当 `[work]` 出错时，引擎按定义顺序选择第一条可达的 `on error` 边。

如果你需要根据上下文区分错误处理策略，更好的做法是让 fallback 节点内部做判断：

```juglans
[init]: set_context(mode="strict")
[work]: read_file(file_path="/tmp/important.txt")
[ok]: print(message="File loaded")
[error_router]: print(message="Handling error...")
[warn]: print(message="File missing, lenient mode")
[abort]: print(message="Strict mode, abort!")
[done]: print(message="Done")

[init] -> [work]
[work] -> [ok]
[work] on error -> [error_router]

[error_router] if $ctx.mode == "strict" -> [abort]
[error_router] -> [warn]

[ok] -> [done]
[warn] -> [done]
[abort] -> [done]
```

`on error` 跳到 `[error_router]`，再根据 `$ctx.mode` 做条件分支——错误处理和路由逻辑各司其职。

## 综合示例

一个包含正常路径和多个错误处理的完整 workflow：

```juglans
name: "Resilient Data Pipeline"
version: "0.1.0"

entry: [start]
exit: [report]

[start]: set_context(errors=0)

# Step 1: 加载配置
[load_config]: read_file(file_path="/etc/app/config.json")
[config_ok]: set_context(config_loaded=true)
[config_err]: set_context(
  config_loaded=false,
  errors=$ctx.errors + 1
)

# Step 2: 加载数据
[load_data]: read_file(file_path="/tmp/dataset.csv")
[data_ok]: set_context(data_loaded=true)
[data_err]: set_context(
  data_loaded=false,
  errors=$ctx.errors + 1
)

# Step 3: 汇总
[report]: print(
  message="Pipeline done. Errors: " + str($ctx.errors)
)

# 正常路径
[start] -> [load_config]
[load_config] -> [config_ok]
[config_ok] -> [load_data]

# 配置加载失败
[load_config] on error -> [config_err]
[config_err] -> [load_data]

# 数据加载
[load_data] -> [data_ok]
[data_ok] -> [report]

# 数据加载失败
[load_data] on error -> [data_err]
[data_err] -> [report]
```

这个 workflow 展示了一个常见的"尽力而为"模式：

1. 每个可能失败的步骤都有 `on error` 边。
2. Fallback 节点记录错误（递增计数器），然后让 workflow 继续。
3. 最终的 `[report]` 节点汇总错误数量。
4. 无论中间哪一步失败，workflow 始终能走到终点。

## 小结

- `[node] on error -> [fallback]` -- 节点出错时跳转到 fallback，不终止 workflow
- `$error.node` 和 `$error.message` -- 全局错误变量，包含最近一次错误的节点 ID 和错误信息
- `$node_id.error` -- 节点级错误信息，适合多节点错误处理
- 每个可能失败的节点可以有独立的 fallback
- `on error` 可以和条件边、switch 组合使用
- 常见模式：fallback 节点设置默认值或记录错误，然后汇入正常流程

下一章：[Tutorial 6: AI Chat](./ai-chat.md) -- 学习在 workflow 中调用 AI 模型，构造多轮对话和结构化输出。
