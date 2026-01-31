# Juglans Design Patterns

## Agent & Workflow 设计模式

### 1. Pure Agents vs Workflow-bound Agents

**Pure Agent（纯净 Agent）**

- 不绑定 workflow（无 `workflow:` 字段）
- 专门用于 workflow 内部调用
- 职责单一，配置简洁
- 示例：`examples/pure-agents/deepseek.jgagent`

**Workflow-bound Agent（工作流绑定 Agent）**

- 绑定了 workflow（有 `workflow:` 字段）
- 作为独立入口运行
- 适合复杂的多步骤任务
- 示例：`examples/test.jgagent`

### 2. 推荐的使用模式

```
Entry Agent (test.jgagent)
  ├─ workflow: test.jgflow
  │
  └─ test.jgflow
      ├─ prompts: [test.jgprompt]
      ├─ agents: [test-worker.jgagent]  ← Pure Agent
      │
      └─ nodes use agent="test-worker"
```

**为什么 workflow 中应该使用 pure agents？**

1. **避免递归调用**

   - 如果 workflow A 使用 agent B，而 agent B 又绑定了 workflow C
   - 虽然当前实现不会递归执行（Chat tool 只读取 agent 配置）
   - 但这种嵌套关系会造成理解和维护困难
2. **依赖关系清晰**

   - workflow 的 `agents:` 列表明确声明了所有依赖
   - 避免隐式依赖（如之前 test.jgflow 使用 "test" agent 但未声明）
3. **职责分离**

   - Entry agent：定义入口点和绑定顶层 workflow
   - Pure agent：提供 LLM 能力，不关心流程控制
   - Workflow：编排任务流程

### 3. 递归防护机制

**当前实现的防护：**

在 `src/builtins/ai.rs:186-214`，Chat tool 使用 agent 时：

```rust
if let Some(local_res) = self.agent_registry.get(agent_slug_str) {
    // 只读取 agent 的配置字段
    json!({
        "slug": local_res.slug,
        "model": local_res.model,
        "system_prompt": resolved_sys_prompt,
        "temperature": local_res.temperature,
    })
    // 注意：agent.workflow 字段被忽略，不会触发嵌套执行
}
```

**这意味着：**

- 即使在 workflow 中使用了带 workflow 的 agent
- Chat tool 只会使用其 model 和 system_prompt
- **不会递归执行该 agent 的 workflow**

**但仍然推荐使用 pure agents：**

- 虽然技术上不会递归，但语义上容易混淆
- 代码审查者可能认为会触发嵌套执行
- 依赖关系不够明确

### 4. 示例对比

❌ **不推荐**（虽然可以工作）：

```yaml
# bad-example.jgflow
prompts: ["./test.jgprompt"]
# agents: []  ← 没有声明依赖

[node1]: chat(
  agent="test",  ← 使用了绑定 workflow 的 agent
  message="hello"
)
```

✅ **推荐**：

```yaml
e# good-example.jgflow
prompts: ["./test.jgprompt"]
agents: ["./agents/test-worker.jgagent"]  ← 明确声明

[node1]: chat(
  agent="test-worker",  ← 使用 pure agent
  message="hello"
)
```

### 5. 最佳实践总结

1. **命名约定**

   - Entry agent: `xxx.jgagent`（如 `test.jgagent`）
   - Pure agent: `xxx-worker.jgagent` 或 `xxx-assistant.jgagent`
   - Workflow: 与 entry agent 同名（如 `test.jgflow`）
2. **目录结构**

   ```
   examples/
   ├── test.jgagent           # Entry agent with workflow
   ├── test.jgflow            # Workflow definition
   ├── test.jgprompt          # Prompt template
   ├── agents/
   │   └── test-worker.jgagent  # Pure agent for workflow
   └── pure-agents/
       └── deepseek.jgagent   # Standalone pure agent
   ```
3. **开发流程**

   - 先创建 pure agent（定义 LLM 能力）
   - 再创建 workflow（编排任务流程）
   - 最后创建 entry agent（绑定 workflow）
4. **调试技巧**

   - Pure agent 可以独立测试：`juglans agents/test-worker.jgagent`
   - Workflow 需要通过 entry agent 运行：`juglans test.jgagent`
