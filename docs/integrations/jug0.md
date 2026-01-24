# Jug0 后端集成

Jug0 是 Juglans 的后端 AI 平台，提供 LLM 调用、资源存储等服务。

## 概述

```
┌─────────────────┐         ┌─────────────────┐
│    Juglans      │  HTTP   │      Jug0       │
│    (本地)        │────────▶│    (后端)        │
│                 │         │                 │
│  - 解析 DSL     │         │  - LLM 调用     │
│  - 执行工作流   │         │  - 资源存储     │
│  - 本地资源     │         │  - 用户管理     │
└─────────────────┘         └─────────────────┘
```

## 配置连接

### juglans.toml

```toml
[account]
id = "your_user_id"
api_key = "jug0_sk_your_api_key"

[jug0]
base_url = "http://localhost:3000"  # 本地开发
# base_url = "https://api.jug0.com"  # 生产环境
timeout = 30
```

### 环境变量

```bash
export JUGLANS_API_KEY="jug0_sk_..."
export JUGLANS_JUG0_URL="http://localhost:3000"
```

## 认证

### API Key 认证

所有请求需要携带 API Key：

```
Authorization: Bearer jug0_sk_your_api_key
```

### 获取 API Key

1. 登录 Jug0 控制台
2. 进入 Settings > API Keys
3. 创建新的 API Key
4. 复制到 juglans.toml

## 资源管理

### 推送资源

将本地资源上传到 Jug0：

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

### 拉取资源

从 Jug0 下载资源：

```bash
# 拉取 Prompt
juglans pull my-prompt --type prompt

# 拉取到指定目录
juglans pull my-agent --type agent --output ./agents/

# 拉取所有
juglans pull --all --output ./resources/
```

### 列出资源

```bash
# 列出所有 Prompts
juglans list --type prompt

# 列出所有 Agents
juglans list --type agent

# 列出所有 Workflows
juglans list --type workflow
```

### 删除资源

```bash
juglans delete my-prompt --type prompt
```

## 资源引用

### GitHub 风格 Slug

Jug0 使用 `owner/slug` 格式标识资源：

```yaml
# 在工作流中引用远程资源
[chat]: chat(agent="juglans/assistant", message=$input.query)
[render]: p(slug="juglans/greeting", name=$input.name)
```

### 本地 vs 远程

```yaml
# 本地资源（通过文件导入）
prompts: ["./prompts/*.jgprompt"]
[render]: p(slug="my-local-prompt")

# 远程资源（从 Jug0 获取）
[render]: p(slug="owner/remote-prompt")
```

### 混合使用

```yaml
name: "Hybrid Workflow"

# 导入本地资源
prompts: ["./prompts/*.jgprompt"]
agents: ["./agents/*.jgagent"]

entry: [start]
exit: [end]

# 使用本地 Agent
[local_chat]: chat(agent="my-local-agent", message=$input.query)

# 使用远程 Agent
[remote_chat]: chat(agent="juglans/premium-agent", message=$output)

[start] -> [local_chat] -> [remote_chat] -> [end]
```

## Chat API

### 基本调用

工作流中的 `chat()` 工具会调用 Jug0 Chat API：

```yaml
[chat]: chat(
  agent="my-agent",
  message="Hello!",
  format="json"
)
```

### 流式响应

Jug0 使用 SSE (Server-Sent Events) 流式返回响应：

```
POST /api/chat
Content-Type: application/json

{
  "agent": "my-agent",
  "message": "Hello!",
  "stream": true
}
```

响应：

```
event: content
data: {"type": "content", "text": "Hello"}

event: content
data: {"type": "content", "text": "! How"}

event: content
data: {"type": "content", "text": " can I help?"}

event: done
data: {"type": "done", "tokens": 15}
```

### 非流式响应

```yaml
[chat]: chat(
  agent="my-agent",
  message="Hello!",
  stream="false"
)
```

## API 端点

### Prompts

| 端点 | 方法 | 说明 |
|------|------|------|
| `/api/prompts` | GET | 列出 Prompts |
| `/api/prompts/:slug` | GET | 获取 Prompt |
| `/api/prompts` | POST | 创建 Prompt |
| `/api/prompts/:slug` | PUT | 更新 Prompt |
| `/api/prompts/:slug` | DELETE | 删除 Prompt |
| `/api/prompts/:slug/render` | POST | 渲染 Prompt |

### Agents

| 端点 | 方法 | 说明 |
|------|------|------|
| `/api/agents` | GET | 列出 Agents |
| `/api/agents/:slug` | GET | 获取 Agent |
| `/api/agents` | POST | 创建 Agent |
| `/api/agents/:slug` | PUT | 更新 Agent |
| `/api/agents/:slug` | DELETE | 删除 Agent |

### Workflows

| 端点 | 方法 | 说明 |
|------|------|------|
| `/api/workflows` | GET | 列出 Workflows |
| `/api/workflows/:slug` | GET | 获取 Workflow |
| `/api/workflows` | POST | 创建 Workflow |
| `/api/workflows/:slug/execute` | POST | 执行 Workflow |

### Chat

| 端点 | 方法 | 说明 |
|------|------|------|
| `/api/chat` | POST | 发送消息 (SSE) |
| `/api/chat/:id/stop` | POST | 停止生成 |

### Resource (统一入口)

```
GET /api/r/:owner/:slug
```

自动识别资源类型（Prompt/Agent/Workflow）。

## 错误处理

### HTTP 状态码

| 状态码 | 说明 |
|--------|------|
| 200 | 成功 |
| 400 | 请求参数错误 |
| 401 | 未认证 |
| 403 | 无权限 |
| 404 | 资源不存在 |
| 429 | 请求过多（限流） |
| 500 | 服务器错误 |

### 错误响应格式

```json
{
  "error": {
    "code": "RESOURCE_NOT_FOUND",
    "message": "Prompt 'my-prompt' not found",
    "details": {}
  }
}
```

### 在工作流中处理

```yaml
[api_call]: chat(agent="external", message=$input)
[api_call] -> [success]
[api_call] on error -> [handle_error]

[handle_error]: notify(status="API call failed, using fallback")
[fallback]: chat(agent="local-fallback", message=$input)
[handle_error] -> [fallback]
```

## 本地开发

### 启动本地 Jug0

```bash
git clone https://github.com/juglans-ai/jug0.git
cd jug0
cargo run
```

### 配置连接

```toml
[jug0]
base_url = "http://localhost:3000"
```

### 开发模式

```bash
# 使用本地文件，不连接 Jug0
juglans workflows/test.jgflow --offline

# 详细日志
juglans workflows/test.jgflow --verbose
```

## 生产部署

### 推荐配置

```toml
[jug0]
base_url = "https://api.jug0.com"
timeout = 60

[logging]
level = "warn"
format = "json"
```

### 健康检查

```bash
curl https://api.jug0.com/health
```

### 监控

Jug0 提供 Prometheus 指标：

```
GET /metrics
```

## 最佳实践

1. **版本控制** - 将 juglans.toml（不含 API Key）提交到 Git
2. **环境分离** - 开发/测试/生产使用不同的 API Key
3. **错误处理** - 为网络调用添加 `on error` 路径
4. **超时设置** - 根据任务复杂度调整 timeout
5. **资源同步** - 定期 `juglans pull` 保持本地资源最新
