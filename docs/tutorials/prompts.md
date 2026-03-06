# Tutorial 7: Prompt Templates

本章学习如何用 `.jgprompt` 文件管理提示词模板，以及在 workflow 中用 `p()` 工具渲染它们——让提示词可复用、可参数化、可维护。

## 7.1 为什么需要 Prompt 模板

先看一个硬编码 prompt 的 workflow：

```juglans
agents: ["./agents/*.jgagent"]

[ask]: chat(agent="assistant", message="You are a senior code reviewer. Please review the following code and provide feedback on: 1) correctness 2) performance 3) readability. Code: " + $input.code)
[show]: print(message=$output)

[ask] -> [show]
```

这能工作，但有三个问题：

1. **不可复用** — 另一个 workflow 要用同样的 prompt，只能复制粘贴。
2. **难以维护** — 修改 prompt 需要打开 `.jg` 文件，在长字符串里找位置改。
3. **无法测试** — 你不能单独验证 prompt 的渲染结果。

解决方案：把 prompt 抽到 `.jgprompt` 文件，用 `p()` 工具渲染。

## 7.2 .jgprompt 文件结构

创建 `prompts/greeting.jgprompt`：

```jgprompt
---
slug: "greeting"
name: "Greeting Prompt"
inputs:
  name: "World"
  style: "formal"
---
Hello, {{ name }}!

{% if style == "casual" %}
What's up?
{% else %}
How may I assist you today?
{% endif %}
```

文件分两部分：

### Frontmatter（`---` 之间）

| 字段 | 作用 | 必填 |
|------|------|------|
| `slug` | 唯一标识符，`p()` 通过它引用模板 | 是 |
| `name` | 显示名称，用于 UI 和日志 | 是 |
| `inputs` | 输入变量及其默认值 | 否 |

### Body（第二个 `---` 之后）

模板正文，支持 Jinja 风格的模板语法：`{{ }}` 插值、`{% %}` 逻辑控制。

### 目录结构

```text
my-project/
├── app.jg
└── prompts/
    ├── greeting.jgprompt
    └── review.jgprompt
```

## 7.3 在 workflow 中使用 p()

用 `p()` 工具渲染模板，用 `prompts:` 元数据加载文件：

```juglans
prompts: ["./prompts/*.jgprompt"]

[render]: p(slug="greeting", name="Alice", style="casual")
[show]: print(message=$output)

[render] -> [show]
```

逐行解释：

1. `prompts: ["./prompts/*.jgprompt"]` — 加载 `prompts/` 目录下所有 `.jgprompt` 文件到 PromptRegistry。
2. `p(slug="greeting", name="Alice", style="casual")` — 渲染 slug 为 `"greeting"` 的模板，传入 `name="Alice"` 和 `style="casual"`。
3. `$output` — 存储渲染后的纯文本结果。

`p()` 的参数规则：

| 参数 | 作用 |
|------|------|
| `slug` | 必填。对应 `.jgprompt` 文件中的 `slug` 字段 |
| 其他 key=value | 对应模板中的变量，覆盖 `inputs` 的默认值 |

上面的例子会输出：

```text
Hello, Alice!

What's up?
```

因为 `style="casual"` 触发了 `{% if style == "casual" %}` 分支。

## 7.4 变量和默认值

`inputs` 中定义的值是默认值。调用 `p()` 时，传入的参数会覆盖默认值；未传入的参数使用默认值。

创建 `prompts/welcome.jgprompt`：

```jgprompt
---
slug: "welcome"
name: "Welcome Message"
inputs:
  name: "Guest"
  role: "user"
  lang: "English"
---
Welcome, {{ name }}!
Your role: {{ role }}
Language: {{ lang }}
```

在 workflow 中只覆盖部分变量：

```juglans
prompts: ["./prompts/*.jgprompt"]

[full]: p(slug="welcome", name="Alice", role="admin", lang="Chinese")
[show1]: print(message=$output)

[partial]: p(slug="welcome", name="Bob")
[show2]: print(message=$output)

[full] -> [show1] -> [partial] -> [show2]
```

