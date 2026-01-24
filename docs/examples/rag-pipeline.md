# RAG 检索增强生成

检索相关文档并基于上下文生成回答的工作流。

## 概述

```
┌─────────┐    ┌─────────┐    ┌─────────┐    ┌─────────┐
│  Query  │───▶│ Embed   │───▶│ Search  │───▶│ Generate│
│         │    │         │    │         │    │         │
└─────────┘    └─────────┘    └─────────┘    └─────────┘
                                  │
                                  ▼
                            ┌─────────┐
                            │ Vector  │
                            │   DB    │
                            └─────────┘
```

## 工作流文件

### rag-pipeline.jgflow

```yaml
name: "RAG Pipeline"
description: "Retrieval-Augmented Generation workflow"

prompts: ["./prompts/*.jgprompt"]
agents: ["./agents/*.jgagent"]

entry: [embed_query]
exit: [respond]

# 1. 将查询转换为向量
[embed_query]: embed(
  text=$input.query,
  model="text-embedding-3-small"
)

# 2. 向量搜索
[search]: vector_search(
  embedding=$output,
  collection=$input.collection || "documents",
  top_k=$input.top_k || 5,
  threshold=$input.threshold || 0.7
)

# 3. 检查是否找到相关文档
[check_results]: set_context(
  has_results=len($output) > 0,
  documents=$output
)

[check_results] if !$ctx.has_results -> [no_context_response]
[check_results] -> [build_context]

# 4. 构建上下文
[build_context]: set_context(
  context=join(map($ctx.documents, d => d.content), "\n\n---\n\n")
)

# 5. 生成回答
[generate]: chat(
  agent="rag-responder",
  message=p(
    slug="rag-prompt",
    query=$input.query,
    context=$ctx.context
  )
)

# 无上下文时的回答
[no_context_response]: chat(
  agent="assistant",
  message="I don't have specific information about that in my knowledge base. " +
          "Here's what I can tell you generally:\n\n" + $input.query
)

# 汇总响应
[respond]: set_context(
  response=$output,
  sources=map($ctx.documents, d => {"id": d.id, "score": d.score})
)

[generate] -> [respond]
[no_context_response] -> [respond]
```

## Prompt 模板

### prompts/rag-prompt.jgprompt

```yaml
name: "rag-prompt"
description: "RAG context injection prompt"

template: |
  Based on the following context, answer the user's question.

  ## Context
  {{ context }}

  ## Question
  {{ query }}

  ## Instructions
  - Only use information from the provided context
  - If the context doesn't contain relevant information, say so
  - Cite specific parts of the context when possible
  - Be concise but thorough
```

## Agent 定义

### agents/rag-responder.jgagent

```yaml
name: "rag-responder"
description: "RAG response generator"

model: "claude-3-sonnet"
temperature: 0.3
max_tokens: 2048

system_prompt: |
  You are a helpful assistant that answers questions based on provided context.

  Rules:
  1. Only use information from the given context
  2. If the context doesn't have the answer, clearly state that
  3. Don't make up information
  4. Cite relevant parts of the context
  5. Be accurate and helpful
```

## 高级版本

### rag-with-rerank.jgflow

带重排序的 RAG：

```yaml
name: "RAG with Reranking"

entry: [embed_query]
exit: [respond]

# 嵌入查询
[embed_query]: embed(text=$input.query)

# 初次检索（取更多结果）
[search]: vector_search(
  embedding=$output,
  collection="documents",
  top_k=20
)

# 重排序
[rerank]: chat(
  agent="reranker",
  message=json({
    "query": $input.query,
    "documents": $output
  }),
  format="json"
)

# 取 top 5
[select_top]: set_context(
  documents=slice($output.ranked_documents, 0, 5)
)

# 构建上下文
[build_context]: set_context(
  context=join(map($ctx.documents, d => d.content), "\n\n---\n\n")
)

# 生成
[generate]: chat(
  agent="rag-responder",
  message=p(slug="rag-prompt", query=$input.query, context=$ctx.context)
)

[respond]: set_context(response=$output, sources=$ctx.documents)

[embed_query] -> [search] -> [rerank] -> [select_top] -> [build_context] -> [generate] -> [respond]
```

### agents/reranker.jgagent

