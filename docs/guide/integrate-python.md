# How to Integrate Python

Juglans can call Python modules directly -- pandas, sklearn, your own `.py` files -- without writing adapters. Large objects stay in Python memory; Juglans holds lightweight references.

## Declare Python Modules

Use the `python:` header to import modules:

```juglans
python: [
    "pandas",                # System package
    "sklearn.ensemble",      # Submodule
    "./utils.py",            # Local file (relative to .jg)
    "./lib/*.py"             # Glob pattern
]

[start]: notify(status="ready")
```

- System modules: must be installed in the Python environment
- Local files: resolved relative to the `.jg` file's directory
- Glob patterns: import all matching `.py` files

## Call Python Functions

Use `module.function(key=value)` format, just like calling a builtin:

```juglans
python: ["pandas", "./utils.py"]

# Call pandas.read_csv
[load]: pandas.read_csv(path="data.csv")

# Call a local module function
[result]: utils.process(data=output)

[load] -> [result]
```

## Object Reference System

Python objects like DataFrames cannot be serialized to JSON. Juglans solves this with references:

1. Python returns a reference ID (e.g., `ref:obj:001`) instead of the full object
2. The actual object stays in Python process memory
3. Method calls on the reference are routed back to Python

```juglans
python: ["pandas"]

# Returns a reference, not the full DataFrame
[df]: pandas.read_csv(path="large_file.csv")

# Method call on the reference -- executed in Python
[filtered]: df.query(expr="score > 0.5")

# Convert to JSON when you need the actual data
[result]: filtered.to_dict()

[df] -> [filtered] -> [result]
```

References are valid for the lifetime of the workflow. When the workflow ends, all references are automatically garbage collected.

## Method Chain Calls

Use `node_id.method()` to call methods on Python object references:

```juglans
python: ["pandas"]

[load]: pandas.read_csv(path="sales.csv")
[dropped]: load.dropna()
[clean]: dropped.reset_index()

[load] -> [dropped] -> [clean]
```

Each step produces a new reference. The chain runs in Python, so only reference IDs cross the process boundary.

## Error Handling

Python exceptions are caught and converted to workflow errors. Use `on error` to handle them:

```juglans
python: ["risky_module"]

[risky]: risky_module.might_fail(data=input)
[done]: notify(message="Success")
[handle]: notify(message="Python error: " + error.message)

[risky] -> [done]
[risky] on error -> [handle]
```

The `error` object contains:

| Field | Description |
|-------|-------------|
| `error.type` | Exception class (e.g., `PythonError`) |
| `error.message` | Error message (e.g., `ValueError: invalid input`) |
| `error.traceback` | Full Python traceback |

## Best Practices

**Batch over loops** -- Minimize cross-process calls. Instead of processing items one by one in a `foreach`, pass the whole batch:

```juglans
python: ["./utils.py"]

# Good: single call, Python handles the loop
[batch]: utils.process_batch(items=input.items)
```

**Encapsulate complex logic** -- Keep `.jg` nodes simple. Put multi-step data processing in a Python function:

```python
# ./processors/data.py
def preprocess(df):
    df = df.dropna()
    df = df.reset_index()
    df['normalized'] = (df['value'] - df['value'].mean()) / df['value'].std()
    return df
```

```juglans
python: ["./processors/data.py"]

[clean]: data.preprocess(df=input)
```

**Only import what you need** -- Each declared module starts a worker process import. Keep the list minimal.

## Configure Workers

For high-concurrency scenarios, configure multiple Python workers in `juglans.toml` under the `[limits]` section:

```toml
[limits]
python_workers = 4        # Number of Python worker processes
```

## Debugging

```bash
# View Python call logs
RUST_LOG=juglans::runtime::python=debug juglans workflow.jg

# Verify a module is importable
python -c "import pandas; print(pandas.__version__)"
```
