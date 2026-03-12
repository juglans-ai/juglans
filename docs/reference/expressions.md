# Expressions & Variables Reference

Complete reference for the Juglans Expression Language (JEL). Covers variables, operators, built-in functions, type system, and usage contexts.

---

## 1. Variable System

### Overview

| Variable | Source | Writable | Scope | Example |
|----------|--------|----------|-------|---------|
| `$input` | CLI `--input` JSON or API body | No | Entire workflow | `$input.name`, `$input.items.0` |
| `$output` | Return value of the last executed node | No | Overwritten each step | `$output`, `$output.text` |
| `$ctx` | `set_context()` | Yes | Entire workflow | `$ctx.count`, `$ctx.user.name` |
| `$reply` | AI response metadata | No | After `chat()` | `$reply.content`, `$reply.tokens` |
| `$error` | Set when `on error` triggers | No | Error path | `$error.message`, `$error.node` |
| `$node_id.output` | Specific node's output | No | Entire workflow | `$classify.output` |

### $input

Data passed when the workflow starts. Read-only.

```juglans
[greet]: print(message="Hello, " + $input.name)
[done]: print(message="Done")
[greet] -> [done]
```

Sources:

```bash
# CLI
juglans workflow.jg --input '{"name": "Alice", "items": [1, 2, 3]}'

# API
# POST /api/workflows/my-flow/execute  body: {"name": "Alice"}
```

Access nested fields with dot notation. Arrays use numeric indices:

```text
$input              # Entire object
$input.name         # "Alice"
$input.items.0      # 1 (first element)
$input.user.address.city   # Deep nesting
```

Missing fields resolve to `null` (no error).

### $output

Return value of the most recently executed node. Overwritten after each step.

```juglans
[step1]: print(message="hello")
[step2]: print(message="step1 said: " + $output)
[step1] -> [step2]
```

### $node_id.output

Access a specific node's output by its ID. Persists for the entire workflow -- use this to reach back to earlier nodes.

```juglans
[a]: print(message="first")
[b]: print(message="second")
[c]: print(message=$a.output + " and " + $b.output)
[a] -> [b] -> [c]
```

### $ctx

User-defined shared storage. Write with `set_context()`, read with `$ctx.key`.

```juglans
[init]: set_context(count=0, name="Alice")
[inc]: set_context(count=$ctx.count + 1)
[show]: print(message=$ctx.name + ": " + str($ctx.count))
[init] -> [inc] -> [show]
```

Context persists across all nodes in the workflow.

### $reply

Metadata from the most recent AI response (after `chat()`).

| Field | Type | Description |
|-------|------|-------------|
| `$reply.content` | string | Reply content |
| `$reply.tokens` | number | Tokens used |
| `$reply.model` | string | Model used |
| `$reply.finish_reason` | string | Finish reason |
| `$reply.chat_id` | string | Conversation session ID |

```juglans
[ask]: chat(agent="assistant", message=$input.query)
[log]: print(message="Tokens: " + str($reply.tokens))
[ask] -> [log]
```

### $error

Available on `on error` paths. Contains information about the failure.

| Field | Type | Description |
|-------|------|-------------|
| `$error.message` | string | Error message |
| `$error.node` | string | Node ID that failed |

```juglans
[risky]: fetch_url(url=$input.url)
[handler]: print(message="Failed at " + $error.node + ": " + $error.message)
[risky] on error -> [handler]
```

### Namespaced Variables (Flow Imports)

When using `flows:` imports, subworkflow node outputs are accessed via namespace prefix:

```text
$auth.verify.output       # Output of the verify node in the auth subworkflow
$trading.extract.output   # Output of extract in the trading subworkflow
```

Global variables (`$ctx`, `$input`, `$output`) are not prefixed.

### Loop Context Variables

Available inside `foreach` and `while` blocks:

| Variable | Type | Description |
|----------|------|-------------|
| `loop.index` | number | Current index (0-based) |
| `loop.first` | boolean | First iteration? |
| `loop.last` | boolean | Last iteration? |

```juglans
[process]: foreach($item in $input.items) {
  [log]: print(message="Item " + str(loop.index + 1) + ": " + $item)
}
```

---

## 2. Operators

### Arithmetic

| Operator | Description | Example |
|----------|-------------|---------|
| `+` | Addition / string concatenation | `$ctx.a + 1`, `"hi" + " " + "there"` |
| `-` | Subtraction | `$ctx.a - $ctx.b` |
| `*` | Multiplication | `$ctx.price * $ctx.quantity` |
| `/` | Division | `$ctx.total / $ctx.count` |
| `%` | Modulo | `$ctx.index % 2` |

```juglans
[calc]: set_context(
  sum=$ctx.a + $ctx.b,
  diff=$ctx.a - $ctx.b,
  product=$ctx.a * $ctx.b
)
```

The `+` operator also concatenates strings:

```juglans
[greet]: print(message="Hello, " + $input.name + "!")
[done]: print(message="ok")
[greet] -> [done]
```

### Comparison

| Operator | Description |
|----------|-------------|
| `==` | Equal to |
| `!=` | Not equal to |
| `>` | Greater than |
| `<` | Less than |
| `>=` | Greater than or equal to |
| `<=` | Less than or equal to |

```juglans
[check]: print(message="checking")
[high]: print(message="high")
[low]: print(message="low")
[check] if $ctx.score >= 80 -> [high]
[check] if $ctx.score < 80 -> [low]
```

### Logical

| Operator | Alias | Description |
|----------|-------|-------------|
| `and` | `&&` | Logical AND |
| `or` | `\|\|` | Logical OR |
| `not` | `!` | Logical NOT |

```juglans
[check]: print(message="checking")
[allow]: print(message="allowed")
[deny]: print(message="denied")
[check] if $ctx.logged_in && $ctx.is_admin -> [allow]
[check] if !$ctx.logged_in -> [deny]
```

### Membership

| Operator | Description |
|----------|-------------|
| `in` | Value is contained in collection |
| `not in` | Value is not contained in collection |

Works with strings (substring check), arrays (element check), and objects (key check).

### String Concatenation

Use `+` to join strings. Non-string values must be converted with `str()` first:

```juglans
[show]: print(message="Count: " + str($ctx.count) + " items")
[done]: print(message="ok")
[show] -> [done]
```

### Operator Precedence (low to high)

1. `or`, `||`
2. `and`, `&&`
3. `in`, `not in`
4. `==`, `!=`, `>`, `<`, `>=`, `<=`
5. `+`, `-`
6. `*`, `/`, `%`
7. `not`, `!`, unary `-`
8. `.` (dot access), `[]` (bracket access), `()` (function call)

Use parentheses to override precedence:

```juglans
[check]: print(message="checking")
[target]: print(message="target")
[check] if ($ctx.a > 0 && $ctx.b > 0) || $ctx.force -> [target]
```

---

## 3. Built-in Functions

### Type Conversion

| Function | Signature | Description |
|----------|-----------|-------------|
| `str(x)` | `str(value) -> str` | Convert any value to string |
| `int(x)` | `int(value) -> int` | Convert to integer (truncates floats, parses strings) |
| `float(x)` | `float(value) -> float` | Convert to float |
| `bool(x)` | `bool(value) -> bool` | Convert using truthiness rules |

```juglans
[conv]: set_context(
  s=str(42),
  n=int("100"),
  f=float("3.14"),
  b=bool(1)
)
```

### Type Checking

| Function | Signature | Description |
|----------|-----------|-------------|
| `type(x)` | `type(value) -> str` | Returns type name: `"str"`, `"number"`, `"bool"`, `"list"`, `"dict"`, `"None"` |
| `is_null(x)` | `is_null(value) -> bool` | True if value is null |
| `is_string(x)` | `is_string(value) -> bool` | True if value is a string |
| `is_number(x)` | `is_number(value) -> bool` | True if value is a number |
| `is_bool(x)` | `is_bool(value) -> bool` | True if value is a boolean |
| `is_array(x)` | `is_array(value) -> bool` | True if value is a list |
| `is_object(x)` | `is_object(value) -> bool` | True if value is a dict |

```juglans
[check]: print(message="Type is: " + type($ctx.value))
[done]: print(message="ok")
[check] -> [done]
```

### String Functions

| Function | Signature | Description |
|----------|-----------|-------------|
| `upper(s)` | `upper(str) -> str` | Convert to uppercase |
| `lower(s)` | `lower(str) -> str` | Convert to lowercase |
| `trim(s)` | `trim(str) -> str` | Remove leading/trailing whitespace (alias: `strip`) |
| `replace(s, old, new)` | `replace(str, str, str) -> str` | Replace all occurrences |
| `split(s, sep)` | `split(str, str) -> list` | Split string by separator |
| `join(arr, sep)` | `join(list, str) -> str` | Join list elements with separator |
| `contains(s, sub)` | `contains(str, str) -> bool` | Check if string contains substring |
| `startswith(s, prefix)` | `startswith(str, str) -> bool` | Check if string starts with prefix |
| `endswith(s, suffix)` | `endswith(str, str) -> bool` | Check if string ends with suffix |
| `find(s, sub)` | `find(str, str) -> int` | Index of first occurrence (-1 if not found) |
| `count(s, sub)` | `count(str, str) -> int` | Count occurrences of substring |
| `capitalize(s)` | `capitalize(str) -> str` | Capitalize first letter, lowercase rest |
| `title(s)` | `title(str) -> str` | Title Case each word |
| `truncate(s, n)` | `truncate(str, int) -> str` | Truncate to n chars, append `...` if truncated |
| `lpad(s, w, c?)` | `lpad(str, int, str?) -> str` | Left-pad to width w with char c (default: space) |
| `rpad(s, w, c?)` | `rpad(str, int, str?) -> str` | Right-pad to width w with char c (default: space) |
| `repeat(s, n)` | `repeat(str, int) -> str` | Repeat string n times |
| `slice(s, start, end?)` | `slice(str, int, int?) -> str` | Substring (supports negative indices) |
| `reverse(s)` | `reverse(str) -> str` | Reverse a string |

```juglans
[demo]: set_context(
  up=upper("hello"),
  lo=lower("WORLD"),
  t=trim("  spaced  "),
  r=replace("foo bar foo", "foo", "baz"),
  parts=split("a,b,c", ","),
  joined=join(["x", "y", "z"], "-")
)
```

```juglans
[pad]: set_context(
  left=lpad("42", 5, "0"),
  right=rpad("hi", 10, "."),
  sub=slice("hello world", 0, 5)
)
```

### Collection Functions

| Function | Signature | Description |
|----------|-----------|-------------|
| `len(x)` | `len(str\|list\|dict) -> int` | Length of string, array, or object |
| `keys(d)` | `keys(dict) -> list` | Get all keys of an object |
| `values(d)` | `values(dict) -> list` | Get all values of an object |
| `items(d)` | `items(dict) -> list` | Get `[key, value]` pairs |
| `has(x, key)` | `has(dict\|list, value) -> bool` | Check if key exists in dict or element in list |
| `get(x, key, default?)` | `get(dict\|list, key, value?) -> value` | Safe access with optional default |
| `append(arr, item)` | `append(list, value) -> list` | Return new list with item appended |
| `range(end)` / `range(start, end)` / `range(start, end, step)` | `range(...) -> list` | Generate a list of integers |
| `sort(arr)` | `sort(list) -> list` | Sort list (numbers numerically, others lexicographically) |
| `reverse(arr)` | `reverse(list) -> list` | Reverse a list |
| `unique(arr)` | `unique(list) -> list` | Remove duplicates, preserving order |
| `flatten(arr)` | `flatten(list) -> list` | Flatten one level of nested lists |
| `slice(arr, start, end?)` | `slice(list, int, int?) -> list` | Sub-list (supports negative indices) |
| `first(arr)` | `first(list) -> value` | First element (null if empty) |
| `last(arr)` | `last(list) -> value` | Last element (null if empty) |
| `chunk(arr, size)` | `chunk(list, int) -> list` | Split list into chunks of given size |
| `sum(arr)` | `sum(list) -> number` | Sum all numeric elements |
| `zip(a, b)` | `zip(list, list) -> list` | Pair elements from two lists |
| `enumerate(arr)` | `enumerate(list) -> list` | Pairs of `[index, element]` |
| `all(arr)` | `all(list) -> bool` | True if all elements are truthy |
| `any(arr)` | `any(list) -> bool` | True if any element is truthy |

```juglans
[init]: set_context(items=[3, 1, 2])
[ops]: set_context(
  length=len($ctx.items),
  sorted=sort($ctx.items),
  added=append($ctx.items, 4),
  total=sum($ctx.items)
)
[init] -> [ops]
```

```juglans
[range_demo]: set_context(
  five=range(5),
  evens=range(0, 10, 2),
  rev=reverse(range(5))
)
```

### Math Functions

| Function | Signature | Description |
|----------|-----------|-------------|
| `round(x, digits?)` | `round(number, int?) -> number` | Round to n decimal places (default: 0) |
| `abs(x)` | `abs(number) -> number` | Absolute value |
| `min(a, b, ...)` | `min(number, number, ...) -> number` | Minimum of two or more values |
| `max(a, b, ...)` | `max(number, number, ...) -> number` | Maximum of two or more values |
| `floor(x)` | `floor(number) -> int` | Round down |
| `ceil(x)` | `ceil(number) -> int` | Round up |
| `pow(base, exp)` | `pow(number, number) -> number` | Exponentiation |
| `sqrt(x)` | `sqrt(number) -> number` | Square root |
| `log(x, base?)` | `log(number, number?) -> number` | Logarithm (natural log if no base) |
| `clamp(x, min, max)` | `clamp(number, number, number) -> number` | Clamp value to range |
| `random()` | `random() -> float` | Random float in [0, 1) |
| `randint(min, max)` | `randint(int, int) -> int` | Random integer in [min, max] |

```juglans
[math]: set_context(
  r=round(3.14159, 2),
  a=abs(-42),
  lo=min(10, 20, 5),
  hi=max(10, 20, 5),
  clamped=clamp(150, 0, 100)
)
```

### JSON / Data Functions

| Function | Signature | Description |
|----------|-----------|-------------|
| `json(x)` | `json(value) -> str\|value` | Encode non-string to JSON string; decode JSON string to value |
| `json_pretty(x)` | `json_pretty(value) -> str` | Pretty-print value as indented JSON |
| `from_json(s)` | `from_json(str) -> value` | Parse JSON string (strict -- errors on invalid JSON) |
| `merge(a, b)` | `merge(dict, dict) -> dict` | Merge two objects (b overwrites a) |
| `pick(obj, keys)` | `pick(dict, list) -> dict` | Keep only listed keys |
| `omit(obj, keys)` | `omit(dict, list) -> dict` | Remove listed keys |
| `from_entries(pairs)` | `from_entries(list) -> dict` | Convert `[[key, value], ...]` to object |

```juglans
[data]: set_context(
  encoded=json({"name": "Alice", "age": 30}),
  merged=merge({"a": 1}, {"b": 2}),
  picked=pick({"a": 1, "b": 2, "c": 3}, ["a", "c"])
)
```

### Date/Time Functions

| Function | Signature | Description |
|----------|-----------|-------------|
| `now()` | `now() -> str` | Current UTC time as ISO 8601 string |
| `timestamp()` | `timestamp() -> int` | Current UTC Unix timestamp (seconds) |
| `timestamp_ms()` | `timestamp_ms() -> int` | Current UTC Unix timestamp (milliseconds) |
| `format_date(iso, fmt)` | `format_date(str, str) -> str` | Format an ISO 8601 string with strftime syntax |
| `parse_date(s, fmt)` | `parse_date(str, str) -> str` | Parse a date string into ISO 8601 |

```juglans
[time]: set_context(
  current=now(),
  ts=timestamp(),
  formatted=format_date(now(), "%Y-%m-%d")
)
```

### Encoding Functions

| Function | Signature | Description |
|----------|-----------|-------------|
| `base64_encode(s)` | `base64_encode(str) -> str` | Base64 encode |
| `base64_decode(s)` | `base64_decode(str) -> str` | Base64 decode |
| `url_encode(s)` | `url_encode(str) -> str` | URL-encode a string |
| `url_decode(s)` | `url_decode(str) -> str` | URL-decode a string |
| `md5(s)` | `md5(str) -> str` | MD5 hash (hex) |
| `sha256(s)` | `sha256(str) -> str` | SHA-256 hash (hex) |

```juglans
[encode]: set_context(
  b64=base64_encode("hello world"),
  url=url_encode("a=1&b=2"),
  hash=sha256("secret")
)
```

### Regex Functions

| Function | Signature | Description |
|----------|-----------|-------------|
| `regex_match(s, pattern)` | `regex_match(str, str) -> bool` | Test if string matches pattern |
| `regex_find(s, pattern)` | `regex_find(str, str) -> str\|null` | First match (null if none) |
| `regex_find_all(s, pattern)` | `regex_find_all(str, str) -> list` | All matches |
| `regex_replace(s, pattern, rep)` | `regex_replace(str, str, str) -> str` | Replace all matches |

```juglans
[rx]: set_context(
  is_email=regex_match("a@b.com", "^[^@]+@[^@]+$"),
  digits=regex_find_all("abc123def456", "[0-9]+"),
  cleaned=regex_replace("Hello   World", "\\s+", " ")
)
```

### Path Functions