```yaml
name: "reranker"
description: "Document reranking agent"

model: "claude-3-haiku"
temperature: 0

system_prompt: |
  You are a document relevance ranker. Given a query and documents,
  rerank them by relevance.

  Input format:
  {"query": "...", "documents": [...]}

  Output format:
  {
    "ranked_documents": [
      {"id": "...", "content": "...", "relevance_score": 0.95, "reasoning": "..."},
      ...
    ]
  }

  Sort by relevance_score descending.
```

### rag-with-hyde.jgflow

使用 HyDE（假设文档嵌入）：

```yaml
name: "RAG with HyDE"

entry: [generate_hypothetical]
exit: [respond]

# 生成假设性答案
[generate_hypothetical]: chat(
  agent="hyde-generator",
  message=$input.query
)

# 嵌入假设答案（而非原始查询）
[embed_hyde]: embed(text=$output)

# 搜索
[search]: vector_search(
  embedding=$output,
  collection="documents",
  top_k=5
)

# 后续同标准 RAG...
[build_context]: set_context(
  context=join(map($output, d => d.content), "\n\n---\n\n")
)

[generate]: chat(
  agent="rag-responder",
  message=p(slug="rag-prompt", query=$input.query, context=$ctx.context)
)

[respond]: set_context(response=$output)

[generate_hypothetical] -> [embed_hyde] -> [search] -> [build_context] -> [generate] -> [respond]
```

### agents/hyde-generator.jgagent

```yaml
name: "hyde-generator"
description: "Hypothetical document generator for HyDE"

model: "claude-3-haiku"
temperature: 0.7
max_tokens: 512

system_prompt: |
  Given a question, write a hypothetical passage that would answer it.
  Write as if you're quoting from an authoritative source.
  Don't hedge or say "I think" - write confidently.
  Keep it to 2-3 paragraphs.
```

## 多源 RAG

### multi-source-rag.jgflow

```yaml
name: "Multi-source RAG"

entry: [embed_query]
exit: [respond]

[embed_query]: embed(text=$input.query)

# 并行搜索多个源（使用上下文保存嵌入）
[save_embedding]: set_context(query_embedding=$output)

# 搜索文档库
[search_docs]: vector_search(
  embedding=$ctx.query_embedding,
  collection="documents",
  top_k=3
)

# 搜索 FAQ
[search_faq]: vector_search(
  embedding=$ctx.query_embedding,
  collection="faq",
  top_k=2
)

# 搜索历史对话
[search_history]: vector_search(
  embedding=$ctx.query_embedding,
  collection="chat_history",
  top_k=2
)

# 合并结果
[merge_results]: set_context(
  all_sources={
    "documents": $ctx.doc_results,
    "faq": $ctx.faq_results,
    "history": $ctx.history_results
  }
)

# 构建分类上下文
[build_context]: set_context(
  context="## Documents\n" + join(map($ctx.all_sources.documents, d => d.content), "\n") +
          "\n\n## FAQ\n" + join(map($ctx.all_sources.faq, d => d.content), "\n") +
          "\n\n## Related Conversations\n" + join(map($ctx.all_sources.history, d => d.content), "\n")
)

# 生成
[generate]: chat(
  agent="rag-responder",
  message=p(slug="rag-prompt", query=$input.query, context=$ctx.context)
)

[respond]: set_context(
  response=$output,
  sources=$ctx.all_sources
)

[embed_query] -> [save_embedding]
[save_embedding] -> [search_docs]
[save_embedding] -> [search_faq]
[save_embedding] -> [search_history]

[search_docs] -> [merge_results]
[search_faq] -> [merge_results]
[search_history] -> [merge_results]

[merge_results] -> [build_context] -> [generate] -> [respond]
```

## 运行示例

```bash
# 基本 RAG
juglans rag-pipeline.jgflow --input '{
  "query": "How do I reset my password?",
  "collection": "help_docs"
}'

# 带重排序
juglans rag-with-rerank.jgflow --input '{
  "query": "What are the pricing plans?"
}'

# 多源 RAG
juglans multi-source-rag.jgflow --input '{
  "query": "How to configure API authentication?"
}'
```

## 目录结构

```
rag-pipeline/
├── rag-pipeline.jgflow
├── rag-with-rerank.jgflow
├── rag-with-hyde.jgflow
├── multi-source-rag.jgflow
├── agents/
│   ├── rag-responder.jgagent
│   ├── reranker.jgagent
│   └── hyde-generator.jgagent
├── prompts/
│   └── rag-prompt.jgprompt
└── test-inputs/
    └── sample-queries.json
```
