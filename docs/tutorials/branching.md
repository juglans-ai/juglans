# Tutorial 3: Branching & Routing

本章学习两种路由方式：**条件边（if）** 和 **多路切换（switch）**，让 workflow 根据数据做出决策。

## 条件边 -- if

最基本的分支：根据条件走不同路径。

```juglans
[check]: set_context(score=85)
[pass]: print(message="Passed!")
[fail]: print(message="Failed.")
[done]: print(message="Done")

[check] if $ctx.score >= 60 -> [pass]
[check] if $ctx.score < 60 -> [fail]
[pass] -> [done]
[fail] -> [done]
```

逐行解释：

1. `[check]` 节点将 `score` 写入上下文，值为 85。
2. `[check] if $ctx.score >= 60 -> [pass]` -- 如果分数 >= 60，走 `[pass]`。
3. `[check] if $ctx.score < 60 -> [fail]` -- 如果分数 < 60，走 `[fail]`。
4. 两条路径最终都汇聚到 `[done]`。

条件边的语法：

```text
[源节点] if 条件表达式 -> [目标节点]
```

条件为真时，边才会被执行。

### 支持的运算符

**比较运算符：**

| 运算符 | 含义 | 示例 |
|--------|------|------|
| `==` | 等于 | `$ctx.status == "ok"` |
| `!=` | 不等于 | `$ctx.status != "error"` |
| `>` | 大于 | `$ctx.score > 80` |
| `<` | 小于 | `$ctx.score < 60` |
| `>=` | 大于等于 | `$ctx.level >= 3` |
| `<=` | 小于等于 | `$ctx.count <= 10` |

**逻辑运算符：**

| 运算符 | 含义 | 示例 |
|--------|------|------|
| `&&` 或 `and` | 与 | `$ctx.a && $ctx.b` |
| `\|\|` 或 `or` | 或 | `$ctx.a \|\| $ctx.b` |
| `!` 或 `not` | 非 | `!$ctx.banned` |

### 字符串比较

字符串值用双引号包裹：

```juglans
[input]: set_context(type="question")
[question]: print(message="Handling question")
[task]: print(message="Handling task")
[other]: print(message="Unknown type")

[input] if $ctx.type == "question" -> [question]
[input] if $ctx.type == "task" -> [task]
[input] -> [other]
```

注意最后一行 `[input] -> [other]` -- 这是**无条件边**，当前面所有条件都不满足时作为默认路径。

## 多条件组合

用逻辑运算符组合多个条件：

```juglans
[check]: set_context(role="admin", level=5, banned=false)
[admin]: print(message="Welcome, admin")
[vip]: print(message="VIP access")
[normal]: print(message="Normal user")
[blocked]: print(message="Access denied")

[check] if $ctx.banned -> [blocked]
[check] if $ctx.role == "admin" && $ctx.level > 3 -> [admin]
[check] if $ctx.role == "vip" || $ctx.level > 8 -> [vip]
[check] -> [normal]
```

条件按定义顺序逐条求值。第一个为真的条件胜出，后续条件**不再检查**。所以要把最具体的条件放在前面。

### 分支汇聚

多条路径汇聚到同一个节点是常见模式：

```juglans
[evaluate]: set_context(grade="B")
[excellent]: print(message="Outstanding!")
[good]: print(message="Well done")
[average]: print(message="Keep going")
[summary]: print(message="Evaluation complete")

[evaluate] if $ctx.grade == "A" -> [excellent]
[evaluate] if $ctx.grade == "B" -> [good]
[evaluate] -> [average]

[excellent] -> [summary]
[good] -> [summary]
[average] -> [summary]
```

`[summary]` 有三个前驱节点，但只有一条路径会被执行。Juglans 使用 **OR 语义**：任意一个前驱完成，汇聚节点就执行。未走到的分支会被自动标记为不可达。

## switch 路由 -- 多路互斥

当分支基于某个变量的值做多路选择时，`switch` 比多条 `if` 更清晰：

