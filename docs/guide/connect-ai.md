# How to Connect AI Models

本指南介绍如何配置 Juglans 连接 AI 模型，包括 Jug0 后端和本地模型。

## Configure Jug0

Juglans 通过 Jug0 后端调用 LLM。在项目根目录的 `juglans.toml` 中配置：

```toml
[account]
id = "your_user_id"
api_key = "jug0_sk_your_api_key"

[jug0]
base_url = "http://localhost:3000"   # 本地开发
# base_url = "https://api.jug0.com" # 生产环境
```

配置文件搜索顺序：`./juglans.toml` -> `~/.config/juglans/juglans.toml` -> `/etc/juglans/juglans.toml`。

也可以通过环境变量覆盖：

```bash
export JUGLANS_API_KEY="jug0_sk_..."
export JUGLANS_JUG0_URL="http://localhost:3000"
```

## Get API Key

1. 登录 Jug0 控制台
2. 进入 Settings > API Keys
3. 创建新的 API Key（格式：`jug0_sk_...`）
4. 复制到 `juglans.toml` 的 `[account].api_key` 字段

## Test Connection

创建一个最小的 chat workflow 验证连接：

```juglans
name: "Connection Test"

entry: [test]
exit: [done]

[test]: chat(agent="assistant", message="Say hello in one word.")
[done]: print(message="Connection OK. Response: " + $output)

[test] -> [done]
```

运行：

```bash
juglans test-connection.jg
```

如果配置正确，你会看到模型的回复。如果失败，检查：

- `juglans.toml` 中的 `api_key` 和 `base_url` 是否正确
- Jug0 后端是否在运行（`curl http://localhost:3000/health`）
- 使用 `juglans whoami --check-connection` 测试连接状态

## Use Local Models (Ollama)

Juglans 支持通过 Jug0 后端连接本地模型。配置 Ollama 示例：

```toml
[jug0]
base_url = "http://localhost:3000"

# Jug0 后端配置中设置 Ollama provider
# 然后在 .jgagent 中指定 model
```

创建使用本地模型的 Agent：

```jgagent
slug: "local-agent"
model: "ollama/llama3"
temperature: 0.7
system_prompt: "You are a helpful assistant."
```

在 Workflow 中使用：

```juglans
agents: ["./agents/*.jgagent"]

entry: [ask]
exit: [done]

[ask]: chat(agent="local-agent", message=$input.query)
[done]: print(message=$output)

[ask] -> [done]
```

## Resource Management

Juglans 资源（Workflow、Agent、Prompt）可以在本地和 Jug0 之间同步。

### Push (Local -> Remote)

```bash
# 推送单个文件
juglans push src/prompts/greeting.jgprompt

# 强制覆盖
juglans push src/agents/assistant.jgagent --force

# 批量推送（使用 workspace 配置）
juglans push

# 预览
juglans push --dry-run
```

### Pull (Remote -> Local)

```bash
juglans pull my-prompt --type prompt
juglans pull my-agent --type agent --output ./agents/
```

### List and Delete

```bash
juglans list                    # 列出所有远程资源
juglans list --type agent       # 只列出 Agent
juglans delete old-prompt --type prompt
```

## Local vs Remote Resources

| | Local | Remote |
|---|---|---|
| 引用方式 | slug（如 `"my-agent"`） | owner/slug（如 `"juglans/assistant"`） |
| 需要导入 | 是（`agents: ["./agents/*.jgagent"]`） | 否，直接引用 |
| 适用场景 | 开发、测试 | 生产部署、团队共享 |

在同一个 Workflow 中混合使用：

```juglans
name: "Hybrid Workflow"

agents: ["./agents/*.jgagent"]

entry: [start]
exit: [end]

[start]: print(msg="begin")
[local_chat]: chat(agent="my-agent", message=$input.query)
[remote_chat]: chat(agent="juglans/premium-agent", message=$output)
[end]: print(msg="done")

[start] -> [local_chat] -> [remote_chat] -> [end]
```

## Troubleshooting

| Problem | Solution |
|---------|----------|
| `Connection refused` | 确认 Jug0 后端正在运行，检查 `base_url` |
| `401 Unauthorized` | 检查 `api_key` 是否正确 |
| `Agent not found` | 确认 Agent slug 拼写正确，本地 Agent 需要 `agents:` 导入 |
| `Timeout` | 增大超时配置，或检查网络连接 |

## Next Steps

- [Jug0 Integration](../integrations/jug0.md) — 完整 API 参考
- [Agent Syntax](./agent-syntax.md) — Agent 配置详解
- [Configuration Reference](../reference/config.md) — 完整配置项
