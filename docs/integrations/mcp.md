# MCP 工具集成

本指南介绍如何在 Juglans 中集成 Model Context Protocol (MCP) 工具服务器。

## 什么是 MCP

MCP (Model Context Protocol) 是一个开放协议，用于将外部工具能力暴露给 AI 系统。

```
┌─────────────────┐         ┌─────────────────┐
│    Juglans      │  stdio  │   MCP Server    │
│                 │◀───────▶│                 │
│  工作流执行器    │  HTTP   │  - 文件系统      │
│                 │         │  - GitHub       │
│                 │         │  - 数据库        │
└─────────────────┘         └─────────────────┘
```

## 配置 MCP 服务器

### juglans.toml 配置

#### 本地命令方式

```toml
# 文件系统工具
[mcp.filesystem]
command = "npx"
args = ["-y", "@anthropic/mcp-filesystem"]
env = { ROOT_DIR = "/workspace" }

# GitHub 工具
[mcp.github]
command = "npx"
args = ["-y", "@anthropic/mcp-github"]
env = { GITHUB_TOKEN = "${GITHUB_TOKEN}" }

# 自定义 MCP 服务器
[mcp.my-tools]
command = "./tools/my-mcp-server"
args = ["--port", "0"]
env = { DEBUG = "true" }
```

#### 远程 HTTP 方式

```toml
[mcp.remote-tools]
url = "http://localhost:3001/mcp"
api_key = "mcp_key_..."

[mcp.cloud-service]
url = "https://mcp.example.com/v1"
api_key = "${MCP_API_KEY}"
```

### 获取工具 Schema

安装配置的 MCP 服务器并获取工具定义：

```bash
juglans install
```

这会：
1. 启动每个配置的 MCP 服务器
2. 获取工具列表和 schema
3. 缓存到 `.juglans/tools/` 目录

## 在工作流中使用 MCP 工具

### 工具命名

MCP 工具在工作流中以 `mcp_<server>_<tool>` 格式使用：

```yaml
# 使用 filesystem MCP 的 read_file 工具
[read]: mcp_filesystem_read_file(path="/data/input.txt")

# 使用 github MCP 的 create_issue 工具
[issue]: mcp_github_create_issue(
  repo="owner/repo",
  title="Bug Report",
  body=$ctx.report
)
```

### 完整示例

```yaml
name: "Code Review Workflow"

entry: [fetch_pr]
exit: [done]

# 从 GitHub 获取 PR
[fetch_pr]: mcp_github_get_pull_request(
  repo=$input.repo,
  number=$input.pr_number
)

# 保存 PR 信息
[save_pr]: set_context(pr=$output)

# 获取变更文件
[get_files]: mcp_github_list_pr_files(
  repo=$input.repo,
  number=$input.pr_number
)

# AI 审查代码
[review]: chat(
  agent="code-reviewer",
  message="Review these changes:\n" + json($output)
)

# 发表评论
[comment]: mcp_github_create_review_comment(
  repo=$input.repo,
  number=$input.pr_number,
  body=$output
)

[done]: notify(status="Review completed")

[fetch_pr] -> [save_pr] -> [get_files] -> [review] -> [comment] -> [done]
```

## 常用 MCP 服务器

### @anthropic/mcp-filesystem

文件系统操作：

```toml
[mcp.filesystem]
command = "npx"
args = ["-y", "@anthropic/mcp-filesystem"]
env = { ROOT_DIR = "/workspace" }
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
[read]: mcp_filesystem_read_file(path="data/config.json")

# 写入文件
[write]: mcp_filesystem_write_file(
  path="output/result.txt",
  content=$ctx.result
)

# 列出目录
[list]: mcp_filesystem_list_directory(path="src/")
```

### @anthropic/mcp-github

GitHub 操作：

```toml
[mcp.github]
command = "npx"
args = ["-y", "@anthropic/mcp-github"]
env = { GITHUB_TOKEN = "${GITHUB_TOKEN}" }
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
[search]: mcp_github_search_code(
  query="TODO in:file language:rust",
  repo=$input.repo
)

# 创建 Issue
[create]: mcp_github_create_issue(
  repo=$input.repo,
  title="Found TODOs",
  body="Found " + len($output.items) + " TODOs"
)
```

### @anthropic/mcp-postgres

PostgreSQL 数据库：

```toml
[mcp.postgres]
command = "npx"
args = ["-y", "@anthropic/mcp-postgres"]
env = {
  DATABASE_URL = "${DATABASE_URL}"
}
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
[query]: mcp_postgres_query(
  sql="SELECT * FROM users WHERE active = true LIMIT 10"
)

# 获取表结构
[schema]: mcp_postgres_describe_table(table="users")
```

## 自定义 MCP 服务器

### 创建 MCP 服务器

使用任何语言实现 MCP 协议：

```python
# my_mcp_server.py
import json
import sys

def handle_request(request):
    method = request.get("method")

    if method == "tools/list":
        return {
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

    elif method == "tools/call":
        tool_name = request["params"]["name"]
        arguments = request["params"]["arguments"]

        if tool_name == "my_tool":
            result = process(arguments["input"])
            return {"content": [{"type": "text", "text": result}]}

    return {"error": "Unknown method"}

# stdio 通信
for line in sys.stdin:
    request = json.loads(line)
    response = handle_request(request)
    print(json.dumps(response))
    sys.stdout.flush()
```

### 配置自定义服务器

```toml
[mcp.my-tools]
command = "python"
args = ["./tools/my_mcp_server.py"]
env = { CUSTOM_VAR = "value" }
```

### 在工作流中使用

```yaml
[custom]: mcp_my-tools_my_tool(input=$ctx.data)
```

## 工具发现

### 列出可用工具

```bash
# 列出所有 MCP 工具
juglans tools --list

# 列出特定服务器的工具
juglans tools --list --server filesystem

# 显示工具详情
juglans tools --describe mcp_filesystem_read_file
```

### 工具 Schema 缓存

工具定义缓存在 `.juglans/tools/` 目录：

```
.juglans/
└── tools/
    ├── filesystem.json
    ├── github.json
    └── my-tools.json
```

刷新缓存：

```bash
juglans install --force
```

## 错误处理

### MCP 工具错误

```yaml
[api_call]: mcp_github_get_repo(repo=$input.repo)
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

限制 MCP 服务器的访问范围：

```toml
[mcp.filesystem]
env = {
  ROOT_DIR = "/workspace/safe-dir",  # 限制目录
  READ_ONLY = "true"                  # 只读模式
}
```

### 3. 错误恢复

为 MCP 调用添加错误处理：

```yaml
[fetch]: mcp_github_get_repo(repo=$input.repo)
[fetch] -> [process]
[fetch] on error -> [retry_or_fallback]
```

### 4. 日志记录

启用调试日志排查问题：

```toml
[logging]
level = "debug"

[mcp.my-tools]
env = { DEBUG = "true" }
```

### 5. 版本固定

固定 MCP 服务器版本：

```toml
[mcp.filesystem]
command = "npx"
args = ["-y", "@anthropic/mcp-filesystem@1.2.3"]
```

## 故障排查

### Q: 工具未找到

确保已运行 `juglans install`：

```bash
juglans install
juglans tools --list
```

### Q: MCP 服务器启动失败

检查命令和环境：

```bash
# 手动测试
npx -y @anthropic/mcp-filesystem
```

### Q: 认证失败

验证环境变量：

```bash
echo $GITHUB_TOKEN
```

### Q: 超时错误

增加超时配置或检查网络：

```toml
[mcp.slow-api]
timeout = 120
```
