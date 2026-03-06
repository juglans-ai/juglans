# Tutorial 8: Workflow Composition

本章学习如何消除重复代码：用**函数定义**封装可复用逻辑，用 **flows** 导入外部子工作流，用 **libs** 引入函数库，将大型 workflow 拆分成可组合的模块。

## 8.1 问题：代码重复

假设你需要在 workflow 中多次执行同样的"记录日志"操作：

```juglans
[log1]: print(message="[LOG] Step 1 started")
[work1]: set_context(data="result1")
[log2]: print(message="[LOG] Step 2 started")
[work2]: set_context(data="result2")
[log3]: print(message="[LOG] Step 3 started")
[work3]: set_context(data="result3")

[log1] -> [work1] -> [log2] -> [work2] -> [log3] -> [work3]
```

`print(message="[LOG] ...")` 重复了三次。如果日志格式要改，三处都要改。随着 workflow 增长，这种重复会越来越难维护。

## 8.2 函数定义

函数让你把重复的逻辑封装成一个可复用的单元。

### 最简示例

```juglans
[log(msg)]: print(message="[LOG] " + $msg)

[step1]: log(msg="Step 1 started")
[step2]: log(msg="Step 2 started")
[step3]: log(msg="Step 3 started")

[step1] -> [step2] -> [step3]
```

逐行解释：

1. `[log(msg)]` — 定义一个名为 `log` 的函数，接受一个参数 `msg`。方括号内的 `(msg)` 就是参数列表。
2. `: print(message="[LOG] " + $msg)` — 函数体。这里是单步函数，直接绑定一个工具调用。`$msg` 引用传入的参数。
3. `[step1]: log(msg="Step 1 started")` — 调用函数。语法与调用内置工具完全相同：`函数名(参数=值)`。
4. 函数定义**不会**出现在 DAG 中，它只是一个可被调用的模板。

函数定义语法：

```text
[函数名(参数1, 参数2, ...)]: 函数体
```

### 多参数

函数可以接受任意数量的参数：

```juglans
[greet(name, greeting)]: print(message=$greeting + ", " + $name + "!")

[step1]: greet(name="Alice", greeting="Hello")
[step2]: greet(name="Bob", greeting="Good morning")
[done]: print(message="All greeted!")

[step1] -> [step2] -> [done]
```

参数在函数体中通过 `$参数名` 访问。调用时必须提供所有参数。

### 多步函数

当函数体需要多个步骤时，用花括号 `{ ... }` 包裹：

```juglans
[deploy(service, env)]: {
  print(message="Deploying " + $service + " to " + $env)
  notify(status=$service + " deployed to " + $env)
}

[step1]: deploy(service="api", env="staging")
[step2]: deploy(service="web", env="staging")
[done]: print(message="All deployed")

[step1] -> [step2] -> [done]
```

逐行解释：

1. `[deploy(service, env)]:` — 函数签名，两个参数。
2. `{ ... }` — 多步函数体。内部的步骤按顺序执行，用换行或 `;` 分隔。
3. 第一步 `print(...)` 输出日志，第二步 `notify(...)` 发送通知。
4. 调用时 `deploy(service="api", env="staging")` 会依次执行两步。

多步函数体内的步骤会被自动串联成一条执行链。

### 用分号分隔

多步函数体也可以写成一行，用 `;` 分隔：

```juglans
[ping(host)]: { print(message="Pinging " + $host); notify(status="Pinged " + $host) }

[a]: ping(host="server-1")
[b]: ping(host="server-2")

[a] -> [b]
```

## 8.3 函数调用

函数调用在节点位置使用，语法与工具调用相同：

```juglans
[check(item)]: print(message="Checking: " + $item)

[c1]: check(item="database")
[c2]: check(item="cache")
[c3]: check(item="queue")
[report]: print(message="All checks done")

[c1] -> [c2] -> [c3] -> [report]
```

调用时，引擎会：

1. 查找名为 `check` 的函数定义。
2. 将参数 `item="database"` 绑定到函数体中的 `$item`。
3. 执行函数体。
4. 将函数体的输出存入 `$output`，供后续节点使用。

函数调用和内置工具调用在语法上完全一致。引擎按如下顺序解析工具名：内置工具 -> 函数定义 -> Python -> MCP -> 客户端桥接。

## 8.4 Flow Import

当 workflow 变大时，你会希望把逻辑拆分到多个文件中。`flows:` 让你导入外部 `.jg` 文件作为子工作流。

### 基本用法

假设你有一个认证子工作流 `auth.jg`：

```text
# auth.jg
[login]: set_context(user="authenticated")
[verify]: print(message="Verifying user...")
[complete]: print(message="Auth complete")

[login] -> [verify] -> [complete]
```

在主工作流中导入它：

```juglans
flows: {
  auth: "./auth.jg"
}

[start]: print(message="Starting app")
[done]: print(message="App ready")

[start] -> [auth.login]
[auth.complete] -> [done]
```

逐行解释：

1. `flows: { auth: "./auth.jg" }` — 在文件头部声明子工作流导入。`auth` 是别名，`"./auth.jg"` 是文件路径（相对于当前 `.jg` 文件）。
2. `[auth.login]` — 引用子工作流中的节点。格式为 `[别名.节点名]`。
3. 引擎在编译时将 `auth.jg` 的节点和边合并到主图中，所有节点自动加上 `auth.` 命名空间前缀。

### 命名空间前缀

