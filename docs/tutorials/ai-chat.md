# Tutorial 6: AI Chat

本章学习如何在 workflow 中调用 AI 模型：使用 `chat()` 工具发送消息、创建 `.jgagent` 配置文件、构造动态消息、串联多轮对话，以及获取结构化 JSON 输出。

## 6.1 chat() 基础

最简单的 AI 调用——一个节点、一句话、一个回复：

```juglans
agents: ["./agents/*.jgagent"]

[ask]: chat(agent="assistant", message="What is 2+2?")
[show]: print(message=$output)

[ask] -> [show]
```

逐行解释：

1. `agents: ["./agents/*.jgagent"]` — 元数据声明，告诉引擎从 `./agents/` 目录加载所有 `.jgagent` 配置文件。只有加载了 agent，`chat()` 才能找到它。
2. `[ask]: chat(agent="assistant", message="What is 2+2?")` — 调用 `chat()` 工具，向 slug 为 `"assistant"` 的 agent 发送消息 `"What is 2+2?"`。引擎将消息发送给对应模型，等待回复。
3. `[show]: print(message=$output)` — AI 的回复存入 `$output`，`print` 将其打印到控制台。
4. `[ask] -> [show]` — 先调用 AI，再打印结果。

`chat()` 是 juglans 最重要的内置工具。它的最小用法只需要两个参数：

| 参数 | 作用 |
|------|------|
| `agent` | agent 的 slug，对应 `.jgagent` 文件中的 `slug` 字段 |
| `message` | 发送给 AI 的消息内容 |

## 6.2 创建 Agent — .jgagent 文件

上一节的 `chat(agent="assistant", ...)` 引用了一个名为 `assistant` 的 agent。这个 agent 定义在 `.jgagent` 文件中。

创建 `agents/assistant.jgagent`：

```jgagent
slug: "assistant"
name: "General Assistant"
model: "deepseek-chat"
temperature: 0.7
system_prompt: "You are a helpful assistant."
```

逐字段解释：

| 字段 | 作用 | 示例值 |
|------|------|--------|
| `slug` | 唯一标识符，`chat()` 中通过它引用 agent | `"assistant"` |
| `name` | 显示名称，用于 UI 和日志 | `"General Assistant"` |
| `model` | 使用的 AI 模型 | `"deepseek-chat"`, `"gpt-4o"`, `"claude-3-sonnet"` |
| `temperature` | 随机性控制（0 = 确定性，2 = 高随机） | `0.7`（推荐默认值） |
| `system_prompt` | 系统提示词，定义 agent 的角色和行为 | `"You are a helpful assistant."` |

### Temperature 选择指南

| 值 | 适用场景 |
|----|----------|
| `0.0` | 分类、提取、JSON 输出——需要一致性 |
| `0.3` | 代码生成、技术问答——准确优先 |
| `0.7` | 通用对话——平衡创造力与准确性 |
| `1.0+` | 创意写作、头脑风暴——鼓励多样性 |

### 目录结构

```text
my-project/
├── chat.jg
└── agents/
    └── assistant.jgagent
```

`agents:` 元数据中的路径相对于 `.jg` 文件所在目录。`["./agents/*.jgagent"]` 会匹配 `agents/` 下所有 `.jgagent` 文件。

## 6.3 动态消息 — 用 $input 构造

硬编码消息只适合测试。实际场景中，消息来自外部输入：

```juglans
agents: ["./agents/*.jgagent"]

[ask]: chat(agent="assistant", message=$input.question)
[result]: print(message=$output)

[ask] -> [result]
```

运行：

```bash
juglans chat.jg --input '{"question": "Explain recursion in one sentence."}'
```

`$input.question` 在执行时被替换为 `"Explain recursion in one sentence."`，AI 收到这条消息并回复。

### 拼接上下文

用 `+` 拼接字符串，为 AI 提供更多上下文：

```juglans
agents: ["./agents/*.jgagent"]

[init]: set_context(lang=$input.lang)
[ask]: chat(agent="assistant", message="Answer in " + $ctx.lang + ": " + $input.question)
[show]: print(message=$output)

[init] -> [ask] -> [show]
```

```bash
juglans chat.jg --input '{"question": "What is Rust?", "lang": "Chinese"}'
```

AI 收到的消息是 `"Answer in Chinese: What is Rust?"`，因此会用中文回答。

## 6.4 多轮对话 — 链式 chat()

将多个 `chat()` 节点串联，前一个的输出作为后一个的输入。这是 AI 工作流最常见的模式：

```juglans
agents: ["./agents/*.jgagent"]

[draft]: chat(agent="assistant", message="Write a short poem about the sea.")
[review]: chat(agent="assistant", message="Review this poem and suggest improvements: " + $output)
[final]: print(message=$output)

[draft] -> [review] -> [final]
```

