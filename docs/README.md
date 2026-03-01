# Juglans

**Juglans** is the first AI orchestration language that treats **graph topology as a first-class programming primitive**.

> Others write code to draw graphs. We write graphs as code.

In traditional programming, execution is fundamentally sequential — even async/await is syntactic sugar over linear control flow. In traditional workflow tools, the graph is a scheduling artifact generated from code. **Juglans inverts this: the graph topology IS the program.**

## Why Topology Matters

In the era of AI agents, the **structure of how agents interact** — who talks to whom, in what order, with what branching — is often more important than any individual agent's capability.

Juglans makes this structure **explicit, composable, and verifiable**:

```yaml
[classify] -> switch $output.intent {
    "question": [answer]
    "task": [execute]
    default: [fallback]
}
[answer] -> [review]
[execute] -> [review]
[review] if $output.quality < 0.8 -> [refine]
[refine] -> [review]
```

The topology of this code — branching, convergence, cycles — **IS** the architecture diagram. No separate drawing needed.

## What Makes Juglans Different

### Node = Function

A node is simultaneously a **graph vertex** (with edges, topological position) and a **callable function** (with parameters, a body, reusable). Function calls don't flatten the topology — sub-graphs are expanded in place, preserving structure.

```yaml
[deploy(env, version)]: {
  bash(command="docker build -t app:" + $version + " .")
  bash(command="kubectl apply --namespace=" + $env)
}

[staging]: deploy(env="staging", version="1.2.0")
[production]: deploy(env="production", version="1.2.0")
[staging] -> [production]
```

### Topology-Preserving Composition

`flows:` imports perform **graph merging** — the sub-workflow's complete topology (nodes, edges, branches) is embedded into the parent graph with namespace prefixes. This is an **embedding**, not a projection. No structural information is lost.

```yaml
flows: {
  auth: "./auth.jg"
  trading: "./trading.jg"
}
[route] if $ctx.need_auth -> [auth.start]
[auth.done] -> [trading.begin]
```

Compare: Python function calls flatten the call stack. Microservice calls hide network topology. Juglans sub-graphs remain **fully visible, reasonnable, and optimizable** within the parent graph.

### Computation Topology = API Topology

With `serve()`, the routing topology of your HTTP API and the execution topology of your computation are **the same graph**:

```yaml
[request]: serve()
[request] -> switch $input.route {
  "GET /api/hello": [hello]
  "POST /api/data": [process]
  default: [not_found]
}
```

### How We Compare

| Paradigm | Role of Graph | Limitation |
|----------|--------------|------------|
| Airflow / Prefect | Python code **generates** DAG; graph is a scheduling artifact | Graph is a second-class citizen |
| Terraform | Declarative dependency graph | No control flow, no functions, no runtime branching |
| BPMN / workflow engines | Visual XML graphs | Verbose, no function abstraction, not composable |
| LangGraph / CrewAI | State machines between agents | State machines are not graphs — no true topological composition |
| **Juglans** | **Graph topology IS the program** | — |

## Features

- **Declarative DSL** — Three file formats: `.jg` (workflows), `.jgprompt` (templates), `.jgagent` (agents)
- **Graph Execution Engine** — DAG traversal with conditionals, switch routing, loops (`while`, `foreach`), error handling
- **Function Definitions** — `[name(params)]: { steps }` — reusable parameterized node blocks
- **Topology-Preserving Composition** — `flows:` merges sub-workflow graphs with namespace isolation
- **Template System** — Jinja-style prompt templates with variable interpolation
- **Expression Language** — Python-like expressions with 30+ built-in functions
- **HTTP Backend** — `serve()` + `response()` turn workflows into HTTP APIs
- **Multi-Agent Collaboration** — Declarative agent interaction topology
- **MCP Integration** — Model Context Protocol for extensible tool capabilities
- **Client Tool Bridge** — Unresolved tool calls forwarded to frontend via SSE
- **Jug0 Backend** — Seamless integration with Jug0 AI platform
- **Cross-Platform** — Native + WebAssembly

## 安装

### 快速安装 (macOS & Linux)

```bash
curl -fsSL https://juglans.ai/get-sdk | sh
```

或使用 GitHub 直链：

```bash
curl -fsSL https://raw.githubusercontent.com/juglans-ai/juglans/main/install.sh | sh
```

### macOS (Homebrew)

```bash
# 添加 tap
brew tap juglans-ai/tap

# 安装
brew install juglans

# 验证安装
juglans --version
```

### Linux

```bash
# 使用安装脚本（推荐）
curl -fsSL https://juglans.ai/get-sdk | sh

# 或从 Releases 下载
wget https://github.com/juglans-ai/juglans/releases/latest/download/juglans-linux-x64.tar.gz
tar -xzf juglans-linux-x64.tar.gz
sudo mv juglans /usr/local/bin/
```

### Windows

```powershell
# PowerShell 安装
irm https://juglans.ai/get-sdk.ps1 | iex

# 或手动下载
# https://github.com/juglans-ai/juglans/releases/latest
```

