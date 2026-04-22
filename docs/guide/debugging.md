# How to Debug Workflows

This guide covers debugging and troubleshooting methods for Juglans workflows.

## juglans check -- Syntax Validation

`juglans check` validates file syntax without executing anything, similar to `cargo check`:

```bash
# Check all files in the current directory
juglans check

# Check a specific directory
juglans check ./src/

# Check a single file
juglans check src/main.jg

# Show all warnings
juglans check --all

# JSON output (suitable for CI)
juglans check --format json
```

An exit code of 0 means validation passed; 1 means there are syntax errors.

## --verbose Mode

Add `--verbose` (or `-v`) to see detailed execution logs:

```bash
juglans src/main.jg --verbose
```

Output includes:

```
[DEBUG] Loading workflow: main.jg
[DEBUG] Parsed 5 nodes, 4 edges
[INFO]  [init] Starting...
[DEBUG] [init] Output: null
[INFO]  [chat] Calling agent: assistant
[DEBUG] [chat] Request: {"message": "..."}
[INFO]  [chat] Response received (234 tokens)
```

You can also set the log level via an environment variable:

```bash
JUGLANS_LOG_LEVEL=debug juglans src/main.jg
```

## juglans doctest -- Validate Doc Code Blocks

`juglans doctest` extracts all ` ```juglans ` code blocks from Markdown files and validates their syntax through `GraphParser::parse()`.

```bash
# Validate a single file
juglans doctest docs/guide/concepts.md

# Validate an entire directory
juglans doctest docs/

# JSON output
juglans doctest docs/ --format json
```

Code blocks that should not be validated can be marked with `ignore`:

````text
```juglans,ignore
[broken]: this_is_intentionally_invalid
```
````

## Common Errors

| # | Error | Cause | Solution |
|---|-------|-------|----------|
| 1 | `Duplicate node ID: X` | Two nodes in the same workflow share a name | Rename one of the nodes |
| 2 | `Edge references undefined node: X` | An edge references a node that has not been defined | Check the node name spelling; make sure nodes are defined before edges |
| 3 | `Node 'X' is not reachable from entry node` (W002) | A node has no path from the entry node | Connect the node to the graph or remove it |
| 4 | `No API-key provided` | No LLM provider configured for `chat()` | Set `OPENAI_API_KEY`/`ANTHROPIC_API_KEY`/etc., or define `[ai.providers]` in `juglans.toml` |
| 5 | `Agent not found: X` | The agent slug does not exist | Check the spelling and ensure the corresponding file is imported via `libs:` |
| 6 | `Tool not found: X` | An unregistered tool was called | Verify the tool name is a builtin or that the MCP server is configured |
| 7 | `Cycle detected involving node 'X'. Workflows must be acyclic (DAG).` (E002) | The graph contains a cycle | Review the edge definitions; DAGs do not allow cycles (use `while`/`foreach` instead) |
| 8 | `Parse error at line N` | DSL syntax error | Check the syntax near that line: bracket matching, quote closure, parameter format |
| 9 | `Variable not found: X` | A context variable was not set | Ensure the variable is set via assignment syntax before it is used |
| 10 | `Timeout` | Tool execution timed out | Check the network connection or increase the timeout configuration |

## Debugging Tips

### Insert Checkpoints with print()

Insert `print()` nodes at key positions to inspect intermediate state:

```juglans
[step1]: data = input.items
[debug1]: print(message="After step1, data = " + json(data))
[step2]: chat(agent="processor", message=json(data))
[debug2]: print(message="After step2, output = " + json(output))
[done]: notify(status="complete")

[step1] -> [debug1] -> [step2] -> [debug2] -> [done]
```

### Save Intermediate State with Assignment

Use assignment syntax to save intermediate results so they can be referenced by later nodes or error paths:

```juglans
[fetch]: fetch_url(url=input.url)
[save_raw]: raw_response = output
[process]: chat(agent="parser", message=output)
[save_parsed]: parsed = output
[handle_error]: print(message="Failed. Raw response: " + json(raw_response))
[done]: print(message="Result: " + json(parsed))

[fetch] -> [save_raw] -> [process] -> [save_parsed] -> [done]
[process] on error -> [handle_error]
```

### Dry-run Mode

Use `--dry-run` to parse without executing, quickly verifying the structure:

```bash
juglans src/main.jg --dry-run
```

### Isolate Problem Nodes

When a workflow is long, create a minimal test file to test the problematic node in isolation:

```bash
# Render a single Prompt
juglans src/prompts/my-prompt.jgx --input '{"name": "Alice"}'

# Test a minimal workflow with an inline agent
juglans test-agent.jg --input '{"message": "test input"}'
```

### Check Configuration

Verify that the current configuration is correct:

```bash
# View account and configuration info
juglans whoami --verbose
```

## Next Steps

- [Testing Workflows](./testing.md) -- Systematic testing methods
- [Error Handling](../tutorials/error-handling.md) -- Handling errors in workflows
- [CLI Reference](../reference/cli.md) -- Complete command reference
