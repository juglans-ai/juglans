# MCP 工具集成

本指南介绍如何在 Juglans 中集成 Model Context Protocol (MCP) 工具服务器。

## 什么是 MCP

MCP (Model Context Protocol) 是一个开放协议，用于将外部工具能力暴露给 AI 系统。

```
┌─────────────────┐  HTTP   ┌─────────────────┐
│    Juglans      │◀───────▶│   MCP Server    │
│                 │ JSON-RPC│                 │
│  工作流执行器    │         │  - 文件系统      │
│                 │         │  - GitHub       │
│                 │         │  - 数据库        │
└─────────────────┘         └─────────────────┘
```

## 配置 MCP 服务器

### juglans.toml 配置

**重要：** Juglans 使用 HTTP/JSON-RPC 连接 MCP 服务器。你需要先启动 MCP 服务器（可以在 jug0 或独立服务中），然后配置 HTTP 连接。

#### HTTP 连接方式

```toml
# 文件系统工具
[[mcp_servers]]
name = "filesystem"
base_url = "http://localhost:3001/mcp/filesystem"
alias = "fs"

# GitHub 工具
[[mcp_servers]]
name = "github"
base_url = "http://localhost:3001/mcp/github"
token = "${GITHUB_TOKEN}"

# 自定义 MCP 服务器
[[mcp_servers]]
name = "my-tools"
base_url = "http://localhost:5000/mcp"
token = "optional_token"

# 云端 MCP 服务
[[mcp_servers]]
name = "cloud-service"
base_url = "https://mcp.example.com/v1"
token = "${MCP_API_KEY}"
```

**配置说明：**

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `name` | string | 是 | 服务器名称（用于生成工具名） |
| `base_url` | string | 是 | MCP 服务器 HTTP 地址 |
| `alias` | string | 否 | 别名 |
| `token` | string | 否 | 认证令牌（Bearer token） |

### 启动 MCP 服务器

Juglans 不会自动启动 MCP 服务器，你需要先手动启动或使用 jug0 集成的 MCP 服务：

**选项 1: 使用 jug0 的 MCP 集成**

jug0 可以托管 MCP 服务器，然后通过 HTTP 暴露：

```bash
# 在 jug0 中配置并启动 MCP 服务
cd jug0
cargo run -- --mcp-enabled
```

**选项 2: 独立启动 MCP 服务器**

使用 HTTP-to-MCP 桥接工具：

```bash
# 示例：启动文件系统 MCP 服务
npx @anthropic/mcp-filesystem --http --port 3001
```

**选项 3: 自定义 HTTP MCP 服务器**

实现一个 HTTP 服务器，遵循 MCP JSON-RPC 协议（见下文）

## 在工作流中使用 MCP 工具

### 工具命名规则

MCP 工具在工作流中以 `<namespace>.<tool_name>` 格式使用：

**namespace 来源：**
- 如果配置中有 `alias`，使用 alias
- 否则使用 `name`

**示例配置：**

```toml
[[mcp_servers]]
name = "filesystem"
base_url = "http://localhost:3001/mcp/filesystem"
alias = "fs"  # 可选别名
```

**工作流中调用：**

```yaml
# 使用 alias（如果配置了）
[read]: fs.read_file(path="/data/input.txt")

# 或使用 name（如果没有 alias）
[read]: filesystem.read_file(path="/data/input.txt")

# GitHub 工具示例
[issue]: github.create_issue(
  repo="owner/repo",
  title="Bug Report",
  body=$ctx.report
)
```

**命名格式：** `namespace.tool_name`（使用点号分隔，不是下划线）

### 完整示例

