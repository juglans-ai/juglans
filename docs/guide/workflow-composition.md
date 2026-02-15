# 工作流组合（Flow Imports）

通过 `flows:` 声明，可以将多个 `.jgflow` 文件组合成一张统一的执行图。子工作流的节点以命名空间前缀合并到父 DAG 中，实现跨文件的自由分支设计。

## 基本语法

### 声明导入

在元数据区使用 `flows:` 对象映射声明要导入的子工作流：

```yaml
flows: {
  auth: "./workflows/auth.jgflow"
  trading: "./workflows/trading.jgflow"
}
```

键为别名（alias），值为相对路径（相对于当前 `.jgflow` 文件所在目录）。

### 引用子工作流节点

使用 `[alias.node_id]` 格式引用子工作流中的节点：

```yaml
# 从父节点跳转到子工作流
[route] if $ctx.need_auth -> [auth.start]

# 从子工作流跳回父节点
[auth.done] -> [next_step]
```

### 最小完整示例

```yaml
name: "Main Router"
prompts: ["./prompts/*.jgprompt"]
agents: ["./agents/*.jgagent"]

flows: {
  trading: "./workflows/trading.jgflow"
  events: "./workflows/events.jgflow"
}

[start]: set_context(event_type=$input.event_type, message=$input.message)
[route]: set_context(routed=true)
[done]: reply(output=$output)

[start] -> [route]
[route] if $ctx.event_type -> [events.start]
[route] if $ctx.message -> [trading.start]

[events.respond] -> [done]
[trading.done] -> [done]
```

---

## 变量命名空间

子工作流内部的变量引用会自动加命名空间前缀。规则是：**只有第一段匹配子工作流内部节点 ID 的变量才加前缀**，其他变量（`$ctx`、`$input`、`$output` 等）保持不变。

### 转换规则

假设 `auth` 子工作流内部有 `verify`、`extract` 两个节点：

| 原始变量（子工作流内部） | 合并后变量 | 说明 |
|--------------------------|-----------|------|
| `$verify.output` | `$auth.verify.output` | `verify` 是子流节点，加前缀 |
| `$extract.output.intent` | `$auth.extract.output.intent` | `extract` 是子流节点，加前缀 |
| `$ctx.some_var` | `$ctx.some_var` | `ctx` 不是节点，不变 |
| `$input.message` | `$input.message` | `input` 不是节点，不变 |
| `$output` | `$output` | 不变 |

### 在父工作流中引用

父工作流可以通过命名空间路径访问子工作流节点的输出：

```yaml
# 子工作流 auth 中 verify 节点的输出
[next]: chat(message=$auth.verify.output)

# 条件中使用
[check] if $auth.extract.output.intent == "trade" -> [trade]
```

---

## 执行模型

### 编译时合并

`flows:` 导入在**编译时**（parse 后、execute 前）处理。子工作流的所有节点和边以命名空间前缀合并到父图中，形成一张统一的 DAG。

```
解析阶段：
  parent.jgflow  →  WorkflowGraph (含 pending edges)
  trading.jgflow →  WorkflowGraph
  events.jgflow  →  WorkflowGraph

合并阶段：
  parent + trading.* + events.* → 统一 DAG

执行阶段：
  executor 按拓扑序执行全部节点（不感知节点来源）
```

### 共享上下文

合并后的所有节点共享同一个 `WorkflowContext`：

- `$ctx` 在整个合并图中共享
- `$input` 是父工作流的输入
- 各节点的 `$output` 按正常拓扑顺序更新

### 执行流程

当父工作流的边指向子工作流节点（如 `[route] -> [trading.start]`），执行器会从 `[trading.start]` 开始，沿子工作流内部的边继续执行，直到遇到跳回父工作流的边（如 `[trading.done] -> [done]`）。

中间的所有子工作流节点（含内部的条件分支、switch 路由等）都会正常执行。

---

