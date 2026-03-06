# Python Ecosystem Integration

Juglans 2.0 supports directly calling the Python ecosystem, eliminating the need to reimplement data processing, machine learning, and other functionality in the DSL.

## Design Philosophy

```
┌─────────────────────────────────────────┐
│        .jg (Orchestration Layer)        │
│  - Graph structure, control flow,       │
│    AI primitives                        │
│  - Doesn't care "how" a node is        │
│    implemented                          │
└────────────────┬────────────────────────┘
                 ▼
┌─────────────────────────────────────────┐
│           Python Ecosystem              │
│  pandas, sklearn, matplotlib...         │
│  Direct calls, no adapters needed       │
└─────────────────────────────────────────┘
```

## Quick Start

### 1. Import Modules

Declare the Python modules you need in the workflow header:

```juglans
name: "Data Analysis"

# Python module imports
python: [
    "pandas",                    # System module
    "sklearn.ensemble",          # Submodule
    "./utils.py",                # Local .py file
    "./lib/*.py"                 # Glob pattern
]

entry: [load]
exit: [done]
```

### 2. Transparent Calls

After importing, you can call module functions directly:

```juglans
entry: [load]
exit: [result]

# Call pandas.read_csv
[load]: pandas.read_csv(path="data.csv")

# Call methods on returned objects
[stats]: $load.describe()

# Step-by-step method calls
[dropped]: $load.dropna()
[clean]: $dropped.reset_index()

# Local module calls
[result]: utils.process(data=$clean)

[load] -> [stats] -> [dropped] -> [clean] -> [result]
```

### 3. Complete Example

```juglans
name: "Sales Analysis"

python: ["pandas", "json"]
agents: ["./agents/analyst.jgagent"]

entry: [load]
exit: [report]

# Load CSV data
[load]: pandas.read_csv(path="sales.csv")

# Data preprocessing
[clean]: $load.dropna()
[summary]: $clean.describe()

# AI analysis
[analysis]: chat(
    agent="analyst",
    message="Analyze this sales data summary",
    format="json"
)

# Generate report
[report]: notify(message="Analysis complete: " + $analysis.output)

[load] -> [clean] -> [summary] -> [analysis] -> [report]
```

## Import Syntax

### System Modules

```juglans
entry: [start]
exit: [start]
python: ["pandas", "numpy", "json"]
[start]: notify(status="ready")
```

### Submodules

```juglans
entry: [start]
exit: [start]
python: ["sklearn.ensemble", "sklearn.preprocessing"]
[start]: notify(status="ready")
```

Usage:
```juglans
[model]: sklearn.ensemble.RandomForestClassifier(n_estimators=100)
```

### Local .py Files

```juglans
entry: [start]
exit: [start]
python: [
    "./utils.py",           # Relative to the .jg file
    "./lib/helpers.py",     # Subdirectory
    "/absolute/path.py"     # Absolute path
]
[start]: notify(status="ready")
```

Functions in the file can be called directly:

```python
# ./utils.py
def process_data(df, threshold=0.5):
    return df[df['score'] > threshold]
```

```juglans
entry: [result]
exit: [result]
[result]: utils.process_data(data=$input, threshold=0.8)
```

### Glob Patterns

```juglans
entry: [start]
exit: [start]
python: ["./processors/*.py"]  # Import all .py files in the directory
[start]: notify(status="ready")
```

## Object Reference System

### Problem

Python objects (such as DataFrames, Models) cannot be directly serialized and passed to Juglans.

### Solution

Juglans uses an object reference system:

```
┌─────────────┐                  ┌─────────────┐
│   Juglans   │  ref:obj:12345   │   Python    │
│   (Rust)    │ ◄──────────────► │   Worker    │
│             │                  │ (actual obj) │
└─────────────┘                  └─────────────┘
```

- Large objects remain in the Python process memory
- Juglans holds a reference ID (e.g., `ref:obj:12345`)
- Method calls are routed to Python via the reference ID

### Example

```juglans
entry: [df]
exit: [result]

[df]: pandas.read_csv(path="large_file.csv")
# Python returns: {"ref": "ref:obj:001", "type": "DataFrame"}
# Juglans stores: $df = Ref("ref:obj:001")

[filtered]: $df.query(expr="score > 0.5")
# Juglans sends: {"target": "ref:obj:001", "method": "query", "args": ["score > 0.5"]}
# Python returns: {"ref": "ref:obj:002"}

[result]: $filtered.to_dict()
# Returns actual JSON data, no longer needs a reference

[df] -> [filtered] -> [result]
```

