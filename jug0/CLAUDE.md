# CLAUDE.md — jug0 (AI Core Backend)

## Tech Stack

Rust, Axum 0.7, SeaORM (PostgreSQL/MySQL/SQLite), async-openai, qdrant-client, DashMap, tokio

## Build & Run

```bash
cargo build            # 编译
cargo run              # 启动 (默认 :3000)
RUST_LOG=jug0=debug cargo run   # 带 debug 日志
cargo run -p migration -- up    # 执行数据库迁移
cargo run -p migration -- status  # 查看迁移状态
```

## Project Structure

```
src/
├── main.rs                    # AppState 定义 + 路由注册 + 启动
├── auth.rs                    # JWT/API Key 验证中间件
├── errors.rs                  # AppError 统一错误类型
├── entities/                  # SeaORM 实体
│   ├── chats.rs
│   ├── messages.rs            # message_types / states / roles 常量
│   ├── agents.rs
│   ├── prompts.rs
│   ├── workflows.rs
│   └── handles.rs
├── handlers/
│   ├── chat/
│   │   ├── mod.rs             # chat_handler, tool_result_handler, stop_chat_handler
│   │   ├── logic.rs           # run_chat_stream() — 核心流式生成逻辑
│   │   ├── types.rs           # 请求/响应类型, InternalStreamEvent, ToolResultPayload
│   │   └── helpers.rs         # merge_tools, try_repair_json
│   ├── prompts.rs
│   ├── agents.rs
│   ├── workflows.rs
│   ├── messages.rs
│   ├── memories.rs
│   └── ...
├── providers/
│   ├── factory.rs             # ProviderFactory (多 LLM 后端)
│   ├── openai.rs / deepseek.rs / qwen.rs / gemini.rs
│   └── embedding.rs
├── services/
│   ├── mcp.rs                 # MCP server-side tool 执行
│   ├── memory/                # 向量记忆 (Qdrant)
│   ├── qdrant.rs              # Qdrant 客户端
│   ├── cache.rs               # Redis 缓存
│   └── models.rs              # 模型同步
└── repositories/              # Agent/Prompt 缓存仓库
```

## AppState 关键字段

```rust
pub struct AppState {
    pub db: DatabaseConnection,
    pub providers: ProviderFactory,
    pub active_requests: DashMap<Uuid, CancellationToken>,   // 活跃请求 (用于 stop)
    pub tool_result_channels: DashMap<Uuid, mpsc::Sender<Vec<ToolResultPayload>>>,  // SSE ↔ tool-result 通信
    pub mcp_client: McpClient,
    pub signing_key: Vec<u8>,          // Execution Token 签名密钥
    pub cache: CacheService,           // Redis
    pub agent_repo: AgentRepository,
    pub prompt_repo: PromptRepository,
    // ... embedding, vector_db, memory, models
}
```

## 核心 API

| Endpoint | Method | Description |
|---|---|---|
| `/api/chat` | POST | 流式聊天 (SSE) 或同步 JSON |
| `/api/chat/stop` | POST | 取消正在进行的生成 |
| `/api/chat/tool-result` | POST | 推送 client tool 执行结果 |
| `/api/chats` | GET | 列出用户会话 |
| `/api/prompts` | CRUD | Prompt 管理 |
| `/api/agents` | CRUD | Agent 管理 |
| `/api/workflows` | CRUD | Workflow 管理 |
| `/api/memories/search` | POST | 向量相似搜索 |

## SSE 统一流 — Tool Call 不中断机制

### 核心设计

SSE 流在遇到 client-side tool call 时**暂停但不断开**，前端执行完 tool 后通过 `/api/chat/tool-result` 发送结果，原始 SSE 流自动恢复继续输出。

使用 `tokio::sync::mpsc` channel 实现 SSE 流与 tool-result handler 之间的通信：

```
POST /api/chat  ──→ SSE 流打开
                    ├─ meta, content 事件
                    ├─ tool_call 事件（流暂停，等待 channel）
                    │
POST /tool-result ─→ 找到 channel sender → 推送结果 → 返回 JSON ack
                    │
                    ├─ SSE 流恢复（保存 tool result，重新调用 AI）
                    ├─ 更多 content 或再次 tool_call
                    └─ done 事件，流关闭
```

### 关键实现细节

**`AppState.tool_result_channels`**
- 类型：`DashMap<Uuid, mpsc::Sender<Vec<ToolResultPayload>>>`
- 生命周期：`chat_handler` 创建 → `run_chat_stream` 的 `defer!` 清理

**`chat_handler()` (mod.rs)**
- 流式模式下创建 `mpsc::channel(1)`，`tx` 存入 `tool_result_channels`，`rx` 传给 `run_chat_stream`
- 非流式模式下 `tool_result_rx = None`

**`run_chat_stream()` (logic.rs)**
- 签名新增 `tool_result_rx` 和 `tool_result_channels` 参数
- client_side_calls 分支：yield `ToolCall` → `tokio::select!` 等待 channel / 5分钟超时 / 取消
- 收到结果后保存消息 → `continue` 重新调用 AI
- 无 channel 时 `break`（非流式兼容）

**`tool_result_handler()` (mod.rs)**
- UUID 分支：验证 chat 归属 → 找 channel sender → `sender.send(results)` → 返回 JSON `{"status":"ok"}`
- Legacy 分支：workflow forwarding 保持不变

### Tool 分类

- **Server-side (MCP)**: 在 `run_chat_stream` 内自动执行，不暴露给前端
- **Client-side**: yield `ToolCall` 事件，前端执行后通过 channel 返回结果

### Message ID 分配

```
User Message ID = N        (由 chat_handler 分配)
Assistant Message ID = N+1 (run_chat_stream 分配)
[Server Tool Results: N+2, N+3, ...]
[Client Tool Results: N+2, N+3, ... (收到 channel 结果后分配)]
[Next Assistant: N+K+1]
```

### 超时与取消

- Tool result 等待超时：5 分钟 (`Duration::from_secs(300)`)
- Stop 取消：`cancel_token.cancelled()` 在 `tokio::select!` 中检测
- Channel 清理：`defer!` 同时清理 `active_requests` 和 `tool_result_channels`

## Auth 模式

- `Authorization: Bearer <JWT>` — 用户直连 (前端 → jug0)
- `X-API-KEY` — CLI/SDK 认证
- `X-USER-ID` + `X-ORG-ID` + `X-ORG-KEY` — 网关代理 (juglans-api → jug0)
- `X-Execution-Token` — Workflow 链式调用签名验证

## Message State 系统

- `context_visible` — 默认，出现在上下文中，前端可见
- `context_hidden` — 出现在上下文中，前端不可见
- `display_only` — 不出现在上下文中，仅展示
- `silent` — 不持久化，不展示

请求中可用组合语法：`input_state:output_state`（如 `context_visible:display_only`）

## 环境变量

```bash
DATABASE_URL=postgresql://...    # 必须
OPENAI_API_KEY=sk_xxxx           # OpenAI 兼容提供商
DEEPSEEK_API_KEY=...
GEMINI_API_KEY=...
QWEN_API_KEY=...
QDRANT_URL=http://localhost:6334
REDIS_URL=redis://127.0.0.1:6379
EXECUTION_SIGNING_KEY=...        # 生产环境必须设置
DEFAULT_LLM_MODEL=qwen-plus      # 默认模型
ENABLE_MEMORY=false               # 向量记忆开关
RUST_LOG=jug0=debug
```
