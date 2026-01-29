# Agent 配置示例

本目录包含常用的 Agent 配置示例。

## 文件列表

### assistant.jgagent
通用助手 Agent，演示：
- 基本 Agent 配置
- 系统提示设置
- 温度参数调整

**使用方法：**
```bash
# 交互式对话
juglans assistant.jgagent

# 发送单条消息
juglans assistant.jgagent --message "What is Rust?"

# 查看配置信息
juglans assistant.jgagent --info
```

### analyst.jgagent
数据分析 Agent，演示：
- 专业化配置
- 较低温度（更确定性）
- 技能标签配置

**使用方法：**
```bash
juglans analyst.jgagent --message "Analyze this sales data: Q1: 100K, Q2: 120K, Q3: 115K, Q4: 140K"
```

## Agent 配置要点

### 必填字段
- `slug` - 唯一标识符
- `name` - 显示名称
- `model` - 使用的模型
- `system_prompt` - 系统提示（可以是字符串或 `p(slug="...")` 引用）

### 可选字段
- `temperature` - 温度参数 (0.0-2.0)
- `description` - Agent 描述
- `skills` - 技能标签列表
- `mcp` - MCP 服务器列表
- `workflow` - 关联的工作流文件路径

### 温度建议
- **0.0-0.3**: 事实性任务（分析、翻译、代码）
- **0.5-0.7**: 平衡（通用助手）
- **0.8-1.0**: 创造性任务（写作、头脑风暴）

更多详情请参考：[Agent 语法指南](../../docs/guide/agent-syntax.md)
