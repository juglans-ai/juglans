# Agent 配置语法 (.jgagent)

`.jgagent` 文件定义 AI Agent 的配置，包括模型、行为和能力。

## 基本结构

```yaml
slug: "agent_identifier"
name: "Display Name"
description: "A brief description of what this agent does"
model: "deepseek-chat"
temperature: 0.7
system_prompt: "You are a helpful assistant."
```

## 配置字段

| 字段 | 类型 | 必填 | 默认值 | 说明 |
|------|------|------|--------|------|
| `slug` | string | 是 | - | 唯一标识符 |
| `name` | string | 否 | - | 显示名称 |
| `description` | string | 否 | - | Agent 描述 |
| `model` | string | 否 | gpt-4o | 模型名称 |
| `temperature` | float | 否 | 0.7 | 温度参数 (0-2) |
| `system_prompt` | string | 否 | - | 系统提示词（内联或引用） |
| `tools` | array/string | 否 | - | 默认工具配置（JSON 数组或字符串） |
| `mcp` | array | 否 | [] | MCP 服务器列表 |
| `skills` | array | 否 | [] | 技能列表 |
| `workflow` | string | 否 | - | 关联的工作流文件路径 |

## 模型配置

### 支持的模型

```yaml
# DeepSeek
model: "deepseek-chat"
model: "deepseek-coder"

# OpenAI
model: "gpt-4o"
model: "gpt-4-turbo"
model: "gpt-3.5-turbo"

# Anthropic
model: "claude-3-opus"
model: "claude-3-sonnet"
model: "claude-3-haiku"

# 本地模型 (Ollama)
model: "llama3"
model: "codellama"
model: "mistral"
```

### 温度参数

```yaml
temperature: 0.0    # 确定性输出
temperature: 0.7    # 平衡创造性（推荐）
temperature: 1.0    # 更多随机性
temperature: 2.0    # 高度随机
```

## 系统提示词

### 内联方式

```yaml
system_prompt: "You are a helpful assistant."

# 多行提示词
system_prompt: |
  You are a professional data analyst.

  Your responsibilities:
  - Analyze data accurately
  - Provide clear insights
  - Use proper formatting
```

### 引用 Prompt 文件

```yaml
system_prompt: p(slug="system-analyst")
```

这会从已加载的 Prompt 中查找 `slug="system-analyst"` 的模板作为系统提示词。

## 工具配置

Agent 可以配置默认的工具集，这些工具在调用 `chat()` 时会自动附加到请求中（除非工作流中显式指定了 `tools` 参数）。

### JSON 数组格式

直接使用 JSON 数组定义工具，无需转义：

```yaml
slug: "web-agent"
model: "gpt-4o"
tools: [
  {
    "type": "function",
    "function": {
      "name": "fetch_url",
      "description": "获取网页内容",
      "parameters": {
        "type": "object",
        "properties": {
          "url": {"type": "string", "description": "目标 URL"},
          "method": {"type": "string", "enum": ["GET", "POST"]}
        },
        "required": ["url"]
      }
    }
  },
  {
    "type": "function",
    "function": {
      "name": "parse_html",
      "description": "解析 HTML 内容",
      "parameters": {
        "type": "object",
        "properties": {
          "html": {"type": "string", "description": "HTML 源码"},
          "selector": {"type": "string", "description": "CSS 选择器"}
        },
        "required": ["html"]
      }
    }
  }
]
```

### 字符串格式

也可以使用 JSON 字符串：

```yaml
slug: "tool-agent"
model: "deepseek-chat"
tools: "[{\"type\":\"function\",\"function\":{\"name\":\"calculator\",\"description\":\"执行数学计算\"}}]"
```

### 工具优先级

当工作流中的 `chat()` 调用同时指定了 `tools` 参数时，工作流中的配置优先：

```yaml
# Agent 配置
slug: "my-agent"
tools: [{"type": "function", "function": {"name": "default_tool"}}]

# 工作流中使用
[step]: chat(
  agent="my-agent",
  message=$input,
  tools=[{"type": "function", "function": {"name": "override_tool"}}]  # 这个会被使用
)

# 不指定 tools 时使用 Agent 的默认配置
[step2]: chat(
  agent="my-agent",
  message=$input  # 会使用 Agent 配置的 default_tool
)
```

## MCP 工具集成

配置 Model Context Protocol 服务器以扩展 Agent 能力：

```yaml
slug: "tool-agent"
model: "gpt-4o"
mcp:
  - "filesystem"     # 文件系统操作
  - "github"         # GitHub 操作
  - "database"       # 数据库操作
```

MCP 服务器需要在 `juglans.toml` 中配置（HTTP 连接方式）：