### 从源码构建

```bash
# 前置要求: Rust 1.70+
git clone https://github.com/juglans-ai/juglans.git
cd juglans
cargo build --release

# 二进制文件位置: target/release/juglans
```

## 快速开始

### 创建第一个 Agent

```yaml
# my-agent.jgagent
slug: "my-assistant"
name: "My Assistant"
model: "deepseek-chat"
temperature: 0.7
system_prompt: "You are a helpful assistant."
```

运行：
```bash
juglans my-agent.jgagent
```

### 从源码构建

```bash
# 前置要求: Rust 1.70+
git clone https://github.com/juglans-ai/juglans.git
cd juglans
cargo build --release

# 二进制文件位置: target/release/juglans
```

## 快速开始

### 创建 Prompt 模板

```yaml
# greeting.jgprompt
---
slug: "greeting"
name: "Greeting Prompt"
inputs: {name: "World"}
---
Hello, {{ name }}! How can I help you today?
```

### 创建工作流

```yaml
# chat-flow.jg
name: "Simple Chat"
version: "0.1.0"

prompts: ["./prompts/*.jgprompt"]
agents: ["./agents/*.jgagent"]

entry: [start]
exit: [end]

[start]: notify(status="Starting chat...")
[chat]: chat(agent="my-assistant", message=$input.question)
[end]: notify(status="Done")

[start] -> [chat] -> [end]
```

运行：
```bash
juglans chat-flow.jg --input '{"question": "What is Juglans?"}'
```

## 文档

- [快速入门](./getting-started/quickstart.md) - 5 分钟上手教程
- [核心概念](./guide/concepts.md) - Agent、Prompt、Workflow 介绍
- [DSL 语法](./guide/workflow-syntax.md) - 完整语法参考
- [CLI 参考](./reference/cli.md) - 命令行工具
- [内置工具](./reference/builtins.md) - chat、p、notify 等

## 架构

```
┌─────────────────────────────────────────────────────────┐
│                      Juglans CLI                        │
├─────────────────────────────────────────────────────────┤
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐     │
│  │  .jg    │  │  .jgprompt  │  │  .jgagent   │     │
│  │  Parser     │  │  Parser     │  │  Parser     │     │
│  └──────┬──────┘  └──────┬──────┘  └──────┬──────┘     │
│         │                │                │             │
│         ▼                ▼                ▼             │
│  ┌─────────────────────────────────────────────────┐   │
│  │              Workflow Executor                   │   │
│  │         (DAG Traversal + Context)               │   │
│  └──────────────────────┬──────────────────────────┘   │
│                         │                               │
│         ┌───────────────┼───────────────┐              │
│         ▼               ▼               ▼              │
│  ┌───────────┐   ┌───────────┐   ┌───────────┐        │
│  │  Builtins │   │  Jug0     │   │   MCP     │        │
│  │  (chat,p) │   │  Client   │   │  Client   │        │
│  └───────────┘   └───────────┘   └───────────┘        │
└─────────────────────────────────────────────────────────┘
```

## 文件格式

| 扩展名 | 用途 | 说明 |
|--------|------|------|
| `.jg` | 工作流 | 定义节点、边、执行逻辑 |
| `.jgprompt` | Prompt 模板 | 可复用的提示词模板 |
| `.jgagent` | Agent 配置 | 模型、温度、系统提示 |
| `juglans.toml` | 项目配置 | API 密钥、服务器设置 |

## 配置文件

创建 `juglans.toml` 来配置项目：

```toml
# 账户配置
[account]
id = "user_123"
name = "Your Name"
role = "admin"
api_key = "jug0_sk_your_api_key_here"

# 工作空间配置（可选）
[workspace]
id = "workspace_456"
name = "My Workspace"
members = ["user_123"]

# Jug0 后端配置
[jug0]
base_url = "http://localhost:3000"  # 本地开发
# base_url = "https://api.jug0.com"  # 生产环境

# Web 服务器配置
[server]
host = "127.0.0.1"
port = 8080

# 环境变量（可选）
[env]
DATABASE_URL = "postgresql://localhost/mydb"
API_KEY = "your_api_key"

# MCP 服务器配置（可选）
[mcp.filesystem]
command = "npx"
args = ["-y", "@anthropic/mcp-filesystem"]
env = { ROOT_DIR = "/workspace" }

[mcp.web-browser]
url = "http://localhost:3001/mcp"
api_key = "mcp_key_..."
```

详细配置说明请参考 [配置文件参考](./reference/config.md)。

## 示例

查看 [examples/](../examples/) 目录获取更多示例：

- `examples/prompts/` - Prompt 模板示例
- `examples/agents/` - Agent 配置示例
- `examples/workflows/` - 工作流示例

## 技术栈

- **解析器**: Pest (PEG 语法)
- **脚本引擎**: Rhai (表达式求值)
- **图结构**: Petgraph (DAG)
- **异步运行时**: Tokio
- **Web 框架**: Axum
- **序列化**: Serde

## License

MIT License
