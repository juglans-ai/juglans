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
juglans path/to/workflow.jg [OPTIONS]
```

**选项：**

| 选项 | 说明 |
|------|------|
| `--input <JSON>` | 输入数据 (JSON 格式) |
| `--input-file <FILE>` | 从文件读取输入 |
| `--verbose`, `-v` | 详细输出 |
| `--dry-run` | 仅解析，不执行 |
| `--output <FILE>` | 输出结果到文件 |
| `--output-format <FORMAT>` | 输出格式 (text, json)，默认 text |

**示例：**

```bash
# 基本执行
juglans src/main.jg

# 传入输入
juglans src/main.jg --input '{"query": "Hello"}'

# 从文件读取输入
juglans src/main.jg --input-file input.json

# 详细模式
juglans src/main.jg -v

# 仅验证
juglans src/main.jg --dry-run

# JSON 格式输出（便于程序化处理）
juglans src/main.jg --output-format json
```

**JSON 输出格式：**

当使用 `--output-format json` 时，输出结构化的执行结果：

```json
{
  "success": true,
  "duration_ms": 1234,
  "nodes_executed": 5,
  "final_output": {
    "status": "completed",
    "result": "..."
  }
}
```

这对于 CI/CD 集成或程序化处理工作流结果非常有用

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
juglans src/agents/assistant.jgagent

# 发送单条消息
juglans src/agents/assistant.jgagent --message "What is Rust?"

# 查看配置
juglans src/agents/assistant.jgagent --info
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
juglans src/prompts/greeting.jgprompt

# 传入变量
juglans src/prompts/greeting.jgprompt --input '{"name": "Alice"}'

# 输出到文件
juglans src/prompts/greeting.jgprompt --output rendered.txt
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
└── src/
    ├── example.jg
    ├── workflows/
    │   └── example.jgflow
    ├── agents/
    │   └── example.jgagent
    ├── pure-agents/
    ├── prompts/
    │   └── example.jgprompt
    └── tools/
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

将本地资源推送到 Jug0 后端，支持单文件或批量操作。

```bash
juglans apply [PATHS...] [OPTIONS]
```

**参数：**

| 参数 | 说明 |
|------|------|
| `PATHS` | 文件或目录路径（可选，为空时使用 workspace 配置） |

**选项：**

| 选项 | 说明 |
|------|------|
| `--force` | 覆盖已存在的资源 |
| `--dry-run` | 预览但不执行 |
| `--type <TYPE>`, `-t` | 过滤资源类型 (workflow, agent, prompt, tool, all) |
| `--recursive`, `-r` | 递归扫描目录 |

#### 基本用法

```bash
# 推送单个文件
juglans apply src/prompts/my-prompt.jgprompt
juglans apply src/agents/my-agent.jgagent
juglans apply src/workflows/my-flow.jgflow

# 强制覆盖
juglans apply src/prompts/my-prompt.jgprompt --force
```

#### 批量操作

**使用 workspace 配置：**

首先在 `juglans.toml` 中配置资源路径：

```toml
[workspace]
agents = ["src/agents/**/*.jgagent", "src/pure-agents/**/*.jgagent"]
workflows = ["src/**/*.jg", "src/workflows/**/*.jgflow"]
prompts = ["src/prompts/**/*.jgprompt"]
tools = ["src/tools/**/*.json"]
exclude = ["**/*.backup", "**/test_*"]
```

然后无参数运行 apply：

```bash
# Apply 所有配置的资源
juglans apply

# 预览将要 apply 的文件
juglans apply --dry-run

# 只 apply workflows
juglans apply --type workflow

# 只 apply agents
juglans apply -t agent
```

**输出示例：**

```
📦 Using workspace configuration from juglans.toml

📂 Found resources:
  📄 3 workflow(s)
  👤 5 agent(s)
  📝 8 prompt(s)

📤 Applying resources...

  ✅ workflow: trading-assistant.jg - Applied
  ✅ agent: trader.jgagent - Applied
  ⚠️  agent: assistant.jgagent - Skipped (exists, use --force)
  ✅ prompt: greeting.jgprompt - Applied

