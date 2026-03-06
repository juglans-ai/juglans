# Prompt Template Syntax (.jgprompt)

`.jgprompt` files define reusable Prompt templates with support for variable interpolation and control flow.

## File Structure

```jgprompt
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

## Frontmatter

Define metadata using YAML format, enclosed by `---`:

| Field | Type | Required | Description |
|------|------|------|------|
| `slug` | string | Yes | Unique identifier, used for referencing |
| `name` | string | No | Display name |
| `type` | string | No | Type, defaults to "prompt" |
| `description` | string | No | Description |
| `inputs` | object | No | Input parameters and default values |

### Input Parameters

```jgprompt
---
slug: "analysis"
inputs:
  # Simple types
  name: "Default Name"
  count: 10
  enabled: true

  # Nested objects
  config:
    model: "gpt-4"
    temperature: 0.7

  # Arrays
  tags: ["tag1", "tag2"]
---
```

## Template Syntax

### Variable Interpolation

Use `{{ }}` to insert variables:

```
Hello, {{ name }}!

Your score is {{ score }}.

Config: {{ config.model }} at {{ config.temperature }}
```

### Filters

Apply transformations to variables:

```
# Rounding
{{ price | round(2) }}

# Truncate text
{{ description | truncate(100) }}

# Case conversion
{{ name | upper }}
{{ name | lower }}

# Default value
{{ optional_field | default("N/A") }}

# JSON formatting
{{ data | json }}
```

### Conditional Statements

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

### Comparison Operators

| Operator | Description |
|--------|------|
| `==` | Equal to |
| `!=` | Not equal to |
| `>` | Greater than |
| `<` | Less than |
| `>=` | Greater than or equal to |
| `<=` | Less than or equal to |

### Logical Operators

```
{% if a && b %}        # AND
{% if a || b %}        # OR
{% if !condition %}    # NOT
```

### Loop Statements

```
{% for item in items %}
  - {{ item.name }}: {{ item.value }}
{% endfor %}

# With else (when the list is empty)
{% for item in items %}
  - {{ item }}
{% else %}
  No items found.
{% endfor %}
```

### Loop Context Variables

```
{% for item in items %}
  {{ loop.index }}: {{ item }}       # Index (1-based, starting from 1)
  {{ loop.index0 }}: {{ item }}      # Index (0-based, starting from 0)
  {% if loop.first %}(First!){% endif %}
  {% if loop.last %}(Last!){% endif %}
{% endfor %}
```

## Complete Examples

### Simple Prompt

```jgprompt
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

### Analysis Prompt

```jgprompt
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

### Multi-Turn Conversation Prompt

```jgprompt
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

### Code Generation Prompt

```jgprompt
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

### Few-Shot Prompt

```jgprompt
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

## Usage in Workflows

### Basic Call

```juglans
[render]: p(slug="greeting", name="Alice")
```

### Passing Variables

```juglans
[render]: p(
  slug="analyze-data",
  topic=$input.topic,
  data=$ctx.collected_data,
  format="json"
)
```

### Combined with Chat

```juglans
[prompt]: p(slug="analysis", data=$input)
[chat]: chat(agent="analyst", message=$output)

[prompt] -> [chat]
```

## Best Practices

1. **Define inputs clearly** - Define all parameters and default values in `inputs`
2. **Structured output** - Use Markdown structure to organize long Prompts
3. **Conditional logic** - Use conditional statements to handle different scenarios
4. **Testable** - Provide reasonable default values for easy testing
5. **Document** - Use `description` to explain the purpose

## Debugging Tips

### Local Rendering Test

```bash
# Render using default values
juglans src/prompts/my-prompt.jgprompt

# Pass custom input
juglans src/prompts/my-prompt.jgprompt --input '{
  "name": "Test User",
  "score": 95
}'
```

### View Rendered Result

```bash
# Output rendered text only, without executing
juglans src/prompts/my-prompt.jgprompt --dry-run
```
