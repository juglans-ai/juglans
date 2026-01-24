# Prompt 模板语法 (.jgprompt)

`.jgprompt` 文件定义可复用的 Prompt 模板，支持变量插值和控制流。

## 文件结构

```yaml
---
slug: "prompt_identifier"
name: "Display Name"
type: "prompt"
description: "Optional description"
inputs:
  param1: "default_value"
  param2: 42
---
Template content goes here...
{{ variable }}
```

## 前置元数据 (Frontmatter)

使用 YAML 格式定义元数据，用 `---` 包围：

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `slug` | string | 是 | 唯一标识符，用于引用 |
| `name` | string | 否 | 显示名称 |
| `type` | string | 否 | 类型，默认 "prompt" |
| `description` | string | 否 | 描述说明 |
| `inputs` | object | 否 | 输入参数及默认值 |

### 输入参数

```yaml
---
slug: "analysis"
inputs:
  # 简单类型
  name: "Default Name"
  count: 10
  enabled: true

  # 嵌套对象
  config:
    model: "gpt-4"
    temperature: 0.7

  # 数组
  tags: ["tag1", "tag2"]
---
```

## 模板语法

### 变量插值

使用 `{{ }}` 插入变量：

```
Hello, {{ name }}!

Your score is {{ score }}.

Config: {{ config.model }} at {{ config.temperature }}
```

### 过滤器

对变量应用转换：

```
# 四舍五入
{{ price | round(2) }}

# 截断文本
{{ description | truncate(100) }}

# 大小写
{{ name | upper }}
{{ name | lower }}

# 默认值
{{ optional_field | default("N/A") }}

# JSON 格式化
{{ data | json }}
```

### 条件语句

```
{% if condition %}
  Content when true
{% endif %}

{% if score > 80 %}
  Excellent!
{% elif score > 60 %}
  Good job!
{% else %}
  Keep trying!
{% endif %}
```

### 比较运算符

| 运算符 | 说明 |
|--------|------|
| `==` | 等于 |
| `!=` | 不等于 |
| `>` | 大于 |
| `<` | 小于 |
| `>=` | 大于等于 |
| `<=` | 小于等于 |

### 逻辑运算符

```
{% if a && b %}        # AND
{% if a || b %}        # OR
{% if !condition %}    # NOT
```

### 循环语句

```
{% for item in items %}
  - {{ item.name }}: {{ item.value }}
{% endfor %}

# 带 else（空列表时）
{% for item in items %}
  - {{ item }}
{% else %}
  No items found.
{% endfor %}
```

### 循环上下文变量

```
{% for item in items %}
  {{ loop.index }}: {{ item }}       # 索引 (1-based，从 1 开始)
  {{ loop.index0 }}: {{ item }}      # 索引 (0-based，从 0 开始)
  {% if loop.first %}(First!){% endif %}
  {% if loop.last %}(Last!){% endif %}
{% endfor %}
```

## 完整示例

### 简单 Prompt

```yaml
---
slug: "greeting"
name: "Greeting"
inputs:
  name: "World"
  language: "English"
---
Hello, {{ name }}!

Please respond in {{ language }}.
```

### 分析 Prompt

```yaml
---
slug: "analyze-data"
name: "Data Analysis Prompt"
description: "Structured data analysis template"
inputs:
  topic: ""
  data: {}
  format: "markdown"
  include_charts: false
---
You are a professional data analyst.

## Task
Analyze the following topic: {{ topic }}

## Data
```json
{{ data | json }}
```

## Requirements
- Output format: {{ format }}
{% if include_charts %}
- Include relevant charts and visualizations
{% endif %}
- Be concise and insightful
- Highlight key trends and anomalies

## Output Structure
1. Executive Summary
2. Key Findings
3. Detailed Analysis
4. Recommendations
```

### 多轮对话 Prompt

```yaml
---
slug: "conversation"
name: "Conversation Context"
inputs:
  history: []
  current_message: ""
  persona: "helpful assistant"
---
You are a {{ persona }}.

## Conversation History
{% for msg in history %}
{{ msg.role }}: {{ msg.content }}
{% else %}
(No previous messages)
{% endfor %}

## Current Message
User: {{ current_message }}

## Instructions
- Respond naturally and helpfully
- Reference previous context when relevant
- Ask clarifying questions if needed
```

### 代码生成 Prompt

```yaml
---
slug: "code-generator"
name: "Code Generator"
inputs:
  language: "python"
  task: ""
  requirements: []
  style: "clean"
---
You are an expert {{ language }} developer.

## Task
{{ task }}

## Requirements
{% for req in requirements %}
- {{ req }}
{% endfor %}

## Code Style
- Style: {{ style }}
{% if style == "clean" %}
- Use meaningful variable names
- Add docstrings and comments
- Follow PEP 8 (if Python)
{% elif style == "minimal" %}
- Write concise code
- Minimal comments
{% endif %}

## Output
Provide only the code, wrapped in appropriate markdown code blocks.
```

### 带示例的 Prompt

```yaml
---
slug: "few-shot"
name: "Few-Shot Learning"
inputs:
  examples: []
  query: ""
  task_description: "Classify the input"
---
{{ task_description }}

## Examples
{% for ex in examples %}
Input: {{ ex.input }}
Output: {{ ex.output }}

{% endfor %}

## Your Turn
Input: {{ query }}
Output:
```

## 在工作流中使用

### 基本调用

```yaml
[render]: p(slug="greeting", name="Alice")
```

### 传递变量

```yaml
[render]: p(
  slug="analyze-data",
  topic=$input.topic,
  data=$ctx.collected_data,
  format="json"
)
```

### 结合 Chat

```yaml
[prompt]: p(slug="analysis", data=$input)
[chat]: chat(agent="analyst", message=$output)

[prompt] -> [chat]
```

## 最佳实践

1. **明确输入** - 在 `inputs` 中定义所有参数和默认值
2. **结构化输出** - 使用 Markdown 结构组织长 Prompt
3. **条件逻辑** - 用条件语句处理不同场景
4. **可测试** - 提供合理的默认值便于测试
5. **文档化** - 使用 `description` 说明用途

## 调试技巧

### 本地渲染测试

```bash
# 使用默认值渲染
juglans prompts/my-prompt.jgprompt

# 传入自定义输入
juglans prompts/my-prompt.jgprompt --input '{
  "name": "Test User",
  "score": 95
}'
```

### 查看渲染结果

```bash
# 只输出渲染后的文本，不执行
juglans prompts/my-prompt.jgprompt --dry-run
```