## 递归导入

子工作流可以有自己的 `flows:` 声明，实现多层组合：

```yaml
# main.jgflow
flows: {
  order: "./workflows/order.jgflow"
}

# order.jgflow
flows: {
  payment: "./workflows/payment.jgflow"
}
```

合并后，`payment` 子工作流的节点会以 `order.payment.` 为前缀出现在最终 DAG 中：

```
order.start → order.validate → order.payment.charge → order.payment.confirm → order.done
```

---

## 循环导入检测

如果出现循环导入（A 导入 B，B 又导入 A），编译器会报错：

```
Error: Circular flow import detected: 'auth' (/path/to/auth.jgflow)
Import chain: ["/path/to/main.jgflow", "/path/to/auth.jgflow"]
```

---

## 资源合并

子工作流声明的资源导入（prompts、agents、tools）会自动合并到父工作流，路径相对于子工作流文件所在目录解析：

```yaml
# workflows/trading.jgflow
prompts: ["./prompts/*.jgprompt"]    # 相对于 workflows/ 目录
agents: ["./agents/*.jgagent"]
```

合并后，父工作流可以使用子工作流引入的 prompts 和 agents。Python 模块导入也会自动合并（去重）。

---

## 完整示例

### 项目结构

```
my-project/
├── juglans.toml
├── main.jgflow
├── prompts/
│   └── system.jgprompt
├── agents/
│   └── router.jgagent
└── workflows/
    ├── trading.jgflow
    ├── events.jgflow
    └── agents/
        ├── trader.jgagent
        └── event-handler.jgagent
```

### 主工作流 — `main.jgflow`

```yaml
name: "Event Router"
agents: ["./agents/*.jgagent"]

flows: {
  trading: "./workflows/trading.jgflow"
  events: "./workflows/events.jgflow"
}

entry: [start]
exit: [done]

[start]: set_context(
  event_type=$input.event_type,
  message=$input.message
)
[route]: chat(
  agent="router",
  message=$input.message,
  format="json"
)
[done]: reply(output=$output)

[start] -> [route]

# 根据路由结果跳转到不同子工作流
[route] if $output.type == "event" -> [events.start]
[route] if $output.type == "trade" -> [trading.start]
[route] -> [done]

# 子工作流完成后汇聚
[events.respond] -> [done]
[trading.done] -> [done]
```

### 子工作流 — `workflows/trading.jgflow`

```yaml
name: "Trading Flow"
agents: ["./agents/*.jgagent"]

entry: [start]
exit: [done]

[start]: set_context(trade_started=true)
[extract]: chat(
  agent="trader",
  message=$ctx.message,
  format="json"
)
[execute]: chat(
  agent="trader",
  message="Execute trade: " + json($extract.output)
)
[done]: set_context(trade_result=$output)

[start] -> [extract] -> [execute] -> [done]
[extract] on error -> [done]
```

### 合并后的等效 DAG

```
[start] → [route] ─── if "event" ──→ [events.start] → ... → [events.respond] ─┐
                  ─── if "trade" ──→ [trading.start] → [trading.extract]        │
                  ─── default ─────→ [done] ←──────── → [trading.execute]       │
                                       ↑                → [trading.done] ───────┤
                                       └────────────────────────────────────────┘
```

---

## 最佳实践

1. **命名清晰** — 别名应反映子工作流的职责，如 `auth`、`trading`、`notification`
2. **显式连接** — 必须显式写出父子工作流之间的边（`[route] -> [auth.start]`、`[auth.done] -> [next]`），不支持隐式入口
3. **单一职责** — 每个子工作流专注于一个功能领域，通过组合实现复杂逻辑
4. **避免深层嵌套** — 递归导入虽然支持，但建议控制在 2-3 层以内
5. **上下文协议** — 在子工作流的注释中说明它期望的 `$ctx` 变量和输出格式，方便其他工作流正确对接
