# 内置 Web 服务器

Juglans 内置 Web 服务器，可将工作流作为 HTTP API 暴露。

## 启动服务器

### 基本启动

```bash
juglans serve
```

默认监听 `http://127.0.0.1:8080`。

### 配置选项

```bash
# 指定端口
juglans serve --port 3030

# 绑定所有网卡
juglans serve --host 0.0.0.0

# 指定工作流目录
juglans serve --dir ./workflows

# 启用热重载
juglans serve --watch

# 组合选项
juglans serve --host 0.0.0.0 --port 8080 --dir ./workflows --watch
```

## 配置文件

### juglans.toml

```toml
[server]
host = "127.0.0.1"
port = 8080
cors_origins = ["http://localhost:5173", "https://app.example.com"]

[server.auth]
enabled = true
api_keys = ["key1", "key2"]
```

## API 端点

### 工作流执行

#### POST /api/workflows/:name/execute

执行指定工作流。

**请求：**

```bash
curl -X POST http://localhost:8080/api/workflows/my-flow/execute \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer your_api_key" \
  -d '{"query": "Hello, world!"}'
```

**响应（非流式）：**

```json
{
  "success": true,
  "result": {
    "response": "Hello! How can I help you today?"
  },
  "execution_time_ms": 1234
}
```

**响应（流式）：**

使用 `Accept: text/event-stream` 获取 SSE 流：

```bash
curl -X POST http://localhost:8080/api/workflows/my-flow/execute \
  -H "Content-Type: application/json" \
  -H "Accept: text/event-stream" \
  -d '{"query": "Hello"}'
```

```
event: node_start
data: {"node": "classify", "timestamp": "2024-01-15T10:30:00Z"}

event: content
data: {"text": "Hello! "}

event: content
data: {"text": "How can I help?"}

event: node_complete
data: {"node": "classify", "duration_ms": 500}

event: done
data: {"success": true, "total_duration_ms": 1234}
```

### Prompt 渲染

#### POST /api/prompts/:slug/render

渲染 Prompt 模板。

**请求：**

```bash
curl -X POST http://localhost:8080/api/prompts/greeting/render \
  -H "Content-Type: application/json" \
  -d '{"name": "Alice", "style": "formal"}'
```

**响应：**

```json
{
  "success": true,
  "result": "Dear Alice,\n\nIt is my pleasure to assist you today..."
}
```

### 资源列表

#### GET /api/workflows

列出所有可用工作流。

```bash
curl http://localhost:8080/api/workflows
```

```json
{
  "workflows": [
    {
      "name": "chat-flow",
      "description": "Basic chat workflow",
      "entry": ["start"],
      "exit": ["end"]
    },
    {
      "name": "analysis-flow",
      "description": "Data analysis workflow",
      "entry": ["input"],
      "exit": ["output"]
    }
  ]
}
```

#### GET /api/prompts

列出所有 Prompt。

```bash
curl http://localhost:8080/api/prompts
```

#### GET /api/agents

列出所有 Agent。

```bash
curl http://localhost:8080/api/agents
```

### 健康检查

#### GET /health

```bash
curl http://localhost:8080/health
```

```json
{
  "status": "healthy",
  "version": "0.1.0",
  "uptime_seconds": 3600
}
```

## 认证

### API Key 认证

启用认证：

```toml
[server.auth]
enabled = true
api_keys = ["key1", "key2"]
```

请求时携带：

```bash
curl -H "Authorization: Bearer key1" http://localhost:8080/api/workflows
```

### 环境变量

```bash
export JUGLANS_SERVER_API_KEYS="key1,key2"
```

## CORS 配置

### 允许特定域

```toml
[server]
cors_origins = [
  "http://localhost:5173",
  "https://app.example.com"
]
```

### 允许所有域（开发用）

```toml
[server]
cors_origins = ["*"]
```

## 流式响应

### SSE 事件类型

| 事件 | 说明 |
|------|------|
| `node_start` | 节点开始执行 |
| `node_complete` | 节点执行完成 |
| `content` | 内容输出（LLM 生成的文本） |
| `error` | 错误发生 |
| `done` | 执行完成 |

### 客户端处理

