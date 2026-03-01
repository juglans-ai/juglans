# 工具定义文件（Tools）

本指南介绍如何使用工具定义文件（`.json`）管理和复用 AI 工具配置。

## 什么是工具定义文件

工具定义文件允许你将 OpenAI Function Calling 格式的工具定义独立存储，便于：

- **模块化管理** - 分离工具定义和业务逻辑
- **复用** - 多个 Agent 和 Workflow 共享同一工具集
- **版本控制** - 独立追踪工具定义的变更
- **团队协作** - 不同成员维护不同的工具集

## 文件格式

### 基本结构

```json
{
  "slug": "web-tools",
  "name": "Web Scraping Tools",
  "description": "Tools for fetching and parsing web content",
  "tools": [
    {
      "type": "function",
      "function": {
        "name": "fetch_url",
        "description": "获取网页内容",
        "parameters": {
          "type": "object",
          "properties": {
            "url": {"type": "string", "description": "目标 URL"}
          },
          "required": ["url"]
        }
      }
    }
  ]
}
```

### 字段说明

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `slug` | string | 是 | 唯一标识符，用于引用 |
| `name` | string | 是 | 工具集名称 |
| `description` | string | 否 | 工具集描述 |
| `tools` | array | 是 | 工具定义数组（OpenAI 格式） |

## 在 Workflow 中使用

### 1. 导入工具定义

在 workflow 文件头部导入工具定义：

```yaml
name: "My Workflow"

# 导入工具定义文件
tools: ["./tools/*.json"]
agents: ["./agents/*.jgagent"]
prompts: ["./prompts/*.jgprompt"]

entry: [start]
```

### 2. 引用工具集

#### 单个工具集

```yaml
[step]: chat(
  agent="assistant",
  message=$input.query,
  tools="web-tools"  # 引用 slug
)
```

#### 多个工具集

```yaml
[step]: chat(
  agent="assistant",
  message=$input.query,
  tools=["web-tools", "data-tools"]  # 合并多个工具集
)
```

#### 内联 JSON（向后兼容）

```yaml
[step]: chat(
  agent="assistant",
  message=$input.query,
  tools=[
    {
      "type": "function",
      "function": {"name": "custom_tool", ...}
    }
  ]
)
```

## 在 Agent 中使用

### Agent 默认工具

在 `.jgagent` 文件中配置默认工具集：

```yaml
slug: "web-agent"
model: "gpt-4o"
system_prompt: "You are a web scraping assistant."

# 单个工具集
tools: "web-tools"

# 或多个工具集
tools: ["web-tools", "data-tools"]
```

Agent 的默认工具会自动附加到所有 `chat()` 调用，除非 workflow 中显式覆盖。

## 内置开发者工具 (devtools)

Juglans 内置 6 个 Claude Code 风格的开发者工具，自动注册为 `"devtools"` slug。无需创建 JSON 文件，直接引用即可。

### 在 Agent 中使用

```yaml
slug: "code-assistant"
model: "deepseek-chat"
tools: ["devtools"]

# 可与其他工具集组合
# tools: ["devtools", "web-tools"]
```

### 在 Workflow 中使用

devtools 作为内置工具可直接在节点中调用，无需在 `tools:` 字段中声明：

```yaml
# 直接作为节点调用
[read]: read_file(file_path="./src/main.rs")
[search]: grep(pattern="TODO|FIXME", path="./src")
[review]: chat(agent="reviewer", message="Review:\n$read.output.content")
[read] -> [search] -> [review]
```

### 包含的工具

| 工具 | 说明 |
|------|------|
| `read_file` | 读取文件，返回带行号的内容 |
| `write_file` | 写入文件，自动创建父目录 |
| `edit_file` | 精确字符串替换 |
| `glob` | 文件模式匹配 |
| `grep` | 正则搜索文件内容 |
| `bash` | 执行 Shell 命令（别名: `sh`） |

