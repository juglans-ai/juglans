# Juglans vs Other Tools

## Comparison Matrix

| Dimension | Juglans | Python Script | LangChain | Airflow | Terraform |
|-----------|---------|---------------|-----------|---------|-----------|
| **Abstraction** | Workflow DAG DSL | Imperative code | Chain abstractions | Task DAG | Declarative IaC |
| **Control Flow** | Declarative edges, switch, conditionals | if/else/for | Chain routing | DAG scheduling | Dependency graph |
| **Composition** | Flow imports, function defs, libs | Module imports | Chain nesting | SubDAGs | Modules |
| **AI Integration** | First-class (chat, agent, prompt) | Library calls | First-class | Operator wrappers | N/A |
| **Tool System** | Builtin + MCP + Python + client bridge | Any library | Tool classes | Operators/Hooks | Providers |
| **Learning Curve** | Low -- 3 concepts (node, edge, variable) | Depends on skill | Medium-high | High | Medium |
| **Type Safety** | Static validation (`juglans check`) | Runtime errors | Runtime errors | Runtime errors | Plan/validate |
| **Deployment** | Single binary, Docker, cron | Python env | Python env | Scheduler cluster | CLI + state |
| **Best For** | AI agent workflows, API orchestration | Quick prototypes, data science | LLM-heavy chains | Batch data pipelines | Infrastructure |

## What Juglans Does Differently

**Declarative over imperative.** Define *what* happens, not *how* to wire it:

```juglans
entry: [input]
exit: [respond]

[input]: set_context(query=$input.question)
[search]: fetch(url="https://api.example.com/search?q=" + $ctx.query)
[respond]: chat(agent="assistant", message=$search)

[input] -> [search] -> [respond]
```

The equivalent Python script requires explicit function definitions, error handling, HTTP client setup, and result passing between steps. In LangChain, you would additionally need chain class instantiation and callback wiring.

**Static analysis before execution.** `juglans check` validates the entire DAG before running -- missing nodes, broken edges, unreachable paths, unused variables. Python and LangChain only discover these at runtime.

**Built-in routing without code.**  Conditional edges and switch routing are part of the DSL, not bolted-on logic:

```juglans
entry: [classify]
exit: [answer, execute, fallback]

[classify]: chat(agent="router", message=$input.query, format="json")

[answer]: chat(agent="qa", message=$input.query)
[execute]: chat(agent="coder", message=$input.query)
[fallback]: chat(agent="general", message=$input.query)

[classify] -> switch $output.intent {
    "question": [answer]
    "task": [execute]
    default: [fallback]
}
```

In Airflow, this requires a BranchPythonOperator with a custom callable. In LangChain, you need a RouterChain or custom RunnableRouter.

## When to Choose Juglans

**Choose Juglans when:**

- You need AI agent orchestration with multi-step workflows
- You want static validation of workflow graphs before deployment
- You need to compose workflows from reusable sub-workflows and function definitions
- You want a single binary with no runtime dependencies (no Python env, no JVM)
- You need SSE streaming, bot adapters, or MCP tool integration out of the box

**Choose something else when:**

- **Python script** -- one-off data analysis or quick prototyping
- **LangChain** -- heavy experimentation with LLM internals (custom embeddings, vector stores)
- **Airflow** -- large-scale batch ETL with scheduling, retries, and cluster management
- **Terraform** -- infrastructure provisioning (not workflow orchestration)