```javascript
const eventSource = new EventSource('/api/workflows/chat/execute');

eventSource.addEventListener('content', (event) => {
  const data = JSON.parse(event.data);
  console.log('Content:', data.text);
});

eventSource.addEventListener('done', (event) => {
  const data = JSON.parse(event.data);
  console.log('Complete:', data);
  eventSource.close();
});

eventSource.addEventListener('error', (event) => {
  console.error('Error:', event);
  eventSource.close();
});
```

### fetch + SSE

```javascript
async function executeWorkflow(name, input) {
  const response = await fetch(`/api/workflows/${name}/execute`, {
    method: 'POST',
    headers: {
      'Content-Type': 'application/json',
      'Accept': 'text/event-stream',
    },
    body: JSON.stringify(input),
  });

  const reader = response.body.getReader();
  const decoder = new TextDecoder();

  while (true) {
    const { done, value } = await reader.read();
    if (done) break;

    const text = decoder.decode(value);
    const lines = text.split('\n');

    for (const line of lines) {
      if (line.startsWith('data: ')) {
        const data = JSON.parse(line.slice(6));
        handleEvent(data);
      }
    }
  }
}
```

## 目录结构

服务器加载的资源目录结构：

```
./workflows/           # --dir 指定的目录
├── chat.jgflow        # 工作流文件
├── analysis.jgflow
├── prompts/           # Prompt 目录
│   ├── greeting.jgprompt
│   └── summary.jgprompt
└── agents/            # Agent 目录
    ├── assistant.jgagent
    └── analyst.jgagent
```

## 热重载

启用 `--watch` 后，文件修改会自动重新加载：

```bash
juglans serve --watch
```

支持监听：
- `.jgflow` 文件变化
- `.jgprompt` 文件变化
- `.jgagent` 文件变化
- `juglans.toml` 配置变化

## 生产部署

### 使用 systemd

```ini
# /etc/systemd/system/juglans.service
[Unit]
Description=Juglans Workflow Server
After=network.target

[Service]
Type=simple
User=juglans
WorkingDirectory=/opt/juglans
ExecStart=/usr/local/bin/juglans serve --host 0.0.0.0 --port 8080 --dir /opt/juglans/workflows
Restart=always
RestartSec=5
Environment=JUGLANS_API_KEY=your_key

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl enable juglans
sudo systemctl start juglans
```

### Docker 部署

```dockerfile
FROM rust:1.75 as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/juglans /usr/local/bin/
COPY workflows/ /app/workflows/
WORKDIR /app
EXPOSE 8080
CMD ["juglans", "serve", "--host", "0.0.0.0", "--port", "8080", "--dir", "/app/workflows"]
```

```bash
docker build -t juglans-server .
docker run -p 8080:8080 -e JUGLANS_API_KEY=key juglans-server
```

### Nginx 反向代理

```nginx
upstream juglans {
    server 127.0.0.1:8080;
}

server {
    listen 443 ssl http2;
    server_name api.example.com;

    ssl_certificate /path/to/cert.pem;
    ssl_certificate_key /path/to/key.pem;

    location / {
        proxy_pass http://juglans;
        proxy_http_version 1.1;
        proxy_set_header Upgrade $http_upgrade;
        proxy_set_header Connection "upgrade";
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_buffering off;  # 重要：SSE 需要禁用缓冲
    }
}
```

## 监控

### Prometheus 指标

```
GET /metrics
```

返回 Prometheus 格式指标：

```
# HELP juglans_workflow_executions_total Total workflow executions
# TYPE juglans_workflow_executions_total counter
juglans_workflow_executions_total{workflow="chat",status="success"} 1234
juglans_workflow_executions_total{workflow="chat",status="error"} 12

# HELP juglans_workflow_duration_seconds Workflow execution duration
# TYPE juglans_workflow_duration_seconds histogram
juglans_workflow_duration_seconds_bucket{workflow="chat",le="0.1"} 100
juglans_workflow_duration_seconds_bucket{workflow="chat",le="1"} 900
juglans_workflow_duration_seconds_bucket{workflow="chat",le="10"} 1200
```

### 日志

```toml
[logging]
level = "info"
format = "json"  # JSON 格式便于日志收集
```

## 最佳实践

1. **认证必选** - 生产环境始终启用认证
2. **CORS 限制** - 只允许必要的域
3. **超时设置** - 为长时间工作流设置合理超时
4. **负载均衡** - 高流量时使用多实例 + 负载均衡
5. **日志收集** - 使用 JSON 格式便于 ELK/Loki 收集
6. **健康检查** - 配置 /health 端点用于监控