```yaml
name: "Code Review Workflow"

entry: [fetch_pr]
exit: [done]

# 从 GitHub 获取 PR
[fetch_pr]: github.get_pull_request(
  repo=$input.repo,
  number=$input.pr_number
)

# 保存 PR 信息
[save_pr]: set_context(pr=$output)

# 获取变更文件
[get_files]: github.list_pr_files(
  repo=$input.repo,
  number=$input.pr_number
)

# AI 审查代码
[review]: chat(
  agent="code-reviewer",
  message="Review these changes:\n" + json($output)
)

# 发表评论
[comment]: github.create_review_comment(
  repo=$input.repo,
  number=$input.pr_number,
  body=$output
)

[done]: notify(status="Review completed")

[fetch_pr] -> [save_pr] -> [get_files] -> [review] -> [comment] -> [done]
```

## 常用 MCP 服务器

### @anthropic/mcp-filesystem

文件系统操作（需要先启动 HTTP 服务）：

```toml
[[mcp_servers]]
name = "filesystem"
base_url = "http://localhost:3001/mcp/filesystem"
```

启动服务器（假设有 HTTP 桥接）：

```bash
# 需要 HTTP-to-stdio 桥接工具
npx @anthropic/mcp-filesystem --http --port 3001
```

可用工具：

| 工具 | 说明 |
|------|------|
| `read_file` | 读取文件内容 |
| `write_file` | 写入文件 |
| `list_directory` | 列出目录 |
| `create_directory` | 创建目录 |
| `delete_file` | 删除文件 |
| `move_file` | 移动/重命名文件 |
| `search_files` | 搜索文件 |

```yaml
# 读取文件
[read]: filesystem.read_file(path="data/config.json")

# 写入文件
[write]: filesystem.write_file(
  path="output/result.txt",
  content=$ctx.result
)

# 列出目录
[list]: filesystem.list_directory(path="src/")
```

### @anthropic/mcp-github

GitHub 操作（需要先启动 HTTP 服务）：

```toml
[[mcp_servers]]
name = "github"
base_url = "http://localhost:3001/mcp/github"
token = "${GITHUB_TOKEN}"
```

启动服务器（假设有 HTTP 桥接）：

```bash
export GITHUB_TOKEN="ghp_..."
npx @anthropic/mcp-github --http --port 3001
```

可用工具：

| 工具 | 说明 |
|------|------|
| `get_repo` | 获取仓库信息 |
| `list_issues` | 列出 Issues |
| `create_issue` | 创建 Issue |
| `get_pull_request` | 获取 PR |
| `create_pull_request` | 创建 PR |
| `list_pr_files` | 列出 PR 文件 |
| `search_code` | 搜索代码 |

```yaml
# 搜索代码
[search]: github.search_code(
  query="TODO in:file language:rust",
  repo=$input.repo
)

# 创建 Issue
[create]: github.create_issue(
  repo=$input.repo,
  title="Found TODOs",
  body="Found " + len($output.items) + " TODOs"
)
```

### @anthropic/mcp-postgres

PostgreSQL 数据库（需要先启动 HTTP 服务）：

```toml
[[mcp_servers]]
name = "postgres"
base_url = "http://localhost:3001/mcp/postgres"
```

启动服务器（假设有 HTTP 桥接）：

```bash
export DATABASE_URL="postgresql://..."
npx @anthropic/mcp-postgres --http --port 3001
```

可用工具：

| 工具 | 说明 |
|------|------|
| `query` | 执行 SQL 查询 |
| `execute` | 执行 SQL 命令 |
| `list_tables` | 列出表 |
| `describe_table` | 描述表结构 |

```yaml
# 查询数据
[query]: postgres.query(
  sql="SELECT * FROM users WHERE active = true LIMIT 10"
)

# 获取表结构
[schema]: postgres.describe_table(table="users")
```

## 自定义 MCP 服务器

### 创建 HTTP MCP 服务器

使用任何语言实现 HTTP + JSON-RPC 协议：

