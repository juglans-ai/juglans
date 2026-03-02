# Example Collection

This directory contains various Juglans workflow examples.

## Example List

| Example | Description | Difficulty |
|---------|-------------|------------|
| [basic-chat](./basic-chat.md) | Basic chat workflow | Beginner |
| [intent-router](./intent-router.md) | Intent classification routing | Beginner |
| [tool-calling](./tool-calling.md) | Function Calling tool invocation | Beginner |
| [rag-pipeline](./rag-pipeline.md) | RAG Retrieval-Augmented Generation | Intermediate |
| [multi-agent](./multi-agent.md) | Multi-Agent collaboration | Intermediate |
| [code-review](./code-review.md) | Automated code review | Intermediate |
| [data-pipeline](./data-pipeline.md) | Data processing pipeline | Advanced |

## By Category

### Conversational
- Basic chat - Single-turn Q&A
- Multi-turn chat - With context memory
- Intent routing - Classify and dispatch

### Content Generation
- Article generation - With quality checks
- Summary extraction - Long text compression
- Translation workflow - Multi-language conversion

### Data Processing
- Batch processing - Iterate over collections
- ETL pipeline - Extract, Transform, Load
- RAG retrieval - Vector search + generation

### Tool Integration
- GitHub integration - PR/Issue automation
- File processing - Read/write local files
- API calls - External service integration

## Running Examples

```bash
# Clone examples
git clone https://github.com/juglans-ai/juglans-examples.git
cd juglans-examples

# Run a basic example
juglans basic-chat.jg --input '{"message": "Hello!"}'

# Run an example with configuration
juglans rag-pipeline.jg --input '{"query": "What is Juglans?"}' --config juglans.toml
```

## Example Structure

Each example contains:

```
example-name/
├── workflow.jg      # Main workflow
├── prompts/             # Prompt templates
│   └── *.jgprompt
├── agents/              # Agent definitions
│   └── *.jgagent
├── README.md            # Documentation
└── test-input.json      # Test input
```
