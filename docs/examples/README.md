# 示例集合

本目录包含各种 Juglans 工作流示例。

## 示例列表

| 示例 | 说明 | 难度 |
|------|------|------|
| [basic-chat](./basic-chat.md) | 基础对话工作流 | 入门 |
| [intent-router](./intent-router.md) | 意图分类路由 | 入门 |
| [tool-calling](./tool-calling.md) | Function Calling 工具调用 | 入门 |
| [rag-pipeline](./rag-pipeline.md) | RAG 检索增强生成 | 中级 |
| [multi-agent](./multi-agent.md) | 多 Agent 协作 | 中级 |
| [code-review](./code-review.md) | 自动代码审查 | 中级 |
| [data-pipeline](./data-pipeline.md) | 数据处理管道 | 高级 |

## 按场景分类

### 对话类
- 基础对话 - 单轮问答
- 多轮对话 - 带上下文记忆
- 意图路由 - 分类并分发

### 内容生成
- 文章生成 - 带质量检查
- 摘要提取 - 长文本压缩
- 翻译工作流 - 多语言转换

### 数据处理
- 批量处理 - 遍历集合
- ETL 管道 - 提取转换加载
- RAG 检索 - 向量搜索 + 生成

### 工具集成
- GitHub 集成 - PR/Issue 自动化
- 文件处理 - 读写本地文件
- API 调用 - 外部服务集成

## 运行示例

```bash
# 克隆示例
git clone https://github.com/juglans-ai/juglans-examples.git
cd juglans-examples

# 运行基础示例
juglans basic-chat.jgflow --input '{"message": "Hello!"}'

# 运行带配置的示例
juglans rag-pipeline.jgflow --input '{"query": "What is Juglans?"}' --config juglans.toml
```

## 示例结构

每个示例包含：

```
example-name/
├── workflow.jgflow      # 主工作流
├── prompts/             # Prompt 模板
│   └── *.jgprompt
├── agents/              # Agent 定义
│   └── *.jgagent
├── README.md            # 说明文档
└── test-input.json      # 测试输入
```