导入后，子工作流的所有节点 ID 自动变为 `别名.原始ID`：

| auth.jg 中的节点 | 合并后的 ID |
|-------------------|-------------|
| `[login]` | `[auth.login]` |
| `[verify]` | `[auth.verify]` |
| `[complete]` | `[auth.complete]` |

命名空间隔离了不同子工作流的节点名，避免冲突。

### 跨工作流连接

在主工作流的边定义中引用子工作流节点：

```juglans
flows: {
  auth: "./auth.jg"
  payment: "./payment.jg"
}

[start]: print(message="Order flow")
[done]: print(message="Order complete")

[start] -> [auth.login]
[auth.complete] -> [payment.charge]
[payment.done] -> [done]
```

子工作流之间也可以通过主工作流的边相互连接。`[auth.complete] -> [payment.charge]` 将认证子流的输出接入支付子流。

### 多个 Flow Import

`flows:` 支持同时导入多个子工作流：

```juglans
flows: {
  auth: "./flows/auth.jg"
  notify: "./flows/notify.jg"
  log: "./flows/log.jg"
}

[start]: print(message="Begin")
[done]: print(message="End")

[start] -> [auth.start]
[auth.done] -> [notify.send]
[notify.done] -> [log.write]
[log.done] -> [done]
```

每个子工作流都有独立的命名空间，节点名不会冲突。

## 8.5 Library Import

`libs:` 用于导入**函数库**——只包含函数定义的 `.jg` 文件。与 `flows:` 不同，`libs:` 不会合并子图节点，只提取函数定义。

### 库文件

一个典型的库文件（`utils.jg`）只包含函数定义：

```text
# utils.jg
slug: "utils"

[log(msg)]: print(message="[LOG] " + $msg)
[format_name(first, last)]: set_context(full_name=$first + " " + $last)
```

### 列表形式导入

```juglans
libs: ["./utils.jg"]

[step1]: utils.log(msg="Starting")
[step2]: print(message="Working...")

[step1] -> [step2]
```

逐行解释：

1. `libs: ["./utils.jg"]` — 列表形式导入库文件。可以同时导入多个：`libs: ["./utils.jg", "./math.jg"]`。
2. `utils.log(msg="Starting")` — 调用库中的函数，格式为 `命名空间.函数名(参数)`。

**命名空间规则**（列表形式）：

1. 如果库文件声明了 `slug`，用 slug 作为命名空间。
2. 否则用文件名（不含扩展名）作为命名空间。

### 对象形式导入

如果想自定义命名空间，用对象形式：

```juglans
libs: {
  u: "./utils.jg"
}

[step1]: u.log(msg="Custom namespace")
```

`u` 是你指定的命名空间，覆盖文件内的 slug 和文件名。

### 导入多个库

```juglans
libs: ["./string_utils.jg", "./math_utils.jg"]

[step1]: string_utils.upper(text="hello")
[step2]: math_utils.add(a=1, b=2)
[done]: print(message="Done")

[step1] -> [step2] -> [done]
```

每个库文件的函数在各自的命名空间下，不会相互冲突。

## 8.6 综合示例

将函数定义、flow import 和 library import 组合在一起：

```juglans
name: "Order Pipeline"
version: "0.1.0"

flows: {
  payment: "./flows/payment.jg"
}
libs: ["./lib/helpers.jg"]

entry: [start]
exit: [report]

# 本地函数定义
[validate(data)]: {
  print(message="Validating: " + $data)
  set_context(is_valid=true)
}

# 入口
[start]: set_context(order_id="ORD-001")

# 调用本地函数
[check]: validate(data=$ctx.order_id)

# 调用库函数
[log]: helpers.log(msg="Order validated: " + $ctx.order_id)

# 汇报
[report]: print(message="Order " + $ctx.order_id + " processed")

# 执行流：本地 -> 子工作流 -> 汇报
[start] -> [check]
[check] -> [log]
[log] -> [payment.start]
[payment.done] -> [report]
```

这个 workflow 展示了三种组合机制的协作：

1. **本地函数** `validate` — 封装验证逻辑，可在当前文件中多次调用。
2. **Flow import** `payment` — 完整的支付子流程，作为子图合并进来。
3. **Library import** `helpers` — 复用工具函数，按命名空间调用。

三者各有适用场景：

| 机制 | 适用场景 | 特点 |
|------|----------|------|
| 函数定义 | 当前文件内的代码复用 | 最简单，就地定义 |
| `flows:` | 导入完整的子工作流（有节点和边） | 合并子图，命名空间隔离 |
| `libs:` | 导入纯函数库 | 只提取函数，不合并子图 |

## 小结

- **函数定义** `[name(params)]: body` -- 封装可复用逻辑，调用语法与内置工具相同
- **单步函数** `[f(x)]: tool(...)` -- 直接绑定一个工具调用
- **多步函数** `[f(x)]: { step1; step2 }` -- 花括号内多步顺序执行
- **Flow Import** `flows: { alias: "path.jg" }` -- 导入完整子工作流，节点自动加命名空间前缀
- **跨工作流边** `[local] -> [alias.node]` -- 在边中引用子工作流节点
- **Library Import** `libs: ["path.jg"]` -- 导入函数库，通过 `namespace.func()` 调用
- 三种机制可以组合使用，各有适用场景

下一章：[Tutorial 9: Full Project](./full-project.md) -- 将前 8 章所有知识整合，从零构建一个完整的 AI 助手项目。
