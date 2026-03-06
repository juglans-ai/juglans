# How to Debug Workflows

本指南介绍 Juglans workflow 的调试与排错方法。

## juglans check -- Syntax Validation

`juglans check` 在不执行的情况下验证文件语法，类似 `cargo check`：

```bash
# 检查当前目录所有文件
juglans check

# 检查指定目录
juglans check ./src/

# 检查单个文件
juglans check src/main.jg

# 显示所有警告
juglans check --all

# JSON 格式输出（适合 CI）
juglans check --format json
```

exit code 0 表示验证通过，1 表示有语法错误。

## --verbose Mode

添加 `--verbose`（或 `-v`）查看详细执行日志：

```bash
juglans src/main.jg --verbose
```

输出包含：

```
[DEBUG] Loading workflow: main.jg
[DEBUG] Parsed 5 nodes, 4 edges
[INFO]  [init] Starting...
[DEBUG] [init] Output: null
[INFO]  [chat] Calling agent: assistant
[DEBUG] [chat] Request: {"message": "..."}
[INFO]  [chat] Response received (234 tokens)
```

也可以通过环境变量设置日志级别：

```bash
JUGLANS_LOG_LEVEL=debug juglans src/main.jg
```

## juglans doctest -- Validate Doc Code Blocks

`juglans doctest` 从 Markdown 文件中提取所有 ` ```juglans ` 代码块，通过 `GraphParser::parse()` 验证语法。

```bash
# 验证单个文件
juglans doctest docs/guide/concepts.md

# 验证整个目录
juglans doctest docs/

# JSON 格式输出
juglans doctest docs/ --format json
```

不需要 doctest 验证的代码块用 `ignore` 标记：

````text
```juglans,ignore
[broken]: this_is_intentionally_invalid
```
````

## Common Errors

| # | Error | Cause | Solution |
|---|-------|-------|----------|
| 1 | `Duplicate node ID: X` | 同一 workflow 中两个节点同名 | 重命名其中一个节点 |
| 2 | `Edge references undefined node: X` | 边引用了未定义的节点 | 检查节点名拼写，确保先定义节点再写边 |
| 3 | `Entry node 'X' not defined` | `entry:` 指定的节点不存在 | 修正 entry 节点名或添加节点定义 |
| 4 | `Connection refused` | Jug0 后端未启动 | 启动 Jug0 或检查 `base_url` 配置 |
| 5 | `Agent not found: X` | Agent slug 不存在 | 检查拼写，确保 `agents:` 导入了对应文件 |
| 6 | `Tool not found: X` | 调用了未注册的工具 | 检查工具名是否为内置工具，或 MCP 服务器是否已配置 |
| 7 | `Cycle detected` | 图中存在环 | 检查边定义，DAG 不允许环（用 while/foreach 代替） |
| 8 | `Parse error at line N` | DSL 语法错误 | 检查该行附近的语法：括号匹配、引号闭合、参数格式 |
| 9 | `Variable not found: $ctx.X` | 上下文变量未设置 | 确保在使用前通过 `set_context()` 设置了该变量 |
| 10 | `Timeout` | 工具执行超时 | 检查网络连接，增大超时配置 |

## Debugging Tips

### Insert Checkpoints with print()

在关键位置插入 `print()` 节点查看中间状态：

```juglans
[step1]: set_context(data=$input.items)
[debug1]: print(message="After step1, data = " + json($ctx.data))
[step2]: chat(agent="processor", message=json($ctx.data))
[debug2]: print(message="After step2, output = " + json($output))
[done]: notify(status="complete")

[step1] -> [debug1] -> [step2] -> [debug2] -> [done]
```

### Save Intermediate State with set_context()

用 `set_context()` 保存中间结果，方便后续节点或错误路径引用：

```juglans
[fetch]: fetch_url(url=$input.url)
[save_raw]: set_context(raw_response=$output)
[process]: chat(agent="parser", message=$output)
[save_parsed]: set_context(parsed=$output)
[handle_error]: print(message="Failed. Raw response: " + json($ctx.raw_response))
[done]: print(message="Result: " + json($ctx.parsed))

[fetch] -> [save_raw] -> [process] -> [save_parsed] -> [done]
[process] on error -> [handle_error]
```

### Dry-run Mode

用 `--dry-run` 只解析不执行，快速检查结构：

```bash
juglans src/main.jg --dry-run
```

### Isolate Problem Nodes

当 workflow 较长时，创建一个最小的测试文件单独测试问题节点：

```bash
# 单独测试一个 Agent
juglans src/agents/my-agent.jgagent --message "test input"

# 单独渲染一个 Prompt
juglans src/prompts/my-prompt.jgprompt --input '{"name": "Alice"}'
```

### Check Configuration

确认当前配置是否正确：

```bash
# 查看账户和配置信息
juglans whoami --verbose

# 测试 Jug0 连接
juglans whoami --check-connection
```

## Next Steps

- [Testing Workflows](./testing.md) -- 系统化测试方法
- [Error Handling](./error-handling.md) -- 在 workflow 中处理错误
- [CLI Reference](../reference/cli.md) -- 完整命令参考
