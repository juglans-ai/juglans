# CLI 命令参考

Juglans CLI 提供工作流执行、资源管理和开发工具。

## 安装

```bash
# 从源码构建
git clone https://github.com/juglans-ai/juglans.git
cd juglans
cargo build --release

# 安装到系统
cargo install --path .

# 或添加到 PATH
export PATH="$PATH:$(pwd)/target/release"
```

## 基本用法

```bash
juglans [OPTIONS] <FILE> [ARGS]
juglans <COMMAND> [OPTIONS]
```

## 执行命令

### 执行工作流

```bash
juglans path/to/workflow.jgflow [OPTIONS]
```

**选项：**

| 选项 | 说明 |
|------|------|
| `--input <JSON>` | 输入数据 (JSON 格式) |
| `--input-file <FILE>` | 从文件读取输入 |
| `--verbose`, `-v` | 详细输出 |
| `--dry-run` | 仅解析，不执行 |
| `--output <FILE>` | 输出结果到文件 |

**示例：**

```bash
# 基本执行
juglans workflows/main.jgflow

# 传入输入
juglans workflows/main.jgflow --input '{"query": "Hello"}'

# 从文件读取输入
juglans workflows/main.jgflow --input-file input.json

# 详细模式
juglans workflows/main.jgflow -v

# 仅验证
juglans workflows/main.jgflow --dry-run
```

---

### 执行 Agent (交互模式)

```bash
juglans path/to/agent.jgagent [OPTIONS]
```

**选项：**

| 选项 | 说明 |
|------|------|
| `--message <TEXT>` | 初始消息 |
| `--verbose`, `-v` | 详细输出 |
| `--info` | 显示 Agent 信息 |

**示例：**

```bash
# 交互对话
juglans agents/assistant.jgagent

# 发送单条消息
juglans agents/assistant.jgagent --message "What is Rust?"

# 查看配置
juglans agents/assistant.jgagent --info
```

**交互命令：**

在交互模式中：
- 输入消息发送给 Agent
- `exit` 或 `quit` 退出
- `clear` 清除历史
- `history` 查看对话历史

---

### 渲染 Prompt

```bash
juglans path/to/prompt.jgprompt [OPTIONS]
```

**选项：**

| 选项 | 说明 |
|------|------|
| `--input <JSON>` | 模板变量 |
| `--output <FILE>` | 输出到文件 |

**示例：**

```bash
# 使用默认值渲染
juglans prompts/greeting.jgprompt

# 传入变量
juglans prompts/greeting.jgprompt --input '{"name": "Alice"}'

# 输出到文件
juglans prompts/greeting.jgprompt --output rendered.txt
```

---

## 项目命令

### init - 初始化项目

```bash
juglans init <PROJECT_NAME> [OPTIONS]
```

**选项：**

| 选项 | 说明 |
|------|------|
| `--template <NAME>` | 使用模板 (basic, advanced) |

**示例：**

```bash
# 创建新项目
juglans init my-project

# 使用高级模板
juglans init my-project --template advanced
```

**生成结构：**

```
my-project/
├── juglans.toml
├── prompts/
│   └── example.jgprompt
├── agents/
│   └── example.jgagent
└── workflows/
    └── example.jgflow
```

---

### install - 安装依赖

获取 MCP 工具 schema：

```bash
juglans install [OPTIONS]
```

**选项：**

| 选项 | 说明 |
|------|------|
| `--force` | 强制重新获取 |

**示例：**

```bash
# 安装 MCP 工具
juglans install

# 强制刷新
juglans install --force
```

---

## 资源管理

### apply - 推送资源

将本地资源推送到 Jug0 后端：

```bash
juglans apply <FILE> [OPTIONS]
```

**选项：**

| 选项 | 说明 |
|------|------|
| `--force` | 覆盖已存在的资源 |

**示例：**

```bash
# 推送 Prompt
juglans apply prompts/my-prompt.jgprompt

# 推送 Agent
juglans apply agents/my-agent.jgagent

# 推送 Workflow
juglans apply workflows/my-flow.jgflow

# 强制覆盖
juglans apply prompts/my-prompt.jgprompt --force
```

