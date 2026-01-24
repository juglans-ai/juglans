# 快速入门

本指南将帮助你在 5 分钟内创建并运行第一个 Juglans 工作流。

## 前置要求

- Rust 1.70+ (用于编译)
- 一个 LLM API Key (DeepSeek, OpenAI 等)

## 1. 安装 Juglans

```bash
# 克隆仓库
git clone https://github.com/juglans-ai/juglans.git
cd juglans

# 编译
cargo build --release

# 添加到 PATH (可选)
export PATH="$PATH:$(pwd)/target/release"
```

## 2. 初始化项目

```bash
# 创建新项目
juglans init my-ai-project
cd my-ai-project
```

这会创建以下结构：
```
my-ai-project/
├── juglans.toml        # 配置文件
├── prompts/            # Prompt 模板
├── agents/             # Agent 配置
└── workflows/          # 工作流定义
```

## 3. 配置 API

编辑 `juglans.toml`：

```toml
[account]
id = "your_user_id"
api_key = "your_api_key"

[jug0]
base_url = "http://localhost:3000"  # 或你的 Jug0 服务地址
```

## 4. 创建 Agent

创建 `agents/assistant.jgagent`：

```yaml
slug: "assistant"
name: "AI Assistant"
model: "deepseek-chat"
temperature: 0.7
system_prompt: |
  You are a helpful AI assistant.
  Be concise and accurate in your responses.
```

## 5. 创建 Prompt 模板

创建 `prompts/analyze.jgprompt`：

```yaml
---
slug: "analyze"
name: "Analysis Prompt"
description: "分析用户输入并提供结构化响应"
inputs:
  topic: ""
  style: "professional"
---
Please analyze the following topic: {{ topic }}

Requirements:
- Style: {{ style }}
- Provide key insights
- Be structured and clear

{% if style == "casual" %}
Feel free to use informal language.
{% endif %}
```

## 6. 创建工作流

创建 `workflows/main.jgflow`：

```yaml
name: "Analysis Workflow"
version: "0.1.0"
description: "A simple analysis workflow"

# 导入资源
prompts: ["../prompts/*.jgprompt"]
agents: ["../agents/*.jgagent"]

# 入口和出口节点
entry: [init]
exit: [complete]

# 节点定义
[init]: notify(status="Starting analysis...")

[render_prompt]: p(
  slug="analyze",
  topic=$input.topic,
  style=$input.style
)

[analyze]: chat(
  agent="assistant",
  message=$output
)

[complete]: notify(status="Analysis complete!")

# 执行流程
[init] -> [render_prompt] -> [analyze] -> [complete]
```

## 7. 运行工作流

```bash
# 运行工作流
juglans workflows/main.jgflow --input '{
  "topic": "AI workflow orchestration",
  "style": "professional"
}'
```

输出示例：
```
[init] Starting analysis...
[render_prompt] Rendered prompt: analyze
[analyze] Calling agent: assistant
[analyze] Response: AI workflow orchestration is a systematic approach...
[complete] Analysis complete!
```

## 8. 交互式 Agent

直接与 Agent 对话：

```bash
juglans agents/assistant.jgagent
```

进入交互模式：
```
> What is Juglans?
Juglans is a Rust-based AI workflow orchestration framework...

> Tell me more about its features
Juglans offers several key features:
1. Declarative DSL for defining workflows
2. Support for conditional branching and loops
...

> exit
Goodbye!
```

## 下一步

- [核心概念](../guide/concepts.md) - 深入理解 Agent、Prompt、Workflow
- [工作流语法](../guide/workflow-syntax.md) - 完整的 .jgflow 语法
- [内置工具](../reference/builtins.md) - chat、p、notify 等工具详解
- [条件与分支](../guide/conditionals.md) - 实现复杂逻辑

## 常见问题

### Q: 如何调试工作流？

使用 `--verbose` 参数查看详细日志：
```bash
juglans workflows/main.jgflow --verbose
```

### Q: 如何使用本地模型？

在 `juglans.toml` 中配置本地端点：
```toml
[jug0]
base_url = "http://localhost:11434/v1"  # Ollama 示例
```

### Q: 支持哪些模型？

支持任何兼容 OpenAI API 的模型：
- DeepSeek (deepseek-chat, deepseek-coder)
- OpenAI (gpt-4o, gpt-4-turbo)
- Anthropic (claude-3-opus, claude-3-sonnet)
- 本地模型 (Ollama, vLLM)
