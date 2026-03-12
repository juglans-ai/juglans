# How to Test Workflows

This guide covers how to validate and test Juglans workflows.

## juglans check -- Static Validation

`juglans check` performs static syntax validation on `.jg`, `.jgagent`, and `.jgprompt` files without executing any tool calls:

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

`juglans test` provides automated testing capabilities for AI workflows. See the [juglans test design document](./juglans-test.md) for details.

Core capabilities:

- **Node-level testing** -- Test individual nodes in isolation with automatic dependency mocking
- **Semantic assertions** -- Use AI to evaluate output quality (rather than exact string matching)
- **Snapshot regression** -- Record each execution result and automatically detect changes

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
# Test an Agent (interactive mode)
juglans src/agents/assistant.jgagent

# Test an Agent (single message)
juglans src/agents/assistant.jgagent --message "What is Rust?"

# Test a Prompt (render the template)
juglans src/prompts/greeting.jgprompt --input '{"name": "Alice"}'
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
- [Error Handling](./error-handling.md) -- Handling errors in workflows
- [CLI Reference](../reference/cli.md) -- Complete command reference