详细参数参见 [内置工具参考](../reference/builtins.md#开发者工具)。

## 优先级规则

```
Workflow 内联 JSON > Workflow 引用 > Agent 默认
```

示例：

```yaml
# src/agents/my-agent.jgagent
tools: "default-tools"

# workflow.jg
[step1]: chat(agent="my-agent", message="...")
# 使用 "default-tools"

[step2]: chat(agent="my-agent", message="...", tools="override-tools")
# 使用 "override-tools"（覆盖）
```

## 工具合并和去重

当引用多个工具集时：

```yaml
tools: ["web-tools", "data-tools"]
```

系统会：
1. 加载所有工具集
2. 合并所有工具定义
3. 去重（同名工具后者覆盖前者）

```
web-tools: [fetch_url, parse_html]
data-tools: [calculate, fetch_url]  # fetch_url 覆盖 web-tools 的版本

最终: [parse_html, calculate, fetch_url]
```

## 示例

### 示例 1: Web 抓取工具

**tools/web-tools.json:**

```json
{
  "slug": "web-tools",
  "name": "Web Scraping Tools",
  "tools": [
    {
      "type": "function",
      "function": {
        "name": "fetch_url",
        "description": "获取网页内容",
        "parameters": {
          "type": "object",
          "properties": {
            "url": {"type": "string"},
            "method": {"type": "string", "enum": ["GET", "POST"]}
          },
          "required": ["url"]
        }
      }
    }
  ]
}
```

**workflow.jg:**

```yaml
tools: ["./tools/*.json"]

[fetch]: chat(
  agent="assistant",
  message="Fetch https://example.com",
  tools="web-tools"
)
```

### 示例 2: 组合多个工具集

**tools/math-tools.json:**

```json
{
  "slug": "math-tools",
  "tools": [
    {
      "type": "function",
      "function": {
        "name": "calculate",
        "description": "执行数学计算"
      }
    }
  ]
}
```

**agents/analyst.jgagent:**

```yaml
slug: "analyst"
tools: ["web-tools", "math-tools"]  # 组合工具
```

## 最佳实践

### 1. 命名规范

```
tools/
├── web-tools.json      # 功能分类命名
├── data-tools.json
├── api-tools.json
└── custom-tools.json
```

### 2. 工具粒度

- **粗粒度** - 按功能领域分组（web-tools, data-tools）
- **细粒度** - 按具体用途拆分（github-tools, slack-tools）

选择适合团队的粒度。

### 3. 版本管理

```bash
# 提交工具定义到版本控制
git add tools/
git commit -m "feat: Add web scraping tools"
```

### 4. 文档化

在工具定义中提供清晰的描述：

```json
{
  "slug": "api-tools",
  "description": "连接外部 API 的工具集，包括认证和数据转换",
  "tools": [...]
}
```

### 5. 测试

创建测试 workflow 验证工具定义：

```yaml
name: "Test Web Tools"
tools: ["./tools/web-tools.json"]

[test]: chat(
  agent="assistant",
  message="Test fetch_url tool",
  tools="web-tools"
)
```

## 错误处理

### 工具集不存在

```yaml
tools: "nonexistent"  # ❌ 错误
```

错误信息：
```
Tool resource 'nonexistent' not found
```

**解决方法：**
1. 检查 slug 拼写
2. 确认工具文件已导入
3. 查看加载日志

### 工具定义格式错误

```json
{
  "slug": "bad-tools",
  "tools": "not an array"  // ❌ 错误
}
```

**解决方法：**
检查 JSON 格式，确保 `tools` 是数组。

## 调试

### 查看加载的工具

启用调试日志：

```bash
DEBUG=true juglans workflow.jg
```

输出：
```
📦 Loading tool definitions from 1 pattern(s)...
  ✅ Loaded 2 tool resource(s) with 5 total tools
Registered tool resource: web-tools
Registered tool resource: data-tools
```

### 工具解析日志

```
Resolving tool reference: web-tools
🛠️ Attaching 2 custom tools to the request.
```

## 相关文档

- [Agent 配置](./agent-syntax.md) - Agent 默认工具配置
- [Workflow 语法](./workflow-syntax.md) - 导入工具定义
- [内置工具](../reference/builtins.md) - chat() 参数说明
