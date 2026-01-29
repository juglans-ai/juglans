# Prompt 模板示例

本目录包含常用的 Prompt 模板示例。

## 文件列表

### greeting.jgprompt
简单的问候模板，演示：
- 基本变量插值 `{{ name }}`
- 条件渲染 `{% if %}`
- 默认值设置

**使用方法：**
```bash
# 使用默认值渲染
juglans greeting.jgprompt

# 传入自定义变量
juglans greeting.jgprompt --input '{"name": "Alice", "language": "Chinese"}'
```

### analysis.jgprompt
数据分析提示模板，演示：
- 数组变量
- 循环渲染 `{% for %}`
- 结构化输出指导

**使用方法：**
```bash
juglans analysis.jgprompt --input '{
  "data": [
    {"name": "Sales", "value": 12345},
    {"name": "Users", "value": 5678}
  ],
  "focus": "growth trends"
}'
```

## Prompt 语法要点

1. **Front Matter** - 使用 `---` 包围的 YAML 元数据
2. **变量插值** - `{{ variable_name }}`
3. **条件语句** - `{% if condition %} ... {% endif %}`
4. **循环** - `{% for item in items %} ... {% endfor %}`
5. **默认值** - 在 `inputs` 中定义

更多语法请参考：[Prompt 语法指南](../../docs/guide/prompt-syntax.md)