`[full]` 覆盖了所有三个变量。`[partial]` 只传入 `name="Bob"`，`role` 和 `lang` 使用默认值 `"user"` 和 `"English"`。

### 用 $input 传递动态值

变量值可以来自 workflow 的输入：

```juglans
prompts: ["./prompts/*.jgprompt"]

[render]: p(slug="welcome", name=$input.name, role=$input.role)
[show]: print(message=$output)

[render] -> [show]
```

```bash
juglans app.jg --input '{"name": "Alice", "role": "admin"}'
```

`$input.name` 和 `$input.role` 在执行时被解析为实际值，传入模板渲染。

## 7.5 条件渲染

用 `{% if %}` / `{% elif %}` / `{% else %}` / `{% endif %}` 控制渲染内容。

创建 `prompts/tone.jgprompt`：

```jgprompt
---
slug: "tone"
name: "Tone-Aware Prompt"
inputs:
  topic: "AI"
  audience: "general"
---
Explain {{ topic }}.

{% if audience == "expert" %}
Use technical terminology. Assume deep domain knowledge. Focus on nuances and edge cases.
{% elif audience == "student" %}
Use simple language. Provide examples. Define key terms.
{% else %}
Use clear, accessible language. Balance depth with readability.
{% endif %}
```

在 workflow 中使用：

```juglans
prompts: ["./prompts/*.jgprompt"]

[expert]: p(slug="tone", topic="Transformer architecture", audience="expert")
[show]: print(message=$output)

[expert] -> [show]
```

`audience="expert"` 命中第一个分支，输出：

```text
Explain Transformer architecture.

Use technical terminology. Assume deep domain knowledge. Focus on nuances and edge cases.
```

### 嵌套条件

条件可以嵌套：

```jgprompt
---
slug: "format"
name: "Format Selector"
inputs:
  format: "text"
  lang: "en"
---
{% if format == "json" %}
Return a valid JSON object.
{% else %}
Return plain text.
{% if lang == "zh" %}
Use Chinese.
{% endif %}
{% endif %}
```

## 7.6 循环渲染

用 `{% for item in list %}` / `{% endfor %}` 遍历数组。

创建 `prompts/checklist.jgprompt`：

```jgprompt
---
slug: "checklist"
name: "Review Checklist"
inputs:
  items: ["correctness", "performance", "readability"]
---
Please review the following aspects:

{% for item in items %}
- {{ item }}
{% endfor %}

Provide feedback for each.
```

渲染结果：

```text
Please review the following aspects:

- correctness
- performance
- readability

Provide feedback for each.
```

### loop 变量

在 `{% for %}` 块中，`loop` 对象提供迭代元数据：

| 字段 | 类型 | 说明 |
|------|------|------|
| `loop.index` | number | 当前索引（从 1 开始） |
| `loop.index0` | number | 当前索引（从 0 开始） |
| `loop.first` | bool | 是否第一个元素 |
| `loop.last` | bool | 是否最后一个元素 |

```jgprompt
---
slug: "numbered"
name: "Numbered List"
inputs:
  steps: ["Parse input", "Validate data", "Execute workflow"]
---
{% for step in steps %}
{{ loop.index }}. {{ step }}
{% endfor %}
```

渲染结果：

```text
1. Parse input
2. Validate data
3. Execute workflow
```

## 7.7 过滤器

在 `{{ }}` 插值中使用函数对值进行转换。Juglans 模板使用函数调用语法（而非 Jinja 的管道语法）：

```jgprompt
---
slug: "filters"
name: "Filter Demo"
inputs:
  name: "alice"
  title: ""
  score: 95
---
Name: {{ upper(name) }}
Title: {{ default(title, "Untitled") }}
Score: {{ str(score) }} points
```

渲染结果（使用默认值）：

```text
Name: ALICE
Title: Untitled
Score: 95 points
```

### 常用函数