📊 Summary:
  ✅ 9 succeeded
  ⚠️  1 skipped
  ❌ 0 failed
```

**Apply 指定目录：**

```bash
# Apply 整个目录
juglans apply src/workflows/

# 递归 apply 所有子目录
juglans apply src/ -r

# Apply 多个目录
juglans apply src/agents/ src/prompts/

# Apply 特定类型
juglans apply src/ -r --type workflow
```

**Glob 模式：**

```bash
# Apply 所有 workflow
juglans apply "src/**/*.jg"

# Apply 特定前缀的文件
juglans apply "src/agents/prod_*.jgagent"
```

**Dry-run 模式：**

```bash
# 预览将要 apply 的文件
juglans apply --dry-run

# 预览特定目录
juglans apply src/workflows/ --dry-run
```

输出：

```
📦 Scanning workspace: src/

📂 Found resources:
  📄 3 workflow(s)
  👤 5 agent(s)

🔍 Dry run mode - preview only:

  ✓ src/trading.jg
  ✓ src/analysis.jg
  ✓ src/pipeline.jg
  ✓ src/agents/trader.jgagent
  ✓ src/agents/assistant.jgagent

📊 Total: 8 file(s)

Run without --dry-run to apply.
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
juglans pull my-agent --type agent --output ./src/agents/
```

---

### list - 列出远程资源

列出 Jug0 后端的资源。

```bash
juglans list [OPTIONS]
```

**选项：**

| 选项 | 说明 |
|------|------|
| `--type <TYPE>`, `-t` | 过滤资源类型 (prompt, agent, workflow)，可选 |

**示例：**

```bash
# 列出所有资源
juglans list

# 只列出 Prompts
juglans list --type prompt

# 只列出 Agents（短选项）
juglans list -t agent

# 只列出 Workflows
juglans list --type workflow
```

**输出格式：**

```
greeting-prompt (prompt)
assistant (agent)
market-analyst (agent)
simple-chat (workflow)
data-pipeline (workflow)
```

输出格式为：`slug (resource_type)`，每行一个资源。

**空结果：**

如果没有找到资源，会显示：
```
No resources found.
```

**使用场景：**

- 查看服务器上已有的资源
- 确认资源是否已成功 apply
- 在 pull 之前确认资源存在

**注意事项：**

- 需要配置有效的 API key
- 只显示当前账户可访问的资源
- 按资源类型和名称排序

---

### check - 验证文件语法

验证 `.jg`、`.jgagent`、`.jgprompt` 文件的语法正确性（类似 `cargo check`）。

```bash
juglans check [PATH] [OPTIONS]
```

**参数：**

| 参数 | 说明 |
|------|------|
| `PATH` | 要检查的文件或目录路径（可选，默认为当前目录） |

**选项：**

| 选项 | 说明 |
|------|------|
| `--all` | 显示所有问题包括警告 |
| `--format <FORMAT>` | 输出格式 (text, json)，默认 text |

**示例：**

```bash
# 检查当前目录所有文件
juglans check

# 检查特定目录
juglans check ./src/

# 检查单个文件
juglans check workflow.jg

# 显示所有警告
juglans check --all

# JSON 格式输出
juglans check --format json
```

**输出示例（text 格式）：**

```
    Checking juglans files in "."

    error[workflow]: src/main.jg (1 error(s), 0 warning(s))
      --> [E001] Entry node 'start' not defined

    warning[workflow]: src/test.jg (1 warning(s))
      --> [W001] Unused node 'debug'

    Finished checking 3 workflow(s), 2 agent(s), 1 prompt(s) - 2 valid with warnings

