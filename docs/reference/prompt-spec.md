# .jgx Syntax Reference

Complete syntax specification for Juglans `.jgx` prompt template files.

## File Format

A `.jgx` file has two parts:

1. **Frontmatter** -- YAML-like metadata enclosed by `---` delimiters
2. **Template body** -- Jinja-style template text with interpolation and control flow

```text
---
slug: "identifier"
name: "Display Name"
inputs:
  param1: "default_value"
---
Template body with {{ param1 }} interpolation.
```

---

## Frontmatter

The frontmatter section is enclosed by two `---` lines. It defines metadata and input parameter defaults.

### Fields

| Field | Type | Required | Description |
|---|---|---|---|
| `slug` | string | **Yes** | Unique identifier for referencing this prompt |
| `name` | string | No | Display name |
| `type` | string | No | Prompt type (e.g., `"user"`, `"system"`) |
| `description` | string | No | Human-readable description |
| `inputs` | object | No | Input parameters with default values |
| `is_public` | boolean | No | Whether the prompt is publicly visible |

### Minimal Frontmatter

```jgx
---
slug: "hello"
---
Hello, world!
```

### Full Frontmatter

```jgx
---
slug: "analysis"
name: "Data Analysis Prompt"
type: "user"
description: "Analyze data and provide insights"
is_public: true
inputs:
  topic: "general"
  data: []
  format: "markdown"
---
Analyze {{ topic }} using {{ format }} format.
```

### Input Parameter Types

The `inputs` field defines parameters and their default values. Supported types:

```jgx
---
slug: "typed-inputs"
inputs:
  name: "World"
  count: 10
  rate: 0.95
  enabled: true
  tags: ["ai", "ml"]
  config: {"model": "gpt-4o-mini", "temperature": 0.7}
---
Hello {{ name }}, count={{ count }}.
```

**Object syntax for inputs:**

```jgx
---
slug: "obj-inputs"
inputs: {name: "Alice", score: 100}
---
Player: {{ name }}, Score: {{ score }}.
```

---

## Template Syntax

### Variable Interpolation

Use `{{ expression }}` to insert values:

```jgx
---
slug: "interpolation-demo"
inputs:
  name: "Alice"
  role: "analyst"
---
Hello, {{ name }}! You are a {{ role }}.
```

Nested object access:

```jgx
---
slug: "nested-access"
inputs:
  user: {"name": "Bob", "level": 5}
---
User: {{ user.name }}, Level: {{ user.level }}.
```

### Expressions in Interpolation

Interpolation supports expressions, not just variable names:

```jgx
---
slug: "expr-demo"
inputs:
  price: 19.99
  quantity: 3
---
Total: {{ price * quantity }}.
```

---

## Conditional Statements

### if / endif

```jgx
---
slug: "if-demo"
inputs:
  premium: true
---
{% if premium %}
Welcome, premium user!
{% endif %}
Regular content here.
```

### if / else / endif

```jgx
---
slug: "if-else-demo"
inputs:
  logged_in: false
---
{% if logged_in %}
Welcome back!
{% else %}
Please log in.
{% endif %}
```

### if / elif / else / endif

```jgx
---
slug: "elif-demo"
inputs:
  score: 85
---
{% if score >= 90 %}
Grade: A
{% elif score >= 80 %}
Grade: B
{% elif score >= 70 %}
Grade: C
{% else %}
Grade: F
{% endif %}
```

### Comparison Operators

| Operator | Description |
|---|---|
| `==` | Equal to |
| `!=` | Not equal to |
| `>` | Greater than |
| `<` | Less than |
| `>=` | Greater than or equal to |
| `<=` | Less than or equal to |

### Logical Operators

| Operator | Description |
|---|---|
| `&&` | Logical AND |
| `\|\|` | Logical OR |
| `!` | Logical NOT |

```jgx
---
slug: "logical-demo"
inputs:
  admin: true
  active: true
---
{% if admin && active %}
Full access granted.
{% endif %}
```

---

## Loop Statements

### for / endfor

```jgx
---
slug: "for-demo"
inputs:
  items: ["apple", "banana", "cherry"]
---
Shopping list:
{% for item in items %}
- {{ item }}
{% endfor %}
```

### for / else / endfor

The `else` block renders when the collection is empty:

```jgx
---
slug: "for-else-demo"
inputs:
  results: []
---
Results:
{% for r in results %}
- {{ r }}
{% else %}
No results found.
{% endfor %}
```

### Iterating Over Objects

```jgx
---
slug: "for-objects"
inputs:
  users: [{"name": "Alice", "role": "admin"}, {"name": "Bob", "role": "user"}]
---
Team:
{% for u in users %}
- {{ u.name }} ({{ u.role }})
{% endfor %}
```

### Nested Loops