| 函数 | 说明 | 示例 |
|------|------|------|
| `upper(x)` | 转大写 | `upper("hello")` -> `"HELLO"` |
| `lower(x)` | 转小写 | `lower("HELLO")` -> `"hello"` |
| `default(x, fallback)` | 空值回退 | `default("", "N/A")` -> `"N/A"` |
| `str(x)` | 转字符串 | `str(42)` -> `"42"` |
| `len(x)` | 取长度 | `len("hello")` -> `5` |
| `replace(x, old, new)` | 替换子串 | `replace("hi all", "hi", "hello")` -> `"hello all"` |
| `join(list, sep)` | 数组拼接 | `join(["a","b"], ", ")` -> `"a, b"` |

## 7.8 配合 chat() 使用

`p()` 渲染模板 + `chat()` 发送给 AI —— 这是 juglans 中最经典的模式：

```juglans
prompts: ["./prompts/*.jgprompt"]
agents: ["./agents/*.jgagent"]

[render]: p(slug="tone", topic=$input.topic, audience=$input.audience)
[ask]: chat(agent="assistant", message=$output)
[show]: print(message=$output)

[render] -> [ask] -> [show]
```

执行流程：

1. `[render]` — `p()` 渲染模板，生成完整的 prompt 文本，存入 `$output`。
2. `[ask]` — `chat()` 读取 `$output` 作为 `message`，发送给 AI。
3. `[show]` — 打印 AI 的回复。

### 内联用法

`p()` 也可以直接作为 `chat()` 的 `message` 参数值：

```juglans
prompts: ["./prompts/*.jgprompt"]
agents: ["./agents/*.jgagent"]

[ask]: chat(agent="assistant", message=p(slug="tone", topic=$input.topic, audience="expert"))
[show]: print(message=$output)

[ask] -> [show]
```

这将 `p()` 的渲染结果直接传给 `chat()`，省去一个中间节点。

### 多模板串联

复杂场景中，可以用多个模板组合 prompt：

```juglans
prompts: ["./prompts/*.jgprompt"]
agents: ["./agents/*.jgagent"]

[system]: p(slug="tone", topic=$input.topic, audience="expert")
[save_sys]: set_context(system_prompt=$output)
[user_msg]: p(slug="checklist")
[ask]: chat(agent="assistant", message=$ctx.system_prompt + "\n\n" + $output)
[show]: print(message=$output)

[system] -> [save_sys] -> [user_msg] -> [ask] -> [show]
```

`[system]` 渲染角色设定模板，`[user_msg]` 渲染检查清单模板，两者拼接后发送给 AI。

## 小结

| 概念 | 语法 | 作用 |
|------|------|------|
| Prompt 文件 | `.jgprompt` | 可复用的模板文件，frontmatter + body |
| 加载模板 | `prompts: ["./prompts/*.jgprompt"]` | 在 workflow 中引入模板文件 |
| 渲染模板 | `p(slug="name", key=value)` | 渲染指定模板，传入参数 |
| 变量插值 | `{{ variable }}` | 输出变量值 |
| 条件渲染 | `{% if %}` / `{% elif %}` / `{% else %}` / `{% endif %}` | 按条件选择内容 |
| 循环渲染 | `{% for item in list %}` / `{% endfor %}` | 遍历数组生成内容 |
| 函数调用 | `{{ upper(name) }}` | 转换变量值 |
| 默认值 | `inputs` 中的值 | 未传参数时的回退值 |

关键规则：

1. 使用 `p()` 前必须通过 `prompts:` 元数据加载模板文件。
2. `slug` 参数必填，其他参数对应模板中的变量。
3. `inputs` 中的值是默认值，`p()` 传入的参数会覆盖它们。
4. `p()` 返回渲染后的纯文本字符串，存入 `$output`。
5. `p()` + `chat()` 是最常用的组合：先渲染 prompt，再发送给 AI。

## 下一章

**[Tutorial 8: Workflow Composition](./composition.md)** — 学习 `flows:` 导入和 `libs:` 库引用，将多个 workflow 组合成复杂的执行图。
