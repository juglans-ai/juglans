# Core Concepts

Juglans 的核心概念速查。本页面提供精简的概念总览，详细用法请参考各专题指南。

## Three File Types

| Type | Extension | Parser | Purpose |
|------|-----------|--------|---------|
| Workflow | `.jg` | GraphParser | 定义 DAG 执行流程，串联节点、边、条件分支 |
| Agent | `.jgagent` | AgentParser | 配置 AI 模型参数（model、system prompt、tools） |
| Prompt | `.jgprompt` | PromptParser | Jinja 风格的可复用提示词模板 |

**Decision tree:**

```
需要多步骤编排？        → .jg (Workflow)
需要配置 AI 模型行为？   → .jgagent (Agent)
需要复用/模板化提示词？   → .jgprompt (Prompt)
```

最小 Workflow 示例：

```juglans
[greet]: print(message="Hello!")
[done]: print(message="Done.")
[greet] -> [done]
```

## DAG Execution Model

Workflow 在内部是一个**有向无环图（DAG）**。引擎按**拓扑排序**执行节点——先执行没有依赖的节点，再执行有依赖的节点。

```
     [A]
    /   \
  [B]   [C]
    \   /
     [D]
```

执行顺序：A -> B, C (可并行) -> D。无环保证执行必终止。

条件边只在运行时求值，未满足的分支自动跳过：

```juglans
[check]: set_context(mode=$input.mode)
[fast]: print(message="fast path")
[slow]: print(message="slow path")

[check] if $ctx.mode == "fast" -> [fast]
[check] -> [slow]
```

## Variable System

| Variable | Scope | Description |
|----------|-------|-------------|
| `$input.field` | Global | CLI/API 传入的输入数据 |
| `$output` | Per-node | 上一个节点的输出 |
| `$ctx.key` | Global | 自定义上下文变量（via `set_context()`） |
| `$reply.output` | Per-chat | Agent 回复的元数据 |
| `$error` | Error path | 错误信息对象（在 `on error` 路径中可用） |

变量在节点参数中通过路径访问：

```juglans
[save]: set_context(user=$input.name)
[greet]: print(message="Hello, " + $ctx.user)
[save] -> [greet]
```

## Tool Resolution Order

当节点调用工具时，引擎按以下顺序查找：

```
1. Builtin    — chat, p, notify, print, set_context, fetch, bash...
2. Function   — 当前 workflow 中定义的 [name(params)]: { ... }
3. Python     — 直接调用 Python 模块（pandas.read_csv() 等）
4. MCP        — 外部 MCP 服务器提供的工具
5. Client Bridge — 未匹配的工具通过 SSE 转发给前端
```

在 Workflow 中调用内置工具：

```juglans
[step1]: print(message="start")
[step2]: notify(status="processing")
[step3]: set_context(result="done")
[step1] -> [step2] -> [step3]
```

## Juglans and Jug0

```
┌─────────────────┐     ┌─────────────────┐
│    Juglans      │     │      Jug0       │
│   (Local CLI)   │────>│   (Backend)     │
│                 │     │                 │
│  - DSL 解析     │     │  - LLM 调用     │
│  - DAG 执行     │     │  - 资源存储     │
│  - 本地开发     │     │  - API 服务     │
└─────────────────┘     └─────────────────┘
```

- **Juglans** 是本地引擎：解析 DSL、执行 DAG、管理工具调用
- **Jug0** 是后端平台：提供 LLM API、资源云端存储、用户管理
- 本地开发时可离线使用本地文件；生产部署时通过 Jug0 管理资源

资源引用方式：本地用 slug（如 `"my-agent"`），远程用 `owner/slug`（如 `"juglans/assistant"`）：

```juglans
[local]: chat(agent="my-agent", message=$input.query)
[remote]: chat(agent="juglans/cloud-agent", message=$output)
[local] -> [remote]
```

## Next Steps

- [Workflow Syntax](./workflow-syntax.md) — 完整语法参考
- [Agent Syntax](./agent-syntax.md) — Agent 配置
- [Prompt Syntax](./prompt-syntax.md) — 模板语法
- [Connect AI](./connect-ai.md) — 连接 AI 模型
- [Debugging](./debugging.md) — 调试与排错
