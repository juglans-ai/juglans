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

# MCP 服务器配置
[mcp.filesystem]
command = "npx"
args = ["-y", "@anthropic/mcp-filesystem"]
env = { ROOT_DIR = "/workspace" }

[mcp.web-browser]
url = "http://localhost:3001/mcp"
api_key = "mcp_key_..."
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

工作空间用于多用户协作，可选配置。

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `id` | string | 是 | 工作空间 ID |
| `name` | string | 是 | 工作空间名称 |
| `members` | array | 否 | 成员用户 ID 列表 |

```toml
[workspace]
id = "workspace_456"
name = "My Team Workspace"
members = ["user_123", "user_789", "user_456"]
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

### [mcp.*] - MCP 服务器

配置 Model Context Protocol 服务器以扩展工具能力。

#### 本地命令方式

```toml
[mcp.filesystem]
command = "npx"
args = ["-y", "@anthropic/mcp-filesystem"]
env = { ROOT_DIR = "/workspace" }
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `command` | string | 启动命令 |
| `args` | array | 命令参数 |
| `env` | object | 环境变量 |

#### 远程服务方式

```toml
[mcp.remote-tools]
url = "http://localhost:3001/mcp"
api_key = "mcp_key_..."
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `url` | string | MCP 服务 URL |
| `api_key` | string | 认证密钥 |

#### 多个 MCP 服务器

```toml
[mcp.filesystem]
command = "npx"
args = ["-y", "@anthropic/mcp-filesystem"]
env = { ROOT_DIR = "/data" }

[mcp.github]
command = "npx"
args = ["-y", "@anthropic/mcp-github"]
env = { GITHUB_TOKEN = "${GITHUB_TOKEN}" }

[mcp.database]
url = "http://localhost:5000/mcp"
api_key = "db_mcp_key"
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

[mcp.filesystem]
command = "npx"
args = ["-y", "@anthropic/mcp-filesystem"]
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