| Function | Signature | Description |
|----------|-----------|-------------|
| `basename(path)` | `basename(str) -> str` | File name from path |
| `dirname(path)` | `dirname(str) -> str` | Directory from path |
| `extname(path)` | `extname(str) -> str` | Extension (e.g., `".txt"`) |
| `join_path(a, b, ...)` | `join_path(str, str, ...) -> str` | Join path segments |

```juglans
[paths]: set_context(
  name=basename("/home/user/data.csv"),
  dir=dirname("/home/user/data.csv"),
  ext=extname("/home/user/data.csv"),
  full=join_path("/home", "user", "data.csv")
)
```

### Other Functions

| Function | Signature | Description |
|----------|-----------|-------------|
| `default(x, fallback)` | `default(value, value) -> value` | Return fallback if x is null or empty string |
| `format(template, args...)` | `format(str, ...) -> str` | Python-style `{}` formatting |
| `uuid()` | `uuid() -> str` | Generate a UUID v4 |
| `env(name, default?)` | `env(str, value?) -> str\|null` | Read environment variable |
| `chr(n)` | `chr(int) -> str` | Unicode code point to character |
| `ord(s)` | `ord(str) -> int` | First character to Unicode code point |
| `hex(n)` | `hex(int) -> str` | Integer to hex string (`"0x2a"`) |
| `bin(n)` | `bin(int) -> str` | Integer to binary string (`"0b101010"`) |
| `oct(n)` | `oct(int) -> str` | Integer to octal string (`"0o52"`) |

```juglans
[misc]: set_context(
  name=default($input.name, "anonymous"),
  msg=format("Hello {}, you have {} items", "Alice", 5),
  id=uuid()
)
```

### Higher-Order Functions (Lambda Support)

These functions accept lambda expressions (`x => expr` or `(x, y) => expr`):

| Function | Signature | Description |
|----------|-----------|-------------|
| `map(arr, fn)` | `map(list, lambda) -> list` | Transform each element |
| `filter(arr, fn)` | `filter(list, lambda) -> list` | Keep elements where fn returns truthy |
| `reduce(arr, fn, init)` | `reduce(list, lambda, value) -> value` | Fold list to single value |
| `sort_by(arr, fn)` | `sort_by(list, lambda) -> list` | Sort by computed key |
| `find_by(arr, fn)` | `find_by(list, lambda) -> value\|null` | First element where fn is truthy |
| `group_by(arr, fn)` | `group_by(list, lambda) -> dict` | Group elements by computed key |
| `flat_map(arr, fn)` | `flat_map(list, lambda) -> list` | Map then flatten one level |
| `count_by(arr, fn)` | `count_by(list, lambda) -> dict` | Count elements by computed key |

Lambda syntax: `param => body` for single parameter, `(a, b) => body` for multiple.

```juglans
[transform]: set_context(
  doubled=map([1, 2, 3], x => x * 2),
  evens=filter([1, 2, 3, 4, 5], x => x % 2 == 0),
  total=reduce([1, 2, 3, 4], (acc, x) => acc + x, 0)
)
```

```juglans
[advanced]: set_context(
  sorted=sort_by(["banana", "apple", "cherry"], x => len(x)),
  grouped=group_by([1, 2, 3, 4, 5], x => x % 2)
)
```

Method call syntax is also supported -- lambdas can be chained:

```juglans
[chain]: set_context(
  result=[1, 2, 3, 4, 5].filter(x => x > 2).map(x => x * 10)
)
```

### Pipe Syntax

Jinja-style pipe filters can be used as an alternative to function calls:

```text
value | upper              # upper(value)
value | truncate(20)       # truncate(value, 20)
value | replace("a", "b")  # replace(value, "a", "b")
```

The value before `|` is passed as the first argument to the filter function.

---

## 4. Expression Usage Contexts

Expressions can appear in these positions within a workflow:

### Node Parameter Values

```juglans
[step]: print(message="Count: " + str(len($ctx.items)))
[done]: print(message="ok")
[step] -> [done]
```

### Conditional Edges (`if`)

```juglans
[check]: print(message="checking")
[pass]: print(message="pass")
[fail]: print(message="fail")
[check] if $ctx.score > 80 && $ctx.verified -> [pass]
[check] if $ctx.score <= 80 -> [fail]
```

### Switch Values

```juglans
[route]: print(message="routing")
[en]: print(message="english")
[zh]: print(message="chinese")
[other]: print(message="other")

[route] -> switch $ctx.language {
  "en": [en]
  "zh": [zh]
  default: [other]
}
```

### While Conditions

```juglans
[loop]: while($ctx.count < 10) {
  [inc]: set_context(count=$ctx.count + 1)
}
```

