# 工作流示例

本目录包含不同复杂度的工作流示例。

## 文件列表

### simple-chat.jgflow
最基础的聊天工作流，演示：
- 基本节点和边的定义
- Agent 调用
- 通知节点使用

**运行方法：**
```bash
juglans simple-chat.jgflow --input '{"message": "Hello, how are you?"}'
```

### router.jgflow
智能路由工作流，演示：
- 条件分支 (`if` 语句)
- 多个 Agent 协作
- JSON 格式输出解析

**运行方法：**
```bash
# 技术查询
juglans router.jgflow --input '{"query": "How to implement binary search in Rust?"}'

# 数据分析查询
juglans router.jgflow --input '{"query": "Analyze this sales trend: up 20% YoY"}'

# 通用查询
juglans router.jgflow --input '{"query": "Tell me a joke"}'
```

### batch-process.jgflow
批量处理工作流，演示：
- `foreach` 循环
- 上下文变量管理 (`set_context`)
- Prompt 模板调用
- 结果聚合

**运行方法：**
```bash
juglans batch-process.jgflow --input '{
  "items": [
    {"id": 1, "name": "Product A", "value": 1000},
    {"id": 2, "name": "Product B", "value": 1500},
    {"id": 3, "name": "Product C", "value": 800}
  ]
}'
```

## 工作流模式

### 基本模式
- **线性流程**: `[A] -> [B] -> [C]`
- **分支**: `[A] if condition -> [B]`
- **汇聚**: 多个节点指向同一个节点

### 循环模式
- **foreach**: 遍历数组
- **while**: 条件循环

### 数据流模式
- **变量传递**: `$input`, `$output`, `$ctx`
- **上下文共享**: `set_context()` 设置全局变量
- **结果聚合**: 使用 `append()` 收集结果

## 最佳实践

1. **清晰命名** - 使用描述性的节点 ID
2. **错误处理** - 为关键操作添加 `on error` 路径
3. **进度反馈** - 使用 `notify()` 提供状态更新
4. **模块化** - 复杂逻辑拆分为多个小工作流
5. **变量管理** - 合理使用 `$input`, `$ctx`, `$output`

更多语法请参考：[工作流语法指南](../../docs/guide/workflow-syntax.md)

## 调试技巧

```bash
# 详细输出模式
juglans workflow.jgflow -v

# 仅验证语法
juglans workflow.jgflow --dry-run

# JSON 格式输出
juglans workflow.jgflow --output-format json
```