```toml
[[mcp_servers]]
name = "filesystem"
base_url = "http://localhost:3001/mcp/filesystem"

[[mcp_servers]]
name = "github"
base_url = "http://localhost:3001/mcp/github"
token = "${GITHUB_TOKEN}"
```

**注意：** Juglans 使用 HTTP/JSON-RPC 连接 MCP 服务器，需要先启动 MCP 服务。详见 [MCP 集成指南](../integrations/mcp.md)。

## 技能系统

为 Agent 添加预定义技能：

```yaml
slug: "skilled-agent"
model: "deepseek-chat"
skills:
  - "code_review"
  - "documentation"
  - "testing"
```

## 关联工作流

将 Agent 与特定工作流绑定：

```yaml
slug: "workflow-agent"
model: "gpt-4o"
workflow: "./workflows/complex-task.jgflow"
```

当用户与此 Agent 对话时，可以触发关联的工作流执行。

## 完整示例

### 通用助手

```yaml
slug: "assistant"
name: "General Assistant"
model: "deepseek-chat"
temperature: 0.7
system_prompt: |
  You are a helpful, harmless, and honest AI assistant.

  Guidelines:
  - Be concise and clear
  - Admit when you don't know something
  - Ask clarifying questions when needed
```

### 代码专家

```yaml
slug: "code-expert"
name: "Code Expert"
model: "deepseek-coder"
temperature: 0.3
system_prompt: |
  You are an expert software engineer with deep knowledge of:
  - Python, TypeScript, Rust, Go
  - System design and architecture
  - Best practices and design patterns

  When providing code:
  1. Write clean, readable code
  2. Include comments for complex logic
  3. Consider edge cases
  4. Suggest tests when appropriate
mcp:
  - "code-executor"
skills:
  - "code_review"
  - "refactoring"
```

### 数据分析师

```yaml
slug: "data-analyst"
name: "Data Analyst"
model: "gpt-4o"
temperature: 0.5
system_prompt: p(slug="analyst-system-prompt")
mcp:
  - "python-executor"
  - "chart-generator"
skills:
  - "data_visualization"
  - "statistical_analysis"
```

### 创意写作

```yaml
slug: "creative-writer"
name: "Creative Writer"
model: "claude-3-opus"
temperature: 1.2
system_prompt: |
  You are a creative writing assistant with a talent for:
  - Storytelling and narrative
  - Poetry and prose
  - Marketing copy
  - Script writing

  Be imaginative, evocative, and original.
  Adapt your style to match the requested genre or tone.
```

### 路由 Agent

```yaml
slug: "router"
name: "Intent Router"
model: "gpt-3.5-turbo"
temperature: 0.0
system_prompt: |
  You are an intent classifier. Analyze the user's message and classify it.

  Categories:
  - technical: Programming, system, debugging questions
  - creative: Writing, design, artistic requests
  - analytical: Data, research, analysis tasks
  - general: General conversation, simple questions

  Respond with ONLY a JSON object:
  {"category": "...", "confidence": 0.0-1.0}
```

### 多步骤工作流 Agent

```yaml
slug: "research-agent"
name: "Research Agent"
model: "gpt-4o"
temperature: 0.7
system_prompt: |
  You are a research assistant capable of:
  1. Breaking down complex questions
  2. Searching for information
  3. Synthesizing findings
  4. Providing cited conclusions
workflow: "./workflows/research-pipeline.jgflow"
mcp:
  - "web-search"
  - "document-reader"
```

## 在工作流中使用

### 基本调用

```yaml
[chat]: chat(agent="assistant", message=$input.question)
```

### 指定输出格式

```yaml
[classify]: chat(
  agent="router",
  message=$input.query,
  format="json"
)
```

### 无状态调用

```yaml
[analyze]: chat(
  agent="analyst",
  message=$input.data,
  stateless="true"    # 不保存到对话历史
)
```

## 交互式使用

直接与 Agent 对话：

```bash
juglans agents/assistant.jgagent
```

传入初始消息：

```bash
juglans agents/assistant.jgagent --message "Hello, how are you?"
```

## 最佳实践

1. **明确角色** - 在 system_prompt 中清晰定义 Agent 的角色和能力
2. **适当温度** - 根据任务类型选择温度（分析任务低，创意任务高）
3. **模块化** - 一个 Agent 专注一个领域
4. **可组合** - 设计可以协作的多个 Agent
5. **测试验证** - 用多种输入测试 Agent 行为

## 调试

### 查看 Agent 配置

```bash
juglans agents/my-agent.jgagent --info
```

### 详细日志

```bash
juglans agents/my-agent.jgagent --verbose
```
