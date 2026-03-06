# RAG Retrieval-Augmented Generation

A workflow that retrieves relevant documents and generates answers based on context.

## Overview

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

## Workflow File

### rag-pipeline.jg

```juglans
name: "RAG Pipeline"
description: "Retrieval-Augmented Generation workflow"

prompts: ["./prompts/*.jgprompt"]
agents: ["./agents/*.jgagent"]

entry: [embed_query]
exit: [respond]

# 1. Convert query to vector
[embed_query]: embed(
  text=$input.query,
  model="text-embedding-3-small"
)

# 2. Vector search
[search]: vector_search(
  embedding=$output,
  collection=$input.collection || "documents",
  top_k=$input.top_k || 5,
  threshold=$input.threshold || 0.7
)

# 3. Check if relevant documents were found
[check_results]: set_context(
  has_results=len($output) > 0,
  documents=$output
)

# 4. Build context
[build_context]: set_context(
  context=join(map($ctx.documents, d => d.content), "\n\n---\n\n")
)

# 5. Generate answer
[generate]: chat(
  agent="rag-responder",
  message=p(
    slug="rag-prompt",
    query=$input.query,
    context=$ctx.context
  )
)

# Response when no context is available
[no_context_response]: chat(
  agent="assistant",
  message="I don't have specific information about that in my knowledge base. " +
          "Here's what I can tell you generally:\n\n" + $input.query
)

# Aggregate response
[respond]: set_context(
  response=$output,
  sources=map($ctx.documents, d => {"id": d.id, "score": d.score})
)

[check_results] if !$ctx.has_results -> [no_context_response]
[check_results] -> [build_context]

[generate] -> [respond]
[no_context_response] -> [respond]
```

## Prompt Template

### src/prompts/rag-prompt.jgprompt

```jgprompt
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

## Agent Definition

### src/agents/rag-responder.jgagent

```jgagent
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

## Advanced Version

### rag-with-rerank.jg

RAG with reranking:

```juglans
name: "RAG with Reranking"

entry: [embed_query]
exit: [respond]

# Embed query
[embed_query]: embed(text=$input.query)

# Initial retrieval (get more results)
[search]: vector_search(
  embedding=$output,
  collection="documents",
  top_k=20
)

# Reranking
[rerank]: chat(
  agent="reranker",
  message=json({
    "query": $input.query,
    "documents": $output
  }),
  format="json"
)

# Take top 5
[select_top]: set_context(
  documents=slice($output.ranked_documents, 0, 5)
)

# Build context
[build_context]: set_context(
  context=join(map($ctx.documents, d => d.content), "\n\n---\n\n")
)

# Generate
[generate]: chat(
  agent="rag-responder",
  message=p(slug="rag-prompt", query=$input.query, context=$ctx.context)
)

[respond]: set_context(response=$output, sources=$ctx.documents)

[embed_query] -> [search] -> [rerank] -> [select_top] -> [build_context] -> [generate] -> [respond]
```

### src/agents/reranker.jgagent

```jgagent
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

### rag-with-hyde.jg

Using HyDE (Hypothetical Document Embedding):

```juglans
name: "RAG with HyDE"

entry: [generate_hypothetical]
exit: [respond]

# Generate hypothetical answer
[generate_hypothetical]: chat(
  agent="hyde-generator",
  message=$input.query
)

# Embed the hypothetical answer (instead of the raw query)
[embed_hyde]: embed(text=$output)

# Search
[search]: vector_search(
  embedding=$output,
  collection="documents",
  top_k=5
)

# The rest follows standard RAG...
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

### src/agents/hyde-generator.jgagent

```jgagent
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

## Multi-source RAG

### multi-source-rag.jg

```juglans
name: "Multi-source RAG"

entry: [embed_query]
exit: [respond]

[embed_query]: embed(text=$input.query)

# Parallel search across multiple sources (save embedding using context)
[save_embedding]: set_context(query_embedding=$output)

# Search document library
[search_docs]: vector_search(
  embedding=$ctx.query_embedding,
  collection="documents",
  top_k=3
)

# Search FAQ
[search_faq]: vector_search(
  embedding=$ctx.query_embedding,
  collection="faq",
  top_k=2
)

# Search conversation history
[search_history]: vector_search(
  embedding=$ctx.query_embedding,
  collection="chat_history",
  top_k=2
)

# Merge results
[merge_results]: set_context(
  all_sources={
    "documents": $ctx.doc_results,
    "faq": $ctx.faq_results,
    "history": $ctx.history_results
  }
)

# Build categorized context
[build_context]: set_context(
  context="## Documents\n" + join(map($ctx.all_sources.documents, d => d.content), "\n") +
          "\n\n## FAQ\n" + join(map($ctx.all_sources.faq, d => d.content), "\n") +
          "\n\n## Related Conversations\n" + join(map($ctx.all_sources.history, d => d.content), "\n")
)

# Generate
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

## Running Examples

```bash
# Basic RAG
juglans rag-pipeline.jg --input '{
  "query": "How do I reset my password?",
  "collection": "help_docs"
}'

# With reranking
juglans rag-with-rerank.jg --input '{
  "query": "What are the pricing plans?"
}'

# Multi-source RAG
juglans multi-source-rag.jg --input '{
  "query": "How to configure API authentication?"
}'
```

## Directory Structure

```
rag-pipeline/
├── rag-pipeline.jg
├── rag-with-rerank.jg
├── rag-with-hyde.jg
├── multi-source-rag.jg
├── agents/
│   ├── rag-responder.jgagent
│   ├── reranker.jgagent
│   └── hyde-generator.jgagent
├── prompts/
│   └── rag-prompt.jgprompt
└── test-inputs/
    └── sample-queries.json
```
