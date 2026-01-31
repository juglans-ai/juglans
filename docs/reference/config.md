# 配置文件参考

Juglans 使用 `juglans.toml` 文件进行配置。

## 文件位置

按优先级查找：

1. `./juglans.toml` - 当前目录（项目配置）
2. `~/.config/juglans/juglans.toml` - 用户配置
3. `/etc/juglans/juglans.toml` - 系统配置

也可以通过环境变量指定：

```bash
JUGLANS_CONFIG=/path/to/juglans.toml juglans ...
```

## 完整配置示例

```toml
# juglans.toml

# 账户配置
[account]
id = "user_123"
name = "John Doe"
role = "admin"
api_key = "jug0_sk_..."

# 工作空间配置（可选）
[workspace]
id = "workspace_456"
name = "My Workspace"
members = ["user_123", "user_789"]

# 资源路径（支持 glob 模式）
agents = ["ops/agents/**/*.jgagent"]
workflows = ["ops/workflows/**/*.jgflow"]
prompts = ["ops/prompts/**/*.jgprompt"]
tools = ["ops/tools/**/*.json"]

# 排除规则
exclude = ["**/*.backup", "**/.draft", "**/test_*"]

# Jug0 后端配置
[jug0]
base_url = "http://localhost:3000"

# Web 服务器配置
[server]
host = "127.0.0.1"
port = 8080

# 环境变量（可选）
[env]
DATABASE_URL = "postgresql://localhost/mydb"
CUSTOM_VAR = "value"

# MCP 服务器配置（HTTP 连接方式）
[[mcp_servers]]
name = "filesystem"
base_url = "http://localhost:3001/mcp/filesystem"
alias = "fs"
token = "optional_token"

[[mcp_servers]]
name = "github"
base_url = "http://localhost:3001/mcp/github"
token = "${GITHUB_TOKEN}"
```

## 配置节详解

### [account] - 账户配置

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `id` | string | 是 | 用户 ID |
| `name` | string | 是 | 用户名称 |
| `role` | string | 否 | 用户角色（如 admin, user） |
| `api_key` | string | 否 | API 密钥 |

```toml
[account]
id = "user_123"
name = "John Doe"
role = "admin"
api_key = "jug0_sk_abcdef123456"
```

**环境变量覆盖：**

```bash
export JUGLANS_API_KEY="jug0_sk_..."
```

---

### [workspace] - 工作空间配置

工作空间用于多用户协作和资源批量管理。

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `id` | string | 是 | 工作空间 ID |
| `name` | string | 是 | 工作空间名称 |
| `members` | array | 否 | 成员用户 ID 列表 |
| `agents` | array | 否 | Agent 文件路径模式 |
| `workflows` | array | 否 | Workflow 文件路径模式 |
| `prompts` | array | 否 | Prompt 文件路径模式 |
| `tools` | array | 否 | Tool 定义文件路径模式 |
| `exclude` | array | 否 | 排除文件路径模式 |

```toml
[workspace]
id = "workspace_456"
name = "My Team Workspace"
members = ["user_123", "user_789", "user_456"]

# 资源路径配置（支持 glob 模式）
agents = ["ops/agents/**/*.jgagent"]
workflows = ["ops/workflows/**/*.jgflow"]
prompts = ["ops/prompts/**/*.jgprompt"]
tools = ["ops/tools/**/*.json"]

# 排除规则
exclude = [
  "**/*.backup",
  "**/.draft",
  "**/test_*",
  "**/private_*"
]
```

#### 资源路径配置

资源路径支持 **glob 模式**，用于批量操作时自动发现文件。

**常用模式：**

- `**/*.jgflow` - 递归匹配所有 .jgflow 文件
- `workflows/*.jgflow` - 只匹配 workflows 目录下的文件（不递归）
- `{ops,dev}/**/*.jgagent` - 匹配 ops 和 dev 目录下的所有 agent

**使用场景：**

```bash
# 使用 workspace 配置批量 apply
juglans apply                    # apply 所有配置的资源
juglans apply --type workflow    # 只 apply workflows
juglans apply --dry-run          # 预览
```

**排除规则：**

使用 `exclude` 字段忽略特定文件：

```toml
[workspace]
exclude = [
  "**/*.backup",        # 所有备份文件
  "**/.draft",          # 草稿文件
  "**/test_*",          # 测试文件
  "**/private_*",       # 私有文件
  "ops/experimental/**" # 实验性目录
]
```

---

### [jug0] - 后端配置

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `base_url` | string | https://api.jug0.com | API 地址 |

```toml
[jug0]
base_url = "https://api.jug0.com"
```

**不同环境配置：**

