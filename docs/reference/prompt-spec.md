# .jgprompt Syntax Reference

Complete syntax specification for Juglans `.jgprompt` prompt template files.

## File Format

A `.jgprompt` file has two parts:

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

```jgprompt
---
slug: "hello"
---
Hello, world!
```

### Full Frontmatter

```jgprompt
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

```jgprompt
---
slug: "typed-inputs"
inputs:
  name: "World"
  count: 10
  rate: 0.95
  enabled: true
  tags: ["ai", "ml"]
  config: {"model": "gpt-4", "temperature": 0.7}
---
Hello {{ name }}, count={{ count }}.
```

**Object syntax for inputs:**

```jgprompt
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

```jgprompt
---
slug: "interpolation-demo"
inputs:
  name: "Alice"
  role: "analyst"
---
Hello, {{ name }}! You are a {{ role }}.
```

Nested object access:

```jgprompt
---
slug: "nested-access"
inputs:
  user: {"name": "Bob", "level": 5}
---
User: {{ user.name }}, Level: {{ user.level }}.
```

### Expressions in Interpolation

Interpolation supports expressions, not just variable names:

```jgprompt
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

```jgprompt
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

```jgprompt
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

```jgprompt
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

```jgprompt
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

```jgprompt
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

```jgprompt
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

```jgprompt
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

```jgprompt
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

## Filters (Pipe Syntax)

Apply transformations using the `|` pipe operator inside `{{ }}`:

```text
{{ value | filter_name }}
{{ value | filter_name(arg1, arg2) }}
{{ value | filter1 | filter2 }}
```

### Available Filters

#### String Filters

| Filter | Description | Example |
|---|---|---|
| `upper` | Uppercase | `{{ name \| upper }}` |
| `lower` | Lowercase | `{{ name \| lower }}` |
| `capitalize` | Capitalize first letter | `{{ name \| capitalize }}` |
| `title` | Title case | `{{ name \| title }}` |
| `strip` / `trim` | Remove leading/trailing whitespace | `{{ text \| strip }}` |
| `truncate(n)` | Truncate to n characters | `{{ text \| truncate(100) }}` |
| `replace(old, new)` | Replace substring | `{{ text \| replace("a", "b") }}` |
| `split(sep)` | Split into array | `{{ text \| split(",") }}` |
| `lpad(n, ch)` | Left-pad to length n | `{{ num \| lpad(5, "0") }}` |
| `rpad(n, ch)` | Right-pad to length n | `{{ name \| rpad(20, ".") }}` |
| `repeat(n)` | Repeat n times | `{{ "-" \| repeat(40) }}` |

#### Numeric Filters

| Filter | Description | Example |
|---|---|---|
| `round(n)` | Round to n decimal places | `{{ price \| round(2) }}` |
| `abs` | Absolute value | `{{ diff \| abs }}` |
| `floor` | Floor | `{{ val \| floor }}` |
| `ceil` | Ceiling | `{{ val \| ceil }}` |

#### Collection Filters

| Filter | Description | Example |
|---|---|---|
| `len` | Length of string/array/object | `{{ items \| len }}` |
| `join(sep)` | Join array elements | `{{ tags \| join(", ") }}` |
| `first` | First element | `{{ items \| first }}` |
| `last` | Last element | `{{ items \| last }}` |
| `sort` | Sort array | `{{ nums \| sort }}` |
| `reverse` | Reverse array/string | `{{ items \| reverse }}` |
| `unique` | Deduplicate array | `{{ items \| unique }}` |
| `flatten` | Flatten nested arrays | `{{ nested \| flatten }}` |
| `sum` | Sum numeric array | `{{ prices \| sum }}` |
| `keys` | Object keys | `{{ obj \| keys }}` |
| `values` | Object values | `{{ obj \| values }}` |

#### Type / Format Filters

| Filter | Description | Example |
|---|---|---|
| `json` | JSON serialize | `{{ data \| json }}` |
| `json_pretty` | Pretty-print JSON | `{{ data \| json_pretty }}` |
| `str` | Convert to string | `{{ num \| str }}` |
| `int` | Convert to integer | `{{ text \| int }}` |
| `float` | Convert to float | `{{ text \| float }}` |
| `default(val)` | Default if null/empty | `{{ name \| default("N/A") }}` |
| `type` | Return type name | `{{ val \| type }}` |

### Filter Examples

```jgprompt
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

```jgprompt
---
slug: "chain-filter"
inputs:
  items: ["Banana", "apple", "Cherry"]
---
{{ items | sort | join(", ") }}
```

---

## Built-in Functions

Functions can be called inside `{{ }}` expressions:

| Function | Description | Example |
|---|---|---|
| `len(x)` | Length | `{{ len(items) }}` |
| `range(n)` | Generate 0..n array | `{{ range(5) }}` |
| `str(x)` | To string | `{{ str(42) }}` |
| `int(x)` | To integer | `{{ int("42") }}` |
| `float(x)` | To float | `{{ float("3.14") }}` |
| `json(x)` | JSON serialize | `{{ json(data) }}` |
| `contains(haystack, needle)` | Membership test | `{{ contains(list, "x") }}` |
| `replace(s, old, new)` | String replace | `{{ replace(text, "a", "b") }}` |
| `upper(s)` | Uppercase | `{{ upper(name) }}` |
| `lower(s)` | Lowercase | `{{ lower(name) }}` |
| `default(val, fallback)` | Default value | `{{ default(x, "none") }}` |
| `join(arr, sep)` | Join array | `{{ join(items, ", ") }}` |
| `split(s, sep)` | Split string | `{{ split(csv, ",") }}` |
| `keys(obj)` | Object keys | `{{ keys(config) }}` |
| `values(obj)` | Object values | `{{ values(config) }}` |
| `round(n, digits)` | Round | `{{ round(3.14159, 2) }}` |
| `abs(n)` | Absolute value | `{{ abs(-5) }}` |
| `min(a, b)` | Minimum | `{{ min(x, y) }}` |
| `max(a, b)` | Maximum | `{{ max(x, y) }}` |

---

## Complete Examples

### Greeting Prompt

```jgprompt
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

```jgprompt
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

```jgprompt
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

```jgprompt
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
prompts: ["./prompts/*.jgprompt"]

entry: [render]
exit: [render]

[render]: p(slug="greeting", name="Alice")
```

### Passing Variables

```juglans
prompts: ["./prompts/*.jgprompt"]

entry: [render]
exit: [render]

[render]: p(
  slug="data-analysis",
  data=$input.data,
  focus="anomalies"
)
```

### Combined with chat()

```juglans
prompts: ["./prompts/*.jgprompt"]
agents: ["./agents/*.jgagent"]

entry: [render]
exit: [respond]

[render]: p(slug="data-analysis", data=$input.data, focus=$input.focus)
[respond]: chat(agent="analyst", message=$output)

[render] -> [respond]
```

### Inline p() in chat()

The `p()` call can be used directly as a parameter value:

```juglans
prompts: ["./prompts/*.jgprompt"]
agents: ["./agents/*.jgagent"]

entry: [ask]
exit: [ask]

[ask]: chat(
  agent="assistant",
  message=p(slug="greeting", name=$input.user)
)
```

---

## CLI Usage

```bash
# Render with default values
juglans prompt.jgprompt

# Render with custom input
juglans prompt.jgprompt --input '{"name": "Alice", "score": 95}'

# Validate syntax
juglans check prompt.jgprompt
```