### Assignment Sugar

Assignment syntax desugars to `set_context()`:

```juglans
[init]: count = 0, name = "Alice"
[show]: print(message=$ctx.name + ": " + str($ctx.count))
[init] -> [show]
```

---

## 5. Type System

### Types

| Type | Literal Syntax | JEL `type()` | Example |
|------|---------------|--------------|---------|
| null | `null`, `none`, `None` | `"None"` | `null` |
| bool | `true`, `false`, `True`, `False` | `"bool"` | `true` |
| number (int) | `42`, `-1`, `0` | `"number"` | `42` |
| number (float) | `3.14`, `-0.5` | `"number"` | `3.14` |
| string | `"hello"`, `'hello'`, `"""multiline"""` | `"str"` | `"hello"` |
| array | `[1, 2, 3]` | `"list"` | `[1, "two", true]` |
| object | `{"key": "value"}` | `"dict"` | `{"a": 1}` |

### F-Strings (Interpolated Strings)

F-strings allow embedding expressions inside string literals:

```text
f"Hello {name}, you have {count} items"
f"Result: {$ctx.score * 100}%"
```

Triple-quoted f-strings support multi-line content:

```text
f"""
Name: {name}
Score: {score}
"""
```

### Truthiness

Juglans uses Python-like truthiness rules. The following values are **falsy**:

| Value | Falsy? |
|-------|--------|
| `null` / `None` | Yes |
| `false` | Yes |
| `0` (zero) | Yes |
| `""` (empty string) | Yes |
| `[]` (empty array) | Yes |
| `{}` (empty object) | Yes |

Everything else is **truthy**.

```juglans
[check]: print(message="checking")
[has_data]: print(message="data exists")
[no_data]: print(message="no data")

# Empty list is falsy, non-empty list is truthy
[check] if $ctx.results -> [has_data]
[check] if !$ctx.results -> [no_data]
```

### Type Coercion

- Arithmetic operators coerce operands to numbers. Strings are parsed; bools become 0/1; null becomes 0.
- `+` with a string operand on either side performs concatenation (non-strings converted via `str()`).
- Comparison operators compare numbers numerically and strings lexicographically.

---

## 6. Comprehensive Examples

### Data Processing Pipeline

```juglans
[init]: set_context(
  results=[],
  total=0,
  processed=0
)

[process]: foreach($item in $input.records) {
  [validate]: set_context(
    valid=$item.score >= 0 && $item.score <= 100
  )

  [transform]: set_context(
    normalized=round($item.score / 100, 2),
    label=upper(slice($item.name, 0, 3)),
    processed=$ctx.processed + 1,
    total=$ctx.total + $item.score,
    results=append($ctx.results, {
      "name": $item.name,
      "score": $item.score,
      "label": upper(slice($item.name, 0, 3))
    })
  )

  [validate] -> [transform]
}

[report]: print(
  message="Processed " + str($ctx.processed) + " items. Avg: " + str(round($ctx.total / $ctx.processed, 1))
)

[init] -> [process] -> [report]
```

### Dynamic Routing with Expressions

```juglans
[evaluate]: set_context(
  score=$input.score,
  tier=default($input.tier, "standard")
)

[done]: print(message="Routed to handler")
[excellent]: print(message="Excellent!")
[good]: print(message="Good")
[retry]: print(message="Needs improvement")

[evaluate] if $ctx.score >= 90 && $ctx.tier == "premium" -> [excellent]
[evaluate] if $ctx.score >= 60 -> [good]
[evaluate] -> [retry]

[excellent] -> [done]
[good] -> [done]
[retry] -> [done]
```

### Collection Transformation

```juglans
[setup]: set_context(
  numbers=[5, 3, 8, 1, 9, 2, 7],
  words=["hello", "world", "juglans"]
)

[transform]: set_context(
  sorted=sort($ctx.numbers),
  top3=slice(sort($ctx.numbers), 4),
  total=sum($ctx.numbers),
  avg=round(sum($ctx.numbers) / len($ctx.numbers), 2),
  upper_words=map($ctx.words, w => upper(w)),
  long_words=filter($ctx.words, w => len(w) > 5),
  lengths=map($ctx.words, w => len(w)),
  word_str=join($ctx.words, ", ")
)

[result]: print(
  message="Sum=" + str($ctx.total) + " Avg=" + str($ctx.avg) + " Words: " + $ctx.word_str
)

[setup] -> [transform] -> [result]
```
