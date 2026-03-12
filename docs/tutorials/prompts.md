# Tutorial 7: Prompt Templates

This chapter covers how to manage prompt templates using `.jgprompt` files, and how to render them in workflows using the `p()` tool — making prompts reusable, parameterizable, and maintainable.

## 7.1 Why Prompt Templates

Consider a workflow with a hardcoded prompt:

```juglans
agents: ["./agents/*.jgagent"]

[ask]: chat(agent="assistant", message="You are a senior code reviewer. Please review the following code and provide feedback on: 1) correctness 2) performance 3) readability. Code: " + $input.code)
[show]: print(message=$output)

[ask] -> [show]
```

This works, but has three problems:

1. **Not reusable** — If another workflow needs the same prompt, you can only copy and paste it.
2. **Hard to maintain** — Modifying the prompt requires opening the `.jg` file and finding the right place within a long string.
3. **Cannot be tested** — You cannot independently verify the prompt's rendering result.

The solution: extract the prompt into a `.jgprompt` file and render it with the `p()` tool.

## 7.2 .jgprompt File Structure

Create `prompts/greeting.jgprompt`:

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

The file has two parts:

### Frontmatter (between the `---` markers)

| Field | Purpose | Required |
|------|------|------|
| `slug` | Unique identifier; `p()` references the template by this value | Yes |
| `name` | Display name, used in UI and logs | Yes |
| `inputs` | Input variables and their default values | No |

### Body (after the second `---`)

The template body, supporting Jinja-style template syntax: `{{ }}` for interpolation, `{% %}` for control logic.

### Directory Structure

```text
my-project/
├── app.jg
└── prompts/
    ├── greeting.jgprompt
    └── review.jgprompt
```

## 7.3 Using p() in Workflows

Use the `p()` tool to render templates, and `prompts:` metadata to load files:

```juglans
prompts: ["./prompts/*.jgprompt"]

[render]: p(slug="greeting", name="Alice", style="casual")
[show]: print(message=$output)

[render] -> [show]
```

Line-by-line explanation:

1. `prompts: ["./prompts/*.jgprompt"]` — Loads all `.jgprompt` files from the `prompts/` directory into the PromptRegistry.
2. `p(slug="greeting", name="Alice", style="casual")` — Renders the template with slug `"greeting"`, passing `name="Alice"` and `style="casual"`.
3. `$output` — Stores the rendered plain text result.

`p()` parameter rules:

| Parameter | Purpose |
|------|------|
| `slug` | Required. Corresponds to the `slug` field in the `.jgprompt` file |
| Other key=value pairs | Correspond to template variables, overriding `inputs` default values |

The example above outputs:

```text
Hello, Alice!

What's up?
```

Because `style="casual"` triggers the `{% if style == "casual" %}` branch.

## 7.4 Variables and Default Values

Values defined in `inputs` are defaults. When calling `p()`, provided parameters override the defaults; omitted parameters use the defaults.

Create `prompts/welcome.jgprompt`:

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

In a workflow, override only some variables:

```juglans
prompts: ["./prompts/*.jgprompt"]

[full]: p(slug="welcome", name="Alice", role="admin", lang="Chinese")
[show1]: print(message=$output)

[partial]: p(slug="welcome", name="Bob")
[show2]: print(message=$output)

[full] -> [show1] -> [partial] -> [show2]
```

`[full]` overrides all three variables. `[partial]` only passes `name="Bob"` — `role` and `lang` use their default values `"user"` and `"English"`.

### Passing Dynamic Values with $input

Variable values can come from workflow input:

```juglans
prompts: ["./prompts/*.jgprompt"]

[render]: p(slug="welcome", name=$input.name, role=$input.role)
[show]: print(message=$output)

[render] -> [show]
```

```bash
juglans app.jg --input '{"name": "Alice", "role": "admin"}'
```

`$input.name` and `$input.role` are resolved to their actual values at execution time and passed into the template for rendering.

## 7.5 Conditional Rendering

Use `{% if %}` / `{% elif %}` / `{% else %}` / `{% endif %}` to control rendered content.

Create `prompts/tone.jgprompt`:

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

Use in a workflow:

```juglans
prompts: ["./prompts/*.jgprompt"]

[expert]: p(slug="tone", topic="Transformer architecture", audience="expert")
[show]: print(message=$output)

[expert] -> [show]
```

`audience="expert"` matches the first branch, outputting:

```text
Explain Transformer architecture.

Use technical terminology. Assume deep domain knowledge. Focus on nuances and edge cases.
```

