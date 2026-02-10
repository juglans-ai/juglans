# Python 生态集成

Juglans 2.0 支持直接调用 Python 生态系统，无需在 DSL 中重新实现数据处理、机器学习等功能。

## 设计理念

```
┌─────────────────────────────────────────┐
│            .jgflow (编排层)              │
│  - 图结构、控制流、AI 原语               │
│  - 不关心节点"怎么实现"                  │
└────────────────┬────────────────────────┘
                 ▼
┌─────────────────────────────────────────┐
│           Python 生态                    │
│  pandas, sklearn, matplotlib...         │
│  直接调用，无需适配                       │
└─────────────────────────────────────────┘
```

## 快速开始

### 1. 导入模块

在工作流头部声明需要使用的 Python 模块：

```yaml
name: "Data Analysis"

# Python 模块导入
python: [
    "pandas",                    # 系统模块
    "sklearn.ensemble",          # 子模块
    "./utils.py",                # 本地 .py 文件
    "./lib/*.py"                 # glob 模式
]

entry: [load]
exit: [done]
```

### 2. 透明调用

导入后，可以直接调用模块函数：

```yaml
# 调用 pandas.read_csv
[load]: pandas.read_csv("data.csv")

# 调用返回对象的方法
[stats]: $load.describe()

# 链式调用
[clean]: $load.dropna().reset_index()

# 本地模块调用
[result]: utils.process($clean)
```

### 3. 完整示例

```yaml
name: "Sales Analysis"

python: ["pandas", "json"]
agents: ["./agents/analyst.jgagent"]

entry: [load]
exit: [report]

# 加载 CSV 数据
[load]: pandas.read_csv("sales.csv")

# 数据预处理
[clean]: $load.dropna()
[summary]: $clean.describe()

# AI 分析
[analysis]: chat(
    agent="analyst",
    message="分析这份销售数据摘要",
    format="json"
)

# 生成报告
[report]: notify(message="分析完成: " + $analysis.output)

[load] -> [clean] -> [summary] -> [analysis] -> [report]
```

## 导入语法

### 系统模块

```yaml
python: ["pandas", "numpy", "json"]
```

### 子模块

```yaml
python: ["sklearn.ensemble", "sklearn.preprocessing"]
```

使用时：
```yaml
[model]: sklearn.ensemble.RandomForestClassifier(n_estimators=100)
```

### 本地 .py 文件

```yaml
python: [
    "./utils.py",           # 相对于 .jgflow 文件
    "./lib/helpers.py",     # 子目录
    "/absolute/path.py"     # 绝对路径
]
```

文件中的函数可直接调用：

```python
# ./utils.py
def process_data(df, threshold=0.5):
    return df[df['score'] > threshold]
```

```yaml
[result]: utils.process_data($input, threshold=0.8)
```

### Glob 模式

```yaml
python: ["./processors/*.py"]  # 导入目录下所有 .py 文件
```

## 对象引用系统

### 问题

Python 对象（如 DataFrame、Model）无法直接序列化传递给 Juglans。

### 解决方案

Juglans 使用对象引用系统：

```
┌─────────────┐                  ┌─────────────┐
│   Juglans   │  ref:obj:12345   │   Python    │
│   (Rust)    │ ◄──────────────► │   Worker    │
│             │                  │  (实际对象)  │
└─────────────┘                  └─────────────┘
```

- 大对象保留在 Python 进程内存
- Juglans 持有引用 ID（如 `ref:obj:12345`）
- 方法调用通过引用 ID 路由到 Python

### 示例

```yaml
[df]: pandas.read_csv("large_file.csv")
# Python 返回: {"ref": "ref:obj:001", "type": "DataFrame"}
# Juglans 存储: $df = Ref("ref:obj:001")

[filtered]: $df.query("score > 0.5")
# Juglans 发送: {"target": "ref:obj:001", "method": "query", "args": ["score > 0.5"]}
# Python 返回: {"ref": "ref:obj:002"}

[result]: $filtered.to_dict()
# 返回实际 JSON 数据，不再需要引用
```

### 生命周期

- 工作流执行期间，引用保持有效
- 工作流结束时，自动发送 GC 消息释放所有引用
- 同一工作流内可自由传递引用

## 运行时架构

### Worker 进程

Juglans 启动 Python Worker 子进程处理调用：

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

### 通信协议

**请求格式**：
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

**响应格式**：
```json
{
    "id": "req-001",
    "type": "value",
    "value": {...},
    "ref": "ref:obj:001"
}
```

### Worker 池（可选）

对于高并发场景，可配置多个 Worker：

```toml
# juglans.toml
[python]
workers = 4          # Worker 数量
timeout = 30000      # 超时时间 (ms)
```

## 错误处理

Python 异常会被捕获并转换为工作流错误：

```yaml
python: ["risky_module"]

[risky]: risky_module.might_fail($input)
[risky] -> [next]
[risky] on error -> [handle]

[handle]: notify(message="Python error: " + $error.message)
```

错误对象包含：
```json
{
    "type": "PythonError",
    "message": "ValueError: invalid input",
    "traceback": "..."
}
```

## 与内置函数的区别

| 调用方式 | 来源 | 说明 |
|---------|------|------|
| `chat()`, `sh()`, `fetch()` | 内置 | 无需导入 |
| `pandas.read_csv()` | Python | 需要 `python: ["pandas"]` |
| `mcp_tool()` | MCP | 需要配置 MCP 服务器 |

**解析顺序**：
1. 检查是否内置函数
2. 检查是否在 `python:` 列表中声明
3. 检查是否 MCP 工具
4. 报错 "Unknown function"

## 最佳实践

### 1. 只导入需要的模块

```yaml
# 好：明确导入
python: ["pandas", "json"]

# 避免：导入过多
python: ["pandas", "numpy", "scipy", "sklearn", ...]
```

### 2. 本地模块封装复杂逻辑

```python
# ./processors/data.py
def preprocess(df):
    """封装复杂的数据预处理逻辑"""
    df = df.dropna()
    df = df.reset_index()
    df['normalized'] = (df['value'] - df['value'].mean()) / df['value'].std()
    return df
```

```yaml
python: ["./processors/data.py"]

[clean]: data.preprocess($raw)  # 简洁调用
```

### 3. 避免在循环中频繁调用

```yaml
# 不好：每次迭代都调用 Python
[process]: foreach($item in $input.items) {
    [call]: utils.process($item)  # 多次进程通信
}

# 好：批量处理
[batch]: utils.process_batch($input.items)  # 单次调用
```

### 4. 使用类型提示（本地模块）

```python
# ./utils.py
from typing import List, Dict

def analyze(data: List[Dict]) -> Dict:
    """类型提示帮助调试"""
    ...
```

## 限制与注意事项

1. **进程开销**：Python 调用涉及进程间通信，比内置函数慢
2. **序列化限制**：只能传递 JSON 可序列化的数据（或使用对象引用）
3. **依赖管理**：确保 Python 环境已安装所需模块
4. **内存管理**：大对象引用会占用 Python 进程内存
5. **并发限制**：单 Worker 模式下调用是串行的

## 调试技巧

### 查看 Python 调用日志

```bash
RUST_LOG=juglans::runtime::python=debug juglans workflow.jgflow
```

### 测试 Python 模块

```bash
# 验证模块可导入
python -c "import pandas; print(pandas.__version__)"

# 验证本地模块
python -c "import sys; sys.path.insert(0, '.'); import utils"
```
