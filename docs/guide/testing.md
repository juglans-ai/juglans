# How to Test Workflows

This guide covers how to validate and test Juglans workflows.

## juglans check -- Static Validation

`juglans check` performs static syntax validation on `.jg` and `.jgx` files without executing any tool calls:

```bash
# Check all files in the current directory
juglans check

# Check a specific directory
juglans check ./src/

# Check a single file
juglans check src/main.jg

# Show all warnings
juglans check --all

# JSON output (suitable for programmatic parsing)
juglans check --format json
```

What it checks:

- Syntax correctness (node definitions, edge definitions, metadata)
- Node reference consistency (nodes referenced by edges must be defined)
- Entry node inference (topological sort; nodes with in-degree 0)
- Cycle detection

## juglans test -- Automated Testing

`juglans test` discovers and runs workflow-embedded tests. Any node whose ID starts with `test_` is treated as a test case: the executor runs it as a subgraph (along with its dependencies) and collects the result.

Core capabilities:

- **Test discovery** -- Scans `.jg` files for nodes prefixed with `test_` and executes each as an isolated subgraph
- **Multiple output formats** -- Report results as human-readable `text`, machine-readable `json`, or CI-friendly `junit` XML via `--format`
- **Standard exit codes** -- Exit 0 when all tests pass, non-zero on failure, suitable for CI pipelines

### Writing assertions: `assert`

Inside a `test_*` node, use `assert <expression>` to declare a condition that must hold. The runtime evaluates the expression with the same engine as `chat()` parameters; the test passes when the result is truthy. Failures are collected and reported at the end of `juglans test`.

```juglans
[test_adds_correctly]: {
  sum = 2 + 2
  assert sum == 4
}

[test_contains_substring]: {
  greeting = "Hello, Alice!"
  assert "Alice" in greeting
}

[test_truthy]: {
  users = fetch_users()
  assert len(users) > 0
}

[test_chained_predicates]: {
  result = classify(input.text)
  assert result.confidence > 0.8 and result.intent in ["question", "task"]
}
```

`assert` is a parser keyword, not a tool call — there are no named parameters. Any expression that the [expression evaluator](../reference/expressions.md) understands is valid: comparisons (`==`, `!=`, `<`, `>=`), membership (`in`, `not in`), logical (`and`, `or`, `not`), function calls (`len`, `contains`, `startswith`, etc.), and dotted access on prior nodes' output (`step.output.field`).

### Stubbing dependencies: `mock()`

Tests that exercise real workflows often need to stub out an LLM, an external HTTP call, or a slow Python tool. `mock()` runs a target workflow with **injected node outputs** — nodes named in `inject` are skipped during execution and their output is set to the injected value. The return value is the workflow's final `output` (i.e. whatever the last terminal node produces):

```juglans
# Classify routing test — pin the LLM output, assert the workflow picks
# the right downstream branch.
[test_classifier_routes_questions]: {
  final = mock(
    workflow = "main.jg",
    inject   = { "classify": { "intent": "question", "confidence": 0.97 } },
    input    = { "text": "What time is it?" }
  )
  assert final.routed_to == "answer"
}
```

The `inject` map can stub any node by ID — LLM calls, HTTP fetches, db queries — so the test only exercises the routing / control-flow logic without burning tokens or hitting the network. Pair `mock()` with one or more `assert` lines to check the expected outcome.

## Manual Testing

Run a workflow manually and pass input data:

```bash
# Basic execution
juglans src/main.jg

# Pass JSON input
juglans src/main.jg --input '{"query": "Hello"}'

# Read input from a file
juglans src/main.jg --input-file input.json

# Verbose mode (view each node's input and output)
juglans src/main.jg --input '{"query": "test"}' --verbose

# Parse only, do not execute (verify structure)
juglans src/main.jg --dry-run

# JSON output format (convenient for programmatic processing)
juglans src/main.jg --output-format json
```

Test individual resource types:

```bash
# Test a Prompt (render the template)
juglans src/prompts/greeting.jgx --input '{"name": "Alice"}'

# Test a workflow with inline agents
juglans src/main.jg --input '{"message": "Hello"}'
```

## doctest -- Validate Documentation

`juglans doctest` extracts ` ```juglans ` code blocks from Markdown files and validates their syntax:

```bash
# Validate a single file
juglans doctest docs/guide/concepts.md

# Validate the entire docs directory
juglans doctest docs/

# JSON output
juglans doctest docs/ --format json
```

Rules for writing code blocks that pass doctest:

1. Node definitions must appear before edge definitions
2. Nodes referenced by edges must already be defined
3. Code blocks that should not be validated must be marked with `ignore`

Example:

```juglans
# This code block will be validated by doctest
[start]: print(message="hello")
[end]: print(message="done")
[start] -> [end]
```

## CI Integration

Integrate Juglans checks into GitHub Actions:

```yaml
# .github/workflows/check.yml
name: Juglans Check

on: [push, pull_request]

jobs:
  check:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Juglans
        run: |
          cargo install --path juglans

      - name: Syntax Check
        run: juglans check ./src/

      - name: Doctest
        run: juglans doctest ./docs/
```

Exit code reference:

| Command | Exit 0 | Exit 1 |
|---------|--------|--------|
| `juglans check` | All files pass validation | Syntax errors found |
| `juglans doctest` | All code blocks parse successfully | One or more code blocks failed to parse |

Both commands are suitable for direct use in CI pipelines with no additional configuration required.

## Best Practices

1. **Before committing** -- Run `juglans check` to ensure correct syntax
2. **After updating docs** -- Run `juglans doctest docs/` to ensure example code is valid
3. **In CI** -- Run both `check` and `doctest` as separate steps
4. **Manual testing** -- Use `--verbose` to view detailed execution and `--dry-run` to quickly verify structure

## Next Steps

- [Debugging](./debugging.md) -- Debugging tips
- [Error Handling](../tutorials/error-handling.md) -- Handling errors in workflows
- [CLI Reference](../reference/cli.md) -- Complete command reference
