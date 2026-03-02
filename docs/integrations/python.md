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

```yaml
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

```yaml
# Call pandas.read_csv
[load]: pandas.read_csv("data.csv")

# Call methods on returned objects
[stats]: $load.describe()

# Chained calls
[clean]: $load.dropna().reset_index()

# Local module calls
[result]: utils.process($clean)
```

### 3. Complete Example

```yaml
name: "Sales Analysis"

python: ["pandas", "json"]
agents: ["./agents/analyst.jgagent"]

entry: [load]
exit: [report]

# Load CSV data
[load]: pandas.read_csv("sales.csv")

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

```yaml
python: ["pandas", "numpy", "json"]
```

### Submodules

```yaml
python: ["sklearn.ensemble", "sklearn.preprocessing"]
```

Usage:
```yaml
[model]: sklearn.ensemble.RandomForestClassifier(n_estimators=100)
```

### Local .py Files

```yaml
python: [
    "./utils.py",           # Relative to the .jg file
    "./lib/helpers.py",     # Subdirectory
    "/absolute/path.py"     # Absolute path
]
```

Functions in the file can be called directly:

```python
# ./utils.py
def process_data(df, threshold=0.5):
    return df[df['score'] > threshold]
```

```yaml
[result]: utils.process_data($input, threshold=0.8)
```

### Glob Patterns

```yaml
python: ["./processors/*.py"]  # Import all .py files in the directory
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

```yaml
[df]: pandas.read_csv("large_file.csv")
# Python returns: {"ref": "ref:obj:001", "type": "DataFrame"}
# Juglans stores: $df = Ref("ref:obj:001")

[filtered]: $df.query("score > 0.5")
# Juglans sends: {"target": "ref:obj:001", "method": "query", "args": ["score > 0.5"]}
# Python returns: {"ref": "ref:obj:002"}

[result]: $filtered.to_dict()
# Returns actual JSON data, no longer needs a reference
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

```yaml
python: ["risky_module"]

[risky]: risky_module.might_fail($input)
[risky] -> [next]
[risky] on error -> [handle]

[handle]: notify(message="Python error: " + $error.message)
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

```yaml
# Good: Explicit imports
python: ["pandas", "json"]

# Avoid: Too many imports
python: ["pandas", "numpy", "scipy", "sklearn", ...]
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

```yaml
python: ["./processors/data.py"]

[clean]: data.preprocess($raw)  # Concise call
```

### 3. Avoid Frequent Calls in Loops

```yaml
# Bad: Calls Python on every iteration
[process]: foreach($item in $input.items) {
    [call]: utils.process($item)  # Multiple inter-process communications
}

# Good: Batch processing
[batch]: utils.process_batch($input.items)  # Single call
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