```jgx
---
slug: "nested-loops"
inputs:
  categories: [{"name": "Fruit", "items": ["apple", "banana"]}, {"name": "Veggie", "items": ["carrot"]}]
---
{% for cat in categories %}
{{ cat.name }}:
{% for item in cat.items %}
  - {{ item }}
{% endfor %}
{% endfor %}
```

---

## Filters and Functions

Template expressions are evaluated by the same engine as workflow expressions. The pipe operator and plain function calls are two notations for the same thing — both resolve to entries in the expression-function catalog:

```text
{{ value | upper }}       ≡   {{ upper(value) }}
{{ text | replace("a", "b") }}  ≡   {{ replace(text, "a", "b") }}
{{ items | sort | join(", ") }} ≡   {{ join(sort(items), ", ") }}
```

Use whichever reads better in context. Chained pipes are commonly clearer for a sequence of string / collection transforms; plain function calls are clearer when arguments surround the value.

The full catalog of 80+ functions (strings, numbers, collections, dates, encoding, higher-order `map`/`filter`/`reduce`, etc.) is documented in **[expressions.md](./expressions.md)** — everything listed there works identically inside `.jgx` templates.

### Filter Examples

```jgx
---
slug: "filter-demo"
inputs:
  name: "alice"
  price: 19.567
  tags: ["ai", "ml", "data"]
  text: "Hello World"
---
Name: {{ name | upper }}
Price: {{ price | round(2) }}
Tags: {{ tags | join(", ") }}
Truncated: {{ text | truncate(5) }}
Default: {{ missing | default("N/A") }}
```

Chained filters:

```jgx
---
slug: "chain-filter"
inputs:
  items: ["Banana", "apple", "Cherry"]
---
{{ items | sort | join(", ") }}
```

---

## Complete Examples

### Greeting Prompt

```jgx
---
slug: "greeting"
name: "Greeting Prompt"
inputs:
  name: "World"
  language: "English"
---
Hello, {{ name }}!

{% if language == "Chinese" %}
Please respond in Chinese.
{% else %}
Please respond in {{ language }}.
{% endif %}
```

### Data Analysis Prompt

```jgx
---
slug: "data-analysis"
name: "Data Analysis Prompt"
type: "user"
description: "Analyze data with configurable focus"
inputs: {data: [], focus: "trends"}
---
Please analyze the following data and focus on {{ focus }}:

{% for item in data %}
- {{ item.name }}: {{ item.value }}
{% endfor %}

Provide:
1. Key findings
2. Trends and patterns
3. Recommendations
```

### Few-Shot Prompt

```jgx
---
slug: "few-shot"
name: "Few-Shot Classification"
inputs:
  examples: [{"input": "I love this!", "output": "positive"}, {"input": "Terrible.", "output": "negative"}]
  query: "Not bad at all"
---
Classify the sentiment of the input text.

Examples:
{% for ex in examples %}
Input: {{ ex.input }}
Output: {{ ex.output }}
{% endfor %}

Now classify:
Input: {{ query }}
Output:
```

### Code Generation Prompt

```jgx
---
slug: "codegen"
name: "Code Generator"
inputs:
  language: "python"
  task: "sort a list"
  requirements: ["handle empty input", "return new list"]
---
You are an expert {{ language }} developer.

Task: {{ task }}

Requirements:
{% for req in requirements %}
- {{ req }}
{% endfor %}

{% if language == "python" %}
Follow PEP 8 style guidelines.
{% endif %}

Provide only the code in a fenced code block.
```

---

## Usage in Workflows

Import prompt files with the `prompts:` metadata, then call with `p()`.

### Basic Rendering

```juglans
prompts: ["./prompts/*.jgx"]

[render]: p(slug="greeting", name="Alice")
```

### Passing Variables

```juglans
prompts: ["./prompts/*.jgx"]

[render]: p(
  slug="data-analysis",
  data=input.data,
  focus="anomalies"
)
```

### Combined with chat()

```juglans
prompts: ["./prompts/*.jgx"]

[analyst]: { "model": "gpt-4o-mini", "system_prompt": "You are a data analyst." }

[render]: p(slug="data-analysis", data=input.data, focus=input.focus)
[respond]: chat(agent=analyst, message=output)

[analyst] -> [render] -> [respond]
```

### Inline p() in chat()

The `p()` call can be used directly as a parameter value:

```juglans
prompts: ["./prompts/*.jgx"]

[assistant]: { "model": "gpt-4o-mini", "system_prompt": "You are a helpful assistant." }

[ask]: chat(
  agent=assistant,
  message=p(slug="greeting", name=input.user)
)
[assistant] -> [ask]
```

---

## CLI Usage

```bash
# Render with default values
juglans prompt.jgx

# Render with custom input
juglans prompt.jgx --input '{"name": "Alice", "score": 95}'

# Validate syntax
juglans check prompt.jgx
```
