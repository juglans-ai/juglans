# How to Test Workflows

本指南介绍如何验证和测试 Juglans workflow。

## juglans check -- Static Validation

`juglans check` 对 `.jg`、`.jgagent`、`.jgprompt` 文件进行静态语法验证，不执行任何工具调用：

```bash
# 检查当前目录所有文件
juglans check

# 检查指定目录
juglans check ./src/

# 检查单个文件
juglans check src/main.jg

# 显示所有警告
juglans check --all

# JSON 输出（适合程序解析）
juglans check --format json
```

检查内容：

- 语法正确性（节点定义、边定义、metadata）
- 节点引用一致性（边引用的节点必须已定义）
- Entry/exit 节点是否存在
- 无环检测

## juglans test -- Automated Testing

`juglans test` 提供 AI workflow 的自动化测试能力。详细设计参见 [juglans test 设计文档](./juglans-test.md)。

核心能力：

- **节点级测试** -- 单独测试某个节点，自动 mock 依赖
- **语义断言** -- 用 AI 判断输出质量（而非字符串精确匹配）
- **快照回归** -- 记录每次执行结果，自动检测变化

## Manual Testing

手动运行 workflow 并传入输入数据：

```bash
# 基本执行
juglans src/main.jg

# 传入 JSON 输入
juglans src/main.jg --input '{"query": "Hello"}'

# 从文件读取输入
juglans src/main.jg --input-file input.json

# 详细模式（查看每个节点的输入输出）
juglans src/main.jg --input '{"query": "test"}' --verbose

# 只解析不执行（验证结构）
juglans src/main.jg --dry-run

# JSON 格式输出（方便程序处理）
juglans src/main.jg --output-format json
```

单独测试各类资源：

```bash
# 测试 Agent（交互模式）
juglans src/agents/assistant.jgagent

# 测试 Agent（单条消息）
juglans src/agents/assistant.jgagent --message "What is Rust?"

# 测试 Prompt（渲染模板）
juglans src/prompts/greeting.jgprompt --input '{"name": "Alice"}'
```

## doctest -- Validate Documentation

`juglans doctest` 从 Markdown 文件中提取 ` ```juglans ` 代码块，验证其语法：

```bash
# 验证单个文件
juglans doctest docs/guide/concepts.md

# 验证整个文档目录
juglans doctest docs/

# JSON 格式
juglans doctest docs/ --format json
```

编写可通过 doctest 的代码块规则：

1. 节点定义必须在边定义之前
2. 边引用的节点必须已定义
3. 不需要验证的代码块加 `ignore` 标记

示例：

```juglans
# 这个代码块会被 doctest 验证
[start]: print(message="hello")
[end]: print(message="done")
[start] -> [end]
```

## CI Integration

在 GitHub Actions 中集成 Juglans 检查：

```yaml
# .github/workflows/check.yml
name: Juglans Check

on: [push, pull_request]

jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Juglans
        run: |
          cargo install --path juglans

      - name: Syntax Check
        run: juglans check ./src/

      - name: Doctest
        run: juglans doctest ./docs/
```

Exit code 说明：

| Command | Exit 0 | Exit 1 |
|---------|--------|--------|
| `juglans check` | 所有文件验证通过 | 存在语法错误 |
| `juglans doctest` | 所有代码块解析通过 | 存在解析失败的代码块 |

两个命令都适合直接在 CI pipeline 中使用，无需额外配置。

## Best Practices

1. **Commit 前** -- 运行 `juglans check` 确保语法正确
2. **文档更新后** -- 运行 `juglans doctest docs/` 确保示例代码有效
3. **CI 中** -- 同时运行 `check` 和 `doctest`，对应不同 step
4. **手动测试** -- 使用 `--verbose` 查看详细执行过程，用 `--dry-run` 快速验证结构

## Next Steps

- [Debugging](./debugging.md) -- 调试技巧
- [Error Handling](./error-handling.md) -- 在 workflow 中处理错误
- [CLI Reference](../reference/cli.md) -- 完整命令参考