```juglans
[classify]: set_context(intent="question")
[handle_q]: print(message="Question handler")
[handle_t]: print(message="Task handler")
[handle_c]: print(message="Chat handler")
[fallback]: print(message="Unknown intent")
[done]: print(message="Done")

[classify] -> switch $ctx.intent {
    "question": [handle_q]
    "task": [handle_t]
    "chat": [handle_c]
    default: [fallback]
}
[handle_q] -> [done]
[handle_t] -> [done]
[handle_c] -> [done]
[fallback] -> [done]
```

语法结构：

```text
[源节点] -> switch $变量 {
    "值1": [目标1]
    "值2": [目标2]
    default: [兜底节点]
}
```

规则：

- 变量的值与每个 case 逐一匹配，只走**第一个**匹配的分支。
- `default` 处理所有未匹配的情况。
- `default` 不是必须的，但强烈建议总是写上，避免出现"无路可走"的死路。

### switch vs if

什么时候用哪个？

| 场景 | 推荐 | 原因 |
|------|------|------|
| 基于一个变量的值做多路选择 | `switch` | 语义清晰，一个 block 搞定 |
| 二选一 | `if` | 简洁，两行就够 |
| 需要复杂条件（范围、逻辑组合） | `if` | switch 只做等值匹配 |
| 需要默认路径 | 都行 | switch 用 `default`，if 用无条件边 |

核心区别：`switch` 保证只走一个分支，`if` 条件边在理论上可以同时满足多条（虽然引擎按顺序只走第一个为真的）。

## 无条件边 + 条件边混合

一个节点可以同时拥有无条件边和条件边：

```juglans
[start]: set_context(priority="high")
[log]: print(message="Logging request...")
[fast_track]: print(message="Priority routing!")
[done]: print(message="Complete")

[start] -> [log]
[start] if $ctx.priority == "high" -> [fast_track]
[log] -> [done]
[fast_track] -> [done]
```

执行行为：

- 无条件边 `[start] -> [log]` **总是**执行。
- 条件边 `[start] if ... -> [fast_track]` 只在条件为真时执行。
- 如果条件为真，`[log]` 和 `[fast_track]` **都会**执行，最终都汇聚到 `[done]`。

这和 `switch` 的"只走一个分支"不同。如果你需要严格互斥，用 `switch` 或确保 `if` 条件互斥。

## 综合示例

一个根据消息类型和优先级路由的 workflow：

```juglans
name: "Message Router"
version: "0.1.0"

entry: [receive]
exit: [done]

[receive]: set_context(type="task", priority="high")

[done]: print(message="Routing complete")

# 第一层：按消息类型路由
[route_type]: print(message="Routing by type...")
[handle_question]: print(message="Answering question")
[route_task]: print(message="Processing task...")
[handle_other]: print(message="General handler")

[receive] -> [route_type]

[route_type] -> switch $ctx.type {
    "question": [handle_question]
    "task": [route_task]
    default: [handle_other]
}

# 第二层：task 按优先级路由
[urgent]: print(message="URGENT: handling immediately")
[normal]: print(message="Queued for processing")

[route_task] if $ctx.priority == "high" -> [urgent]
[route_task] -> [normal]

# 所有路径汇聚
[handle_question] -> [done]
[urgent] -> [done]
[normal] -> [done]
[handle_other] -> [done]
```

这个例子展示了两层路由的组合：

1. 第一层用 `switch` 按类型分流。
2. 第二层用 `if` 按优先级细分 task 路径。
3. 所有分支最终汇聚到 `[done]`。

这是实际项目中最常见的路由模式：先粗分，再细分，最终合流。

## 小结

- **条件边** `[node] if expr -> [target]` -- 条件为真时走这条边
- **switch** `[node] -> switch $var { "val": [target], default: [fb] }` -- 基于变量值的多路互斥选择
- 条件按定义顺序求值，第一个为真的胜出
- 无条件边 `[node] -> [target]` 可作为默认路径
- 分支汇聚使用 OR 语义：任一前驱完成即触发
- `switch` 适合等值多选，`if` 适合复杂条件和二选一

下一章：[Tutorial 4: 循环](./loops.md) -- 学习 `foreach` 和 `while`，让 workflow 重复执行。