执行流程：

1. `[draft]` — AI 写一首关于大海的短诗，结果存入 `$output`。
2. `[review]` — 读取 `$output`（上一步的诗），请 AI 审阅并改进。AI 的改进建议覆盖 `$output`。
3. `[final]` — 打印最终的审阅结果。

### 保存中间结果

如果后续还需要原始诗歌，用 `$ctx` 保存：

```juglans
agents: ["./agents/*.jgagent"]

[draft]: chat(agent="assistant", message="Write a haiku about mountains.")
[save]: set_context(poem=$output)
[review]: chat(agent="assistant", message="Critique this haiku: " + $ctx.poem)
[show]: print(message="Original: " + $ctx.poem + " | Review: " + $output)

[draft] -> [save] -> [review] -> [show]
```

`$ctx.poem` 在整个 workflow 中持久存在，不会被后续 `$output` 覆盖。

### 使用不同 Agent

链中的每个节点可以使用不同的 agent，发挥各自专长：

```juglans
agents: ["./agents/*.jgagent"]

[translate]: chat(agent="translator", message="Translate to English: " + $input.text)
[summarize]: chat(agent="summarizer", message="Summarize in one sentence: " + $output)
[result]: print(message=$output)

[translate] -> [summarize] -> [result]
```

先翻译，再摘要——两个 agent 各司其职。

## 6.5 JSON 格式输出 — format="json"

默认情况下，`chat()` 返回自由文本。添加 `format="json"` 参数可以强制 AI 返回结构化 JSON：

```juglans
agents: ["./agents/*.jgagent"]

[analyze]: chat(agent="assistant", message="Analyze the sentiment: " + $input.text, format="json")
[show]: print(message=$output)

[analyze] -> [show]
```

```bash
juglans analyze.jg --input '{"text": "I love this product!"}'
```

AI 返回的 JSON（示例）：

```json
{"sentiment": "positive", "confidence": 0.95}
```

`format="json"` 的作用：

- 向模型添加 JSON 输出约束（response_format）
- 返回值是一个 JSON 对象，可以用 `$output.sentiment` 等路径访问内部字段

### JSON 输出 + 条件路由

JSON 输出最强大的用法是与条件路由结合，让 AI 做决策：

```juglans
agents: ["./agents/*.jgagent"]

[classify]: chat(agent="assistant", message="Classify this as positive or negative: " + $input.text, format="json")
[pos]: print(message="Positive feedback detected!")
[neg]: print(message="Negative feedback — escalating.")
[done]: print(message="Classification complete.")

[classify] if $output.sentiment == "positive" -> [pos]
[classify] if $output.sentiment == "negative" -> [neg]
[classify] -> [done]
[pos] -> [done]
[neg] -> [done]
```

AI 返回 `{"sentiment": "positive"}`，workflow 根据 `$output.sentiment` 自动路由到 `[pos]` 或 `[neg]`。

## 6.6 配置说明

`chat()` 通过 Jug0 后端服务与 AI 模型通信。要让 `chat()` 正常工作，需要在项目根目录创建 `juglans.toml`：

```toml
[account]
id = "your_user_id"
api_key = "jug0_sk_..."

[jug0]
base_url = "http://localhost:3000"
```

| 字段 | 作用 |
|------|------|
| `account.id` | Jug0 账户 ID |
| `account.api_key` | API 密钥 |
| `jug0.base_url` | Jug0 服务地址 |

没有此配置，`chat()` 会返回连接错误。详见 [Jug0 集成指南](../integrations/jug0.md)。

## 小结

| 概念 | 语法 | 作用 |
|------|------|------|
| AI 调用 | `chat(agent="slug", message=...)` | 向 agent 发送消息，获取回复 |
| Agent 配置 | `.jgagent` 文件 | 定义模型、温度、系统提示词 |
| 加载 Agent | `agents: ["./agents/*.jgagent"]` | 在 workflow 中引入 agent 文件 |
| 动态消息 | `message=$input.question` | 用变量构造消息内容 |
| 多轮链式 | `[a] -> [b]` 多个 chat 节点串联 | 前一个输出作为后一个输入 |
| JSON 输出 | `format="json"` | 强制结构化输出，可与条件路由结合 |

关键规则：

1. 使用 `chat()` 前必须通过 `agents:` 元数据加载 agent 文件。
2. `chat()` 的返回值存入 `$output`，下一个节点可以直接读取。
3. `format="json"` 让 AI 返回 JSON 对象，字段可通过 `$output.field` 访问。
4. 多个 `chat()` 节点可以使用相同或不同的 agent。

## 下一章

**[Tutorial 7: Prompt Templates](./prompt-templates.md)** — 学习 `.jgprompt` 模板语法和 `p()` 工具，用 Jinja 风格的模板管理复杂提示词。