### Nested Conditions

Conditions can be nested:

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

## 7.6 Loop Rendering

Use `{% for item in list %}` / `{% endfor %}` to iterate over arrays.

Create `prompts/checklist.jgprompt`:

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

Rendered result:

```text
Please review the following aspects:

- correctness
- performance
- readability

Provide feedback for each.
```

### The loop Variable

Inside `{% for %}` blocks, the `loop` object provides iteration metadata:

| Field | Type | Description |
|------|------|------|
| `loop.index` | number | Current index (1-based) |
| `loop.index0` | number | Current index (0-based) |
| `loop.first` | bool | Whether this is the first element |
| `loop.last` | bool | Whether this is the last element |

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

Rendered result:

```text
1. Parse input
2. Validate data
3. Execute workflow
```

## 7.7 Filters

Use functions within `{{ }}` interpolation to transform values. Juglans templates use function call syntax (rather than Jinja's pipe syntax):

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

Rendered result (using default values):

```text
Name: ALICE
Title: Untitled
Score: 95 points
```

### Common Functions

| Function | Description | Example |
|------|------|------|
| `upper(x)` | Convert to uppercase | `upper("hello")` -> `"HELLO"` |
| `lower(x)` | Convert to lowercase | `lower("HELLO")` -> `"hello"` |
| `default(x, fallback)` | Fallback for empty values | `default("", "N/A")` -> `"N/A"` |
| `str(x)` | Convert to string | `str(42)` -> `"42"` |
| `len(x)` | Get length | `len("hello")` -> `5` |
| `replace(x, old, new)` | Replace substring | `replace("hi all", "hi", "hello")` -> `"hello all"` |
| `join(list, sep)` | Join array elements | `join(["a","b"], ", ")` -> `"a, b"` |

## 7.8 Using with chat()

`p()` renders the template + `chat()` sends it to the AI — this is the most classic pattern in Juglans:

```juglans
prompts: ["./prompts/*.jgprompt"]
agents: ["./agents/*.jgagent"]

[render]: p(slug="tone", topic=$input.topic, audience=$input.audience)
[ask]: chat(agent="assistant", message=$output)
[show]: print(message=$output)

[render] -> [ask] -> [show]
```

Execution flow:

1. `[render]` — `p()` renders the template, generating the complete prompt text, stored in `$output`.
2. `[ask]` — `chat()` reads `$output` as the `message` and sends it to the AI.
3. `[show]` — Prints the AI's response.

### Inline Usage

`p()` can also be used directly as the value of `chat()`'s `message` parameter:

```juglans
prompts: ["./prompts/*.jgprompt"]
agents: ["./agents/*.jgagent"]

[ask]: chat(agent="assistant", message=p(slug="tone", topic=$input.topic, audience="expert"))
[show]: print(message=$output)

[ask] -> [show]
```

This passes the rendered result of `p()` directly to `chat()`, eliminating an intermediate node.

### Chaining Multiple Templates

In complex scenarios, you can combine prompts from multiple templates:

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

`[system]` renders the role-setting template, `[user_msg]` renders the checklist template, and the two are concatenated before being sent to the AI.

## Summary

| Concept | Syntax | Purpose |
|------|------|------|
| Prompt file | `.jgprompt` | Reusable template file with frontmatter + body |
| Loading templates | `prompts: ["./prompts/*.jgprompt"]` | Import template files into a workflow |
| Rendering templates | `p(slug="name", key=value)` | Render a specified template with parameters |
| Variable interpolation | `{{ variable }}` | Output variable values |
| Conditional rendering | `{% if %}` / `{% elif %}` / `{% else %}` / `{% endif %}` | Select content based on conditions |
| Loop rendering | `{% for item in list %}` / `{% endfor %}` | Generate content by iterating over arrays |
| Function calls | `{{ upper(name) }}` | Transform variable values |
| Default values | Values in `inputs` | Fallback values when parameters are not provided |

Key rules:

1. Before using `p()`, you must load template files via `prompts:` metadata.
2. The `slug` parameter is required; other parameters correspond to template variables.
3. Values in `inputs` are defaults; parameters passed to `p()` override them.
4. `p()` returns a rendered plain text string, stored in `$output`.
5. `p()` + `chat()` is the most common combination: render the prompt first, then send it to the AI.

## Next Chapter

**[Tutorial 8: Workflow Composition](./composition.md)** — Learn about `flows:` imports and `libs:` library references to compose multiple workflows into complex execution graphs.