```python
# my_mcp_server.py
from flask import Flask, request, jsonify

app = Flask(__name__)

@app.route('/messages', methods=['POST'])
def handle_request():
    req = request.json
    method = req.get("method")

    if method == "tools/list":
        return jsonify({
            "jsonrpc": "2.0",
            "id": req.get("id"),
            "result": {
                "tools": [
                    {
                        "name": "my_tool",
                        "description": "My custom tool",
                        "inputSchema": {
                            "type": "object",
                            "properties": {
                                "input": {"type": "string"}
                            },
                            "required": ["input"]
                        }
                    }
                ]
            }
        })

    elif method == "tools/call":
        tool_name = req["params"]["name"]
        arguments = req["params"]["arguments"]

        if tool_name == "my_tool":
            result = process(arguments["input"])
            return jsonify({
                "jsonrpc": "2.0",
                "id": req.get("id"),
                "result": {
                    "content": [{"type": "text", "text": result}]
                }
            })

    return jsonify({"error": "Unknown method"}), 400

if __name__ == '__main__':
    app.run(port=5000)
```

### 配置自定义服务器

先启动服务器：

```bash
python ./tools/my_mcp_server.py
```

然后配置 Juglans：

```toml
[[mcp_servers]]
name = "my-tools"
base_url = "http://localhost:5000"
```

### 在工作流中使用

```yaml
[custom]: my-tools.my_tool(input=$ctx.data)
```

## 工具发现

### 列出可用工具

```bash
# 列出所有 MCP 工具
juglans tools --list

# 列出特定服务器的工具
juglans tools --list --server filesystem

# 显示工具详情
juglans tools --describe filesystem.read_file
```

### 工具发现过程

当工作流加载 Agent 时，Juglans 会：

1. 读取 `juglans.toml` 中的 `[[mcp_servers]]` 配置
2. 对每个服务器发送 `tools/list` JSON-RPC 请求
3. 获取工具定义并缓存到内存
4. 将工具注册为可调用的内置函数

## 错误处理

### MCP 工具错误

```yaml
[api_call]: github.get_repo(repo=$input.repo)
[api_call] -> [process]
[api_call] on error -> [handle_error]

[handle_error]: notify(status="GitHub API error, repo may not exist")
[fallback]: set_context(repo_info=null)

[handle_error] -> [fallback]
```

### 超时处理

在配置中设置超时：

```toml
[mcp.slow-service]
url = "http://slow-api.example.com/mcp"
timeout = 120  # 秒
```

## 最佳实践

### 1. 环境变量管理

不要在配置文件中硬编码密钥：

```toml
# 好
[mcp.github]
env = { GITHUB_TOKEN = "${GITHUB_TOKEN}" }

# 不好
[mcp.github]
env = { GITHUB_TOKEN = "ghp_xxxx..." }
```

### 2. 工具权限

在 MCP 服务器实现中限制访问范围，或使用代理层控制权限。

Juglans 仅通过 HTTP 连接，权限控制应在 MCP 服务器端实现。

### 3. 错误恢复

为 MCP 调用添加错误处理：

```yaml
[fetch]: github.get_repo(repo=$input.repo)
[fetch] -> [process]
[fetch] on error -> [retry_or_fallback]
```

### 4. 日志记录

启用调试日志排查问题：

```toml
[logging]
level = "debug"
```

### 5. 服务健康检查

确保 MCP 服务器在 Juglans 启动前已运行：

```bash
# 检查 MCP 服务器是否可达
curl http://localhost:3001/mcp/filesystem/messages -d '{"jsonrpc":"2.0","method":"tools/list","id":"1"}'
```

## 故障排查

### Q: 工具未找到

确保 MCP 服务器正在运行并可访问：

```bash
# 测试 MCP 服务器连接
curl http://localhost:3001/mcp/filesystem/messages \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"tools/list","id":"1"}'
```

### Q: MCP 服务器连接失败

检查服务器地址和状态：

```bash
# 检查服务器是否运行
curl http://localhost:3001/health

# 检查配置的 base_url 是否正确
```

### Q: 认证失败

验证环境变量：

```bash
echo $GITHUB_TOKEN
```

### Q: 超时错误

MCP 客户端默认超时 30 秒。检查网络或 MCP 服务器性能。

如需调整超时，需要修改 Juglans 源码中的 `src/services/mcp.rs`。