### Lifecycle

- References remain valid during workflow execution
- When the workflow ends, a GC message is automatically sent to release all references
- References can be freely passed within the same workflow

## Runtime Architecture

### Worker Process

Juglans starts a Python Worker subprocess to handle calls:

```
                    Juglans (Rust)
                         │
                    stdin/stdout
                    JSON-RPC
                         │
                         ▼
                  Python Worker
                  (subprocess)
```

### Communication Protocol

**Request format**:
```json
{
    "id": "req-001",
    "type": "call",
    "target": "pandas",
    "method": "read_csv",
    "args": ["data.csv"],
    "kwargs": {}
}
```

**Response format**:
```json
{
    "id": "req-001",
    "type": "value",
    "value": {...},
    "ref": "ref:obj:001"
}
```

### Worker Pool (Optional)

For high-concurrency scenarios, you can configure multiple Workers:

```toml
# juglans.toml
[python]
workers = 4          # Number of workers
timeout = 30000      # Timeout (ms)
```

## Error Handling

Python exceptions are caught and converted to workflow errors:

```juglans
entry: [risky]
exit: [done]
python: ["risky_module"]

[risky]: risky_module.might_fail(data=$input)
[done]: notify(message="Success")
[handle]: notify(message="Python error: " + $error.message)

[risky] -> [done]
[risky] on error -> [handle]
```

The error object contains:
```json
{
    "type": "PythonError",
    "message": "ValueError: invalid input",
    "traceback": "..."
}
```

## Differences from Built-in Functions

| Call Method | Source | Description |
|-------------|--------|-------------|
| `chat()`, `sh()`, `fetch()` | Built-in | No import needed |
| `pandas.read_csv()` | Python | Requires `python: ["pandas"]` |
| `mcp_tool()` | MCP | Requires MCP server configuration |

**Resolution order**:
1. Check if it is a built-in function
2. Check if it is declared in the `python:` list
3. Check if it is an MCP tool
4. Report "Unknown function" error

## Best Practices

### 1. Only Import Modules You Need

```juglans
entry: [start]
exit: [start]

# Good: Explicit imports
python: ["pandas", "json"]

[start]: notify(status="ready")
```

### 2. Encapsulate Complex Logic in Local Modules

```python
# ./processors/data.py
def preprocess(df):
    """Encapsulate complex data preprocessing logic"""
    df = df.dropna()
    df = df.reset_index()
    df['normalized'] = (df['value'] - df['value'].mean()) / df['value'].std()
    return df
```

```juglans
entry: [clean]
exit: [clean]
python: ["./processors/data.py"]

[clean]: data.preprocess(df=$raw)  # Concise call
```

### 3. Avoid Frequent Calls in Loops

```juglans
entry: [process]
exit: [batch]

# Bad: Calls Python on every iteration
[process]: foreach($item in $input.items) {
    [call]: utils.process(data=$item)  # Multiple inter-process communications
    [save]: set_context(last=$output)
    [call] -> [save]
}

# Good: Batch processing
[batch]: utils.process_batch(items=$input.items)  # Single call

[process] -> [batch]
```

### 4. Use Type Hints (Local Modules)

```python
# ./utils.py
from typing import List, Dict

def analyze(data: List[Dict]) -> Dict:
    """Type hints help with debugging"""
    ...
```

## Limitations and Caveats

1. **Process overhead**: Python calls involve inter-process communication, which is slower than built-in functions
2. **Serialization limitations**: Only JSON-serializable data can be passed (or use object references)
3. **Dependency management**: Ensure the Python environment has the required modules installed
4. **Memory management**: Large object references occupy Python process memory
5. **Concurrency limitations**: Calls are serial in single Worker mode

## Debugging Tips

### View Python Call Logs

```bash
RUST_LOG=juglans::runtime::python=debug juglans workflow.jg
```

### Test Python Modules

```bash
# Verify module can be imported
python -c "import pandas; print(pandas.__version__)"

# Verify local module
python -c "import sys; sys.path.insert(0, '.'); import utils"
```
