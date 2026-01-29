# Juglans 示例集合

本目录包含完整的 Juglans 使用示例，从基础到高级逐步展示工作流编排的各种模式。

## 📁 目录结构

```
examples/
├── prompts/          # Prompt 模板示例
│   ├── greeting.jgprompt
│   ├── analysis.jgprompt
│   └── README.md
├── agents/           # Agent 配置示例
│   ├── assistant.jgagent
│   ├── analyst.jgagent
│   └── README.md
└── workflows/        # 工作流示例
    ├── simple-chat.jgflow
    ├── router.jgflow
    ├── batch-process.jgflow
    └── README.md
```

## 🚀 快速开始

### 1. 渲染 Prompt
```bash
cd examples/prompts
juglans greeting.jgprompt --input '{"name": "Alice"}'
```

### 2. 与 Agent 对话
```bash
cd examples/agents
juglans assistant.jgagent --message "Explain recursion"
```

### 3. 运行工作流
```bash
cd examples/workflows
juglans simple-chat.jgflow --input '{"message": "Hello!"}'
```

## 📚 学习路径

### 初级 - 基础概念
1. **Prompt 模板** (`prompts/greeting.jgprompt`)
   - 变量插值
   - 条件渲染
   - 默认值

2. **Agent 配置** (`agents/assistant.jgagent`)
   - 基本字段
   - 模型选择
   - 系统提示

3. **简单工作流** (`workflows/simple-chat.jgflow`)
   - 节点定义
   - 线性流程
   - Agent 调用

### 中级 - 控制流
4. **条件分支** (`workflows/router.jgflow`)
   - `if` 语句
   - 多路分支
   - JSON 输出解析

5. **循环处理** (`workflows/batch-process.jgflow`)
   - `foreach` 循环
   - 上下文变量
   - 结果聚合

### 高级 - 实战模式
参考 [docs/examples/](../docs/examples/) 中的完整教程：
- RAG 知识库问答
- 意图识别路由
- 多 Agent 协作

## 🛠️ 实用命令

```bash
# 验证所有示例文件
juglans check examples/

# 查看工作流详细执行
juglans workflows/router.jgflow -v --input '{"query": "test"}'

# 查看 Agent 配置
juglans agents/analyst.jgagent --info

# 推送到服务器
juglans apply prompts/greeting.jgprompt
```

## 📖 相关文档

- [CLI 命令参考](../docs/reference/cli.md)
- [工作流语法](../docs/guide/workflow-syntax.md)
- [Agent 语法](../docs/guide/agent-syntax.md)
- [Prompt 语法](../docs/guide/prompt-syntax.md)
- [内置工具](../docs/reference/builtins.md)

## 💡 提示

- 所有示例都可以直接运行
- 修改输入参数来实验不同场景
- 使用 `--dry-run` 验证语法而不执行
- 查看 README.md 了解每个示例的详细说明
