# Tutorial 1: Hello Workflow

本章结束后你将理解四个核心概念：**节点（Node）**、**边（Edge）**、**工具（Tool）**，以及一个 `.jg` 文件的基本结构。

## 最小 workflow

创建一个文件 `hello.jg`，写入以下内容：

```juglans
[greet]: print(message="Hello!")
[done]: print(message="Done.")
[greet] -> [done]
```

运行它：

```bash
juglans hello.jg
```

输出：

```
Hello!
Done.
```

三行代码，三个概念：

- `[greet]` 和 `[done]` 是**节点**。方括号包裹一个唯一的名字，代表 workflow 中的一个执行单元。
- `print(message="Hello!")` 是**工具调用**。`print` 是工具名，`message` 是参数。节点通过 `:` 绑定一个工具调用。
- `[greet] -> [done]` 是**边**。箭头 `->` 表示执行顺序：`greet` 完成后执行 `done`。

一个 workflow 就是"用边把节点连起来"。

## 多节点链式执行

节点可以用 `->` 串成任意长度的链：

```juglans
[step1]: print(message="Step 1: Preparing data")
[step2]: print(message="Step 2: Processing")
[step3]: print(message="Step 3: Formatting output")
[step4]: print(message="Step 4: Complete")
[step1] -> [step2] -> [step3] -> [step4]
```

执行顺序严格遵循边的方向。Juglans 在内部将所有节点和边构建成一个 **DAG（有向无环图）**，然后按**拓扑排序**决定执行顺序——简单来说，就是"先做没有依赖的事，再做有依赖的事"。

在线性链中，拓扑排序的结果就是你写的顺序：step1, step2, step3, step4。

## 添加 metadata

到目前为止的例子都能运行，但缺少描述信息。一个完整的 `.jg` 文件通常在顶部声明 **metadata**：

```juglans
name: "My First Workflow"
version: "0.1.0"

entry: [greet]
exit: [done]

[greet]: print(message="Hello, Juglans!")
[log]: print(message="Workflow is running...")
[done]: print(message="Goodbye!")

[greet] -> [log] -> [done]
```

逐项解释：

| 字段 | 作用 |
|------|------|
| `name` | Workflow 的名称，用于展示和检索 |
| `version` | 版本号，用于追踪变更 |
| `entry` | **入口节点**——执行从这里开始 |
| `exit` | **出口节点**——执行到这里结束 |

`entry` 和 `exit` 不是必须的。省略时，Juglans 会自动推断：没有入边的节点是入口，没有出边的节点是出口。但显式声明让意图更清晰，在复杂 workflow 中尤其重要。

## 认识更多工具

`print` 适合调试，但 Juglans 内置了更多工具。以下是最常用的三个：

### print()

最简单的输出工具，将 `message` 参数的值打印到控制台。

```juglans
[hello]: print(message="Hello, World!")
```

### notify()

发送状态通知。接受 `status` 参数，用于在控制台或 UI 中显示流程进度。

```juglans
entry: [start]
exit: [done]

[start]: notify(status="Workflow started")
[process]: print(message="Processing...")
[done]: notify(status="Workflow completed")

[start] -> [process] -> [done]
```

`print` 和 `notify` 的区别：`print` 是纯文本输出，`notify` 携带语义（这是一条状态通知），在 Web UI 中会以不同样式渲染。

### set_context()

设置**上下文变量**，接受任意 `key=value` 对。变量存入上下文后，后续节点可以通过 `$ctx.key` 读取。

```juglans
entry: [start]
exit: [done]

[start]: print(message="Starting workflow")
[save]: set_context(user="Alice", score=100)
[report]: notify(status="User saved: Alice")
[done]: print(message="All done")

[start] -> [save] -> [report] -> [done]
```

`set_context` 不产生可见输出，但它改变了 workflow 的内部状态。变量系统是下一章的主题，这里只需知道 `set_context` 是"往 workflow 的记忆里写东西"。

## 组合使用

把学到的工具组合在一起：

```juglans
name: "Status Pipeline"
version: "0.1.0"

entry: [init]
exit: [finish]

[init]: notify(status="Pipeline starting...")
[setup]: set_context(stage="prepared")
[work]: print(message="Doing the real work here")
[report]: notify(status="Work complete")
[finish]: print(message="Pipeline finished")

[init] -> [setup] -> [work] -> [report] -> [finish]
```

这个 workflow 展示了一个典型模式：用 `notify` 标记关键节点，用 `set_context` 记录中间状态，用 `print` 输出调试信息。

## 常见错误

### 节点名重复

```juglans,ignore
[step]: print(message="first")
[step]: print(message="second")
[step] -> [step]
```

同一个 workflow 中两个节点使用相同的名字 `step`，解析器会报错：

```
Error: Duplicate node ID: step
```

每个节点名在整个 workflow 中必须唯一。

### 引用不存在的节点

```juglans,ignore
[start]: print(message="Hello")
[start] -> [end]
```

边 `[start] -> [end]` 引用了节点 `end`，但它从未被定义。验证器会报错：

```
Error: Edge references undefined node: end
```

所有被边引用的节点都必须先定义。先写节点，再写边——这是 `.jg` 文件的基本约定。

## 小结

本章涵盖了 Juglans workflow 的基础：

- **节点** `[name]` 是执行单元，通过 `:` 绑定工具调用
- **边** `->` 定义执行顺序
- **工具** 是节点的实际行为：`print` 输出文本，`notify` 发送通知，`set_context` 写入上下文
- **Metadata**（`name`、`version`、`entry`、`exit`）让 workflow 更完整、更易维护

下一章：[Tutorial 2: 变量与数据流]() —— 学习 `$input`、`$output`、`$ctx`，让节点之间传递数据。