error: could not validate 1 file(s) due to 1 previous error(s)
```

**输出示例（JSON 格式）：**

```json
{
  "total": 6,
  "valid": 5,
  "errors": 1,
  "warnings": 1,
  "by_type": {
    "workflows": 3,
    "agents": 2,
    "prompts": 1
  },
  "results": [
    {
      "file": "src/main.jg",
      "type": "workflow",
      "slug": "main",
      "valid": false,
      "errors": [
        {"code": "E001", "message": "Entry node 'start' not defined"}
      ],
      "warnings": []
    }
  ]
}
```

**退出码：**

- `0` - 所有文件验证通过
- `1` - 存在语法错误

**使用场景：**

- CI/CD 流水线中的语法验证
- 提交前的本地检查
- 批量验证项目中所有工作流文件

---

### delete - 删除远程资源

从 Jug0 后端删除资源。

```bash
juglans delete <SLUG> --type <TYPE>
```

**参数：**

| 参数 | 说明 |
|------|------|
| `SLUG` | 要删除的资源 slug |

**选项：**

| 选项 | 说明 |
|------|------|
| `--type <TYPE>`, `-t` | 资源类型 (prompt, agent, workflow) |

**示例：**

```bash
# 删除 Prompt
juglans delete my-prompt --type prompt

# 删除 Agent（短选项）
juglans delete my-agent -t agent

# 删除 Workflow
juglans delete chat-flow --type workflow
```

**注意事项：**

- 需要配置有效的 API key（通过 `juglans.toml` 或环境变量）
- 删除操作不可逆，请谨慎使用
- 只能删除当前账户拥有的资源
- 删除成功后会显示确认消息：`✅ Deleted <slug> (<type>)`

**错误处理：**

- 如果资源不存在，会返回错误
- 如果没有权限删除，会返回认证错误
- 网络错误会显示相应的错误信息

---

### whoami - 显示账号信息

显示当前用户和工作空间配置信息。

```bash
juglans whoami [OPTIONS]
```

**选项：**

| 选项 | 说明 |
|------|------|
| `--verbose`, `-v` | 显示详细信息 |
| `--check-connection` | 测试与 Jug0 服务器的连接 |

**基本用法：**

```bash
# 显示账号信息
juglans whoami

# 显示详细信息
juglans whoami --verbose

# 测试连接
juglans whoami --check-connection

# 详细模式 + 连接测试
juglans whoami -v --check-connection
```

**输出示例（基本）：**

```
📋 Account Information

User ID:       u_demo
Name:          Demo User
Role:          admin
API Key:       jug0_sk_***...***def (configured)

Workspace:     ws_default (My Workspace)
Members:       2 user(s)

Jug0 Server:   http://localhost:3000

Config:        ./juglans.toml
```

**输出示例（详细模式）：**

```
📋 Account Information

User ID:       u_demo
Name:          Demo User
Role:          admin
API Key:       jug0_sk_***...***def (configured)

Workspace:     ws_default (My Workspace)
Members:       2 user(s)

Resource Paths:
  Agents:      src/agents/**/*.jgagent, src/pure-agents/**/*.jgagent
  Workflows:   src/**/*.jg, src/workflows/**/*.jgflow
  Prompts:     src/prompts/**/*.jgprompt
  Tools:       src/tools/**/*.json

Exclude:       **/*.backup, **/.draft, **/test_*

Jug0 Server:   http://localhost:3000
Status:        ✅ Connected

Web Server:    127.0.0.1:3000

MCP Servers:   2 configured
  - filesystem (alias: fs): http://localhost:3001/mcp/filesystem
  - github: http://localhost:3001/mcp/github

Config:        ./juglans.toml
```

**状态指示：**

- `✅ Connected` - 服务器连接正常
- `⚠️ Server unreachable` - 无法连接服务器
- `❌ Error: ...` - 连接错误
- `⚠️ Not configured` - API Key 未配置

**使用场景：**

- 确认当前使用的账号
- 检查配置是否正确
- 调试连接问题
- 查看工作空间设置
- 验证 API Key 是否已配置

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
[DEBUG] Loading workflow: main.jg
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
juglans workflow.jg --output-format json
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
juglans --config /path/to/juglans.toml workflow.jg
```

**Q: API 连接失败**
```bash
# 检查连接
curl http://localhost:3000/health

# 查看详细日志
JUGLANS_LOG_LEVEL=debug juglans workflow.jg
```

**Q: MCP 工具不可用**
```bash
# 重新安装
juglans install --force
```

**Q: 内存不足**
```bash
# 对于大型工作流，增加栈大小
RUST_MIN_STACK=8388608 juglans workflow.jg
```