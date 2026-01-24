# Juglans

**Juglans** 是一个基于 Rust 的 AI 工作流编排框架，提供声明式 DSL 来定义和执行复杂的 AI Agent 工作流。

## 特性

- **声明式 DSL** - 使用 `.jgflow`、`.jgprompt`、`.jgagent` 三种文件格式定义工作流
- **图执行引擎** - 支持条件分支、循环、错误处理的 DAG 执行
- **模板系统** - Jinja 风格的 Prompt 模板，支持变量插值和控制流
- **多 Agent 协作** - 灵活配置多个 Agent 协同工作
- **MCP 集成** - 支持 Model Context Protocol 扩展工具能力
- **Jug0 后端** - 与 Jug0 AI 平台无缝集成
- **跨平台** - 支持 Native + WebAssembly

## 快速开始

### 安装

```bash
# 从源码构建
git clone https://github.com/juglans-ai/juglans.git
cd juglans
cargo build --release

# 添加到 PATH
export PATH="$PATH:$(pwd)/target/release"
```

### 创建第一个 Agent

```yaml
# my-agent.jgagent
slug: "my-assistant"
name: "My Assistant"
model: "deepseek-chat"
temperature: 0.7
system_prompt: "You are a helpful assistant."
```

运行：
```bash
juglans my-agent.jgagent
```

### 创建 Prompt 模板

```yaml
# greeting.jgprompt
---
slug: "greeting"
name: "Greeting Prompt"
inputs:
  name: "World"
---
Hello, {{ name }}! How can I help you today?
```

### 创建工作流

```yaml
# chat-flow.jgflow
name: "Simple Chat"
version: "0.1.0"

prompts: ["./prompts/*.jgprompt"]
agents: ["./agents/*.jgagent"]

entry: [start]
exit: [end]

[start]: notify(status="Starting chat...")
[chat]: chat(agent="my-assistant", message=$input.question)
[end]: notify(status="Done")

[start] -> [chat] -> [end]
```

运行：
```bash
juglans chat-flow.jgflow --input '{"question": "What is Juglans?"}'
```

## 文档

- [快速入门](./getting-started/quickstart.md) - 5 分钟上手教程
- [核心概念](./guide/concepts.md) - Agent、Prompt、Workflow 介绍
- [DSL 语法](./guide/workflow-syntax.md) - 完整语法参考
- [CLI 参考](./reference/cli.md) - 命令行工具
- [内置工具](./reference/builtins.md) - chat、p、notify 等

## 架构

```
┌─────────────────────────────────────────────────────────┐
│                      Juglans CLI                        │
├─────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐     │
│  │  .jgflow    │  │  .jgprompt  │  │  .jgagent   │     │
│  │  Parser     │  │  Parser     │  │  Parser     │     │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘     │
│         │                │                │             │
│         ▼                ▼                ▼             │
│  ┌─────────────────────────────────────────────────┐   │
│  │              Workflow Executor                   │   │
│  │         (DAG Traversal + Context)               │   │
│  └──────────────────────┬──────────────────────────┘   │
│                         │                               │
│         ┌───────────────┼───────────────┐              │
│         ▼               ▼               ▼              │
│  ┌───────────┐   ┌───────────┐   ┌───────────┐        │
│  │  Builtins │   │  Jug0     │   │   MCP     │        │
│  │  (chat,p) │   │  Client   │   │  Client   │        │
│  └───────────┘   └───────────┘   └───────────┘        │
└─────────────────────────────────────────────────────────┘
```

## 文件格式

| 扩展名 | 用途 | 说明 |
|--------|------|------|
| `.jgflow` | 工作流 | 定义节点、边、执行逻辑 |
| `.jgprompt` | Prompt 模板 | 可复用的提示词模板 |
| `.jgagent` | Agent 配置 | 模型、温度、系统提示 |
| `juglans.toml` | 项目配置 | API 密钥、服务器设置 |

## 示例

查看 [examples/](../examples/) 目录获取更多示例：

- `examples/prompts/` - Prompt 模板示例
- `examples/agents/` - Agent 配置示例
- `examples/workflows/` - 工作流示例

## 技术栈

- **解析器**: Pest (PEG 语法)
- **脚本引擎**: Rhai (表达式求值)
- **图结构**: Petgraph (DAG)
- **异步运行时**: Tokio
- **Web 框架**: Axum
- **序列化**: Serde

## License

MIT License