```toml
# 开发环境
[jug0]
base_url = "http://localhost:3000"

# 生产环境
# [jug0]
# base_url = "https://api.jug0.com"
```

---

### [server] - Web 服务器

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `host` | string | 127.0.0.1 | 绑定地址 |
| `port` | number | 3000 | 端口号 |

```toml
[server]
host = "0.0.0.0"
port = 8080
```

---

### [env] - 环境变量

自定义环境变量字典，可在工作流中访问。

```toml
[env]
DATABASE_URL = "postgresql://localhost/mydb"
API_ENDPOINT = "https://api.example.com"
CUSTOM_SETTING = "value"
```

这些环境变量可以在工作流执行时通过 `$env.DATABASE_URL` 等方式访问。

**使用场景：**
- 数据库连接字符串
- API 端点配置
- 自定义配置项
- 开发/生产环境切换

---

### [logging] - 日志配置

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `level` | string | info | 日志级别 |
| `format` | string | pretty | 输出格式 |

**日志级别：**

- `error` - 仅错误
- `warn` - 警告和错误
- `info` - 信息、警告、错误
- `debug` - 调试信息
- `trace` - 详细跟踪

**输出格式：**

- `pretty` - 彩色可读格式
- `json` - JSON 格式（适合日志收集）
- `compact` - 紧凑单行格式

```toml
[logging]
level = "debug"
format = "json"
```

**环境变量覆盖：**

```bash
export JUGLANS_LOG_LEVEL=debug
```

---

### [[mcp_servers]] - MCP 服务器

配置 Model Context Protocol 服务器以扩展工具能力。

**重要：** Juglans 使用 HTTP/JSON-RPC 连接 MCP 服务器，不支持进程启动方式。你需要先启动 MCP 服务器，然后通过 HTTP 连接。

#### 配置格式

```toml
[[mcp_servers]]
name = "filesystem"
base_url = "http://localhost:3001/mcp/filesystem"
alias = "fs"
token = "optional_token"
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `name` | string | 是 | 服务器名称（用于工具命名） |
| `base_url` | string | 是 | MCP 服务器 HTTP 地址 |
| `alias` | string | 否 | 别名 |
| `token` | string | 否 | 认证令牌 |

#### 多个 MCP 服务器

```toml
[[mcp_servers]]
name = "filesystem"
base_url = "http://localhost:3001/mcp/filesystem"
alias = "fs"

[[mcp_servers]]
name = "github"
base_url = "http://localhost:3001/mcp/github"
token = "${GITHUB_TOKEN}"

[[mcp_servers]]
name = "database"
base_url = "http://localhost:5000/mcp"
token = "db_mcp_key"
```

---

## 环境变量

| 变量 | 说明 |
|------|------|
| `JUGLANS_API_KEY` | API 密钥 |
| `JUGLANS_CONFIG` | 配置文件路径 |
| `JUGLANS_LOG_LEVEL` | 日志级别 |
| `JUGLANS_JUG0_URL` | Jug0 API 地址 |

**在配置中引用环境变量：**

```toml
[mcp.github]
env = { GITHUB_TOKEN = "${GITHUB_TOKEN}" }
```

---

## 项目配置 vs 用户配置

### 项目配置 (./juglans.toml)

项目特定设置，应提交到版本控制（不含敏感信息）：

```toml
# 项目配置示例
[jug0]
base_url = "http://localhost:3000"

[server]
port = 8080

[[mcp_servers]]
name = "filesystem"
base_url = "http://localhost:3001/mcp/filesystem"
```

### 用户配置 (~/.config/juglans/juglans.toml)

个人设置和敏感信息：

```toml
# 用户配置示例
[account]
id = "my_user_id"
api_key = "jug0_sk_my_secret_key"

[logging]
level = "debug"
```

---

## 配置验证

检查配置是否有效：

```bash
juglans config --check
```

查看生效的配置：

```bash
juglans config --show
```

---

## 最佳实践

### 1. 分离敏感信息

```toml
# juglans.toml (提交到 git)
[jug0]
base_url = "http://localhost:3000"

# 敏感信息用环境变量
# export JUGLANS_API_KEY="..."
```

### 2. 使用 .env 文件

创建 `.env` 文件（加入 .gitignore）：

```bash
JUGLANS_API_KEY=jug0_sk_...
GITHUB_TOKEN=ghp_...
```

### 3. 环境特定配置

```toml
# juglans.dev.toml
[jug0]
base_url = "http://localhost:3000"

# juglans.prod.toml
[jug0]
base_url = "https://api.jug0.com"
```

使用：

```bash
JUGLANS_CONFIG=juglans.prod.toml juglans ...
```