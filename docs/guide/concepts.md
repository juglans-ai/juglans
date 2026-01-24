# 核心概念

本指南介绍 Juglans 的核心概念和设计理念。

## 概述

Juglans 是一个 AI 工作流编排框架，通过三种核心资源类型来组织和执行复杂的 AI 任务：

```
┌─────────────────────────────────────────────────────┐
│                    Workflow                          │
│                   (.jgflow)                          │
│                                                      │
│   ┌─────────┐    ┌─────────┐    ┌─────────┐        │
│   │  Node   │───▶│  Node   │───▶│  Node   │        │
│   └────┬────┘    └────┬────┘    └────┬────┘        │
│        │              │              │              │
│        ▼              ▼              ▼              │
│   ┌─────────┐    ┌─────────┐    ┌─────────┐        │
│   │  Agent  │    │ Prompt  │    │  Agent  │        │
│   └─────────┘    └─────────┘    └─────────┘        │
└─────────────────────────────────────────────────────┘
```

## Agent

**Agent** 是一个配置化的 AI 实体，定义了：

- 使用的模型（如 GPT-4, DeepSeek）
- 行为特征（温度、系统提示）
- 可用能力（MCP 工具、技能）

### 用途

- 封装特定领域的 AI 能力
- 复用一致的 AI 行为配置
- 实现多 Agent 协作

### 示例

```yaml
# agents/analyst.jgagent
slug: "analyst"
model: "gpt-4o"
temperature: 0.5
system_prompt: |
  You are a data analyst expert.
  Provide accurate, data-driven insights.
```

### 在工作流中使用

```yaml
[analyze]: chat(agent="analyst", message=$input.data)
```

---

## Prompt

**Prompt** 是可复用的提示词模板，支持：

- 变量插值
- 条件逻辑
- 循环迭代
- 过滤器

### 用途

- 分离提示词逻辑和工作流逻辑
- 复用常见的提示模式
- 版本控制和迭代优化

### 示例

```yaml
# prompts/report.jgprompt
---
slug: "report"
inputs:
  data: {}
  format: "markdown"
---
Generate a {{ format }} report for:

{{ data | json }}

Include key findings and recommendations.
```

### 在工作流中使用

```yaml
[render]: p(slug="report", data=$ctx.results, format="html")
[generate]: chat(agent="writer", message=$output)
```

---

## Workflow

**Workflow** 是执行图，定义了：

- 节点（执行单元）
- 边（执行顺序和条件）
- 入口和出口

### 用途

- 编排复杂的多步骤任务
- 实现条件分支和循环
- 组合多个 Agent 和 Prompt

### 示例

```yaml
# workflows/pipeline.jgflow
name: "Data Pipeline"

entry: [start]
exit: [end]

[start]: notify(status="Starting...")
[process]: chat(agent="processor", message=$input.data)
[end]: notify(status="Done")

[start] -> [process] -> [end]
```

---

## 执行上下文

工作流执行时维护一个上下文（Context），存储：

- 输入数据 (`$input`)
- 节点输出 (`$output`)
- 自定义变量 (`$ctx`)
- 回复元数据 (`$reply`)

### 变量路径

```yaml
$input.field          # 输入字段
$output               # 当前节点输出
$output.nested.field  # 嵌套访问
$ctx.my_var           # 上下文变量
$reply.tokens         # 回复 token 数
```

### 变量流动

```
                    输入
                      │
                      ▼
┌──────────────────────────────────────┐
│              Context                  │
│  $input: { query: "..." }            │
│  $ctx: {}                            │
│  $output: null                        │
└──────────────────────────────────────┘
                      │
          ┌───────────┴───────────┐
          ▼                       ▼
    ┌──────────┐           ┌──────────┐
    │  Node A  │           │  Node B  │
    │  $output │           │  $output │
    └────┬─────┘           └────┬─────┘
         │                      │
         └───────────┬──────────┘
                     ▼
┌──────────────────────────────────────┐
│              Context                  │
│  $input: { query: "..." }            │
│  $ctx: { result_a: ..., result_b: }  │
│  $output: (最后一个节点输出)           │
└──────────────────────────────────────┘
```

---

## 执行模型

### 图遍历

工作流是有向无环图（DAG），执行器按拓扑顺序遍历：

```
     [A]
    /   \
  [B]   [C]
    \   /
     [D]
```

执行顺序：A → B (并行 C) → D

### 条件路由

```yaml
[router] if $ctx.type == "a" -> [path_a]
[router] if $ctx.type == "b" -> [path_b]
[router] -> [default]
```

只有满足条件的路径会被执行。

### 错误处理

```yaml
[risky] -> [success]
[risky] on error -> [fallback]
```

`on error` 路径在节点失败时执行。

---

## 资源组织

### 推荐项目结构

```
my-project/
├── juglans.toml          # 配置
├── prompts/              # Prompt 模板
│   ├── system/           # 系统 Prompt
│   ├── tasks/            # 任务 Prompt
│   └── common/           # 通用 Prompt
├── agents/               # Agent 配置
│   ├── core/             # 核心 Agent
│   └── specialized/      # 专业 Agent
└── workflows/            # 工作流
    ├── main.jgflow       # 主工作流
    └── sub/              # 子工作流
```

### 资源引用

**相对路径导入：**

```yaml
prompts: ["./prompts/**/*.jgprompt"]
agents: ["./agents/**/*.jgagent"]
```

**通过 Slug 引用：**

```yaml
[node]: chat(agent="my-agent")
[node]: p(slug="my-prompt")
```

---

## 设计原则

### 1. 声明式优于命令式

定义"什么"而不是"怎么做"：

```yaml
# 好：声明式
[classify]: chat(agent="classifier", format="json")
[classify] if $output.type == "A" -> [handle_a]

# 避免：复杂的命令式逻辑
```

### 2. 组合优于继承

通过组合小的、专注的资源构建复杂功能：

```yaml
# 多个专业 Agent
agents/classifier.jgagent    # 分类
agents/analyzer.jgagent      # 分析
agents/writer.jgagent        # 写作

# 在工作流中组合
[classify] -> [analyze] -> [write]
```

### 3. 关注点分离

- Prompt：内容和格式
- Agent：能力和行为
- Workflow：流程和逻辑

### 4. 可测试性

每个组件都可以独立测试：

```bash
# 测试 Prompt
juglans prompts/my-prompt.jgprompt --input '{...}'

# 测试 Agent
juglans agents/my-agent.jgagent --message "test"

# 测试 Workflow
juglans workflows/my-flow.jgflow --input '{...}'
```

---

## 与 Jug0 的关系

Juglans 是 DSL 和本地执行器，Jug0 是后端平台：

```
┌─────────────────┐     ┌─────────────────┐
│    Juglans      │     │      Jug0       │
│    (本地)        │────▶│    (后端)        │
│                 │     │                 │
│  - DSL 解析     │     │  - LLM 调用     │
│  - 工作流执行   │     │  - 资源存储     │
│  - 本地开发     │     │  - API 服务     │
└─────────────────┘     └─────────────────┘
```

**本地模式：** 使用本地文件，适合开发

**远程模式：** 资源存储在 Jug0，适合生产

---

## 下一步

- [工作流语法](./workflow-syntax.md) - 详细语法参考
- [Prompt 语法](./prompt-syntax.md) - 模板语法
- [Agent 语法](./agent-syntax.md) - Agent 配置
- [内置工具](../reference/builtins.md) - 可用工具