---

### pull - 拉取资源

从 Jug0 后端拉取资源：

```bash
juglans pull <SLUG> [OPTIONS]
```

**选项：**

| 选项 | 说明 |
|------|------|
| `--type <TYPE>` | 资源类型 (prompt, agent, workflow) |
| `--output <DIR>` | 输出目录 |

**示例：**

```bash
# 拉取 Prompt
juglans pull my-prompt --type prompt

# 拉取到指定目录
juglans pull my-agent --type agent --output ./agents/
```

---

## 开发服务器

### web - 启动 Web 服务器

```bash
juglans web [OPTIONS]
```

**选项：**

| 选项 | 默认值 | 说明 |
|------|--------|------|
| `--host <HOST>` | 127.0.0.1 | 绑定地址 |
| `--port <PORT>` | 8080 | 端口号 |

**示例：**

```bash
# 默认配置
juglans web

# 自定义端口
juglans web --port 3000

# 允许外部访问
juglans web --host 0.0.0.0 --port 8080
```

**API 端点：**

| 端点 | 方法 | 说明 |
|------|------|------|
| `/api/agents` | GET | 列出 Agents |
| `/api/agents/:slug` | GET | 获取 Agent |
| `/api/prompts` | GET | 列出 Prompts |
| `/api/prompts/:slug` | GET | 获取 Prompt |
| `/api/prompts/:slug/render` | POST | 渲染 Prompt |
| `/api/workflows` | GET | 列出 Workflows |
| `/api/workflows/:slug/execute` | POST | 执行 Workflow |
| `/api/chat` | POST | Chat (SSE) |

---

## 配置

### 配置文件位置

按优先级查找：

1. `./juglans.toml` (当前目录)
2. `~/.config/juglans/juglans.toml` (用户配置)
3. `/etc/juglans/juglans.toml` (系统配置)

### 配置文件格式

```toml
# juglans.toml

[account]
id = "user_id"
api_key = "your_api_key"

[jug0]
base_url = "http://localhost:3000"

[server]
host = "127.0.0.1"
port = 8080

[mcp.filesystem]
command = "npx"
args = ["-y", "@anthropic/mcp-filesystem"]
env = { ROOT_DIR = "/workspace" }
```

### 环境变量

| 变量 | 说明 |
|------|------|
| `JUGLANS_API_KEY` | API 密钥 (覆盖配置文件) |
| `JUGLANS_CONFIG` | 配置文件路径 |
| `JUGLANS_LOG_LEVEL` | 日志级别 (debug, info, warn, error) |

---

## 输出格式

### 默认输出

```
[node_id] Status message...
[node_id] Result: ...
```

### 详细模式 (-v)

```
[DEBUG] Loading workflow: main.jgflow
[DEBUG] Parsed 5 nodes, 4 edges
[INFO] [init] Starting...
[DEBUG] [init] Output: null
[INFO] [chat] Calling agent: assistant
[DEBUG] [chat] Request: {"message": "..."}
[INFO] [chat] Response received (234 tokens)
...
```

### JSON 输出

```bash
juglans workflow.jgflow --output-format json
```

```json
{
  "success": true,
  "duration_ms": 1234,
  "nodes_executed": 5,
  "final_output": { ... }
}
```

---

## 退出码

| 码 | 说明 |
|----|------|
| 0 | 成功 |
| 1 | 一般错误 |
| 2 | 解析错误 |
| 3 | 执行错误 |
| 4 | 配置错误 |
| 5 | 网络错误 |

---

## 故障排除

### 常见问题

**Q: 找不到配置文件**
```bash
juglans --config /path/to/juglans.toml workflow.jgflow
```

**Q: API 连接失败**
```bash
# 检查连接
curl http://localhost:3000/health

# 查看详细日志
JUGLANS_LOG_LEVEL=debug juglans workflow.jgflow
```

**Q: MCP 工具不可用**
```bash
# 重新安装
juglans install --force
```

**Q: 内存不足**
```bash
# 对于大型工作流，增加栈大小
RUST_MIN_STACK=8388608 juglans workflow.jgflow
```