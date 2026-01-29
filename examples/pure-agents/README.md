# Pure Agents

Pure agents 是不绑定 workflow 的独立 agent，用于简单的对话场景。

## 特点

- ✅ 无需 workflow
- ✅ 简洁配置
- ✅ 快速测试
- ✅ 直接运行

## 使用方法

```bash
# 交互式对话
juglans pure-agents/deepseek.jgagent

# 退出
输入 exit 或 quit
```

## 示例

```bash
cd examples
juglans pure-agents/deepseek.jgagent
```

## 配置说明

Pure agent 只需要基本字段：
- `slug`: 唯一标识符
- `name`: 显示名称
- `model`: 模型名称
- `temperature`: 温度参数
- `system_prompt`: 系统提示词

**不需要** `workflow` 字段。
