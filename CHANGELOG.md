# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.4] - 2026-01-31

### Added

#### Tool Definition Files
- **Tool definition file support** - Store and reuse tool configurations in JSON files
  - Create `tools/*.json` files with OpenAI Function Calling format
  - Import tools in workflows: `tools: ["./tools/*.json"]`
  - Three reference methods:
    - Inline JSON: `tools: [{...}]` (backward compatible)
    - Single reference: `tools: "web-tools"`
    - Multiple references: `tools: ["web-tools", "data-tools"]`
  - Agent default tools: Configure in `.jgagent` files
  - Automatic tool merging and deduplication
  - Priority: Workflow inline > Workflow reference > Agent default

#### Core Infrastructure
- `ToolResource` data structure for tool definitions
- `ToolLoader` for loading tools from JSON files with glob support
- `ToolRegistry` for tool registration, lookup, and merging
- Workflow parser support for `tools:` field in metadata
- Runtime tool reference resolution in `chat()` builtin

#### Examples & Documentation
- Example tool files: `web-tools.json`, `data-tools.json`
- Complete tool usage example workflow
- Agent with default tools example
- Comprehensive tools guide: `docs/guide/tools.md`
  - File format specification
  - Usage patterns and best practices
  - Error handling and debugging

### Fixed

#### MCP Documentation Corrections
- **MCP configuration format** - Fixed documentation to match actual implementation
  - Corrected to HTTP/JSON-RPC connection model (not process spawning)
  - Updated config format to `[[mcp_servers]]` with `base_url`
  - Removed incorrect `command`, `args`, `env` examples
  - Added proper HTTP server setup instructions

- **MCP tool naming** - Fixed tool invocation format
  - Corrected from `mcp_namespace_tool` to `namespace.tool`
  - Updated all examples to use dot notation
  - Clarified namespace resolution (alias > name)

### Changed

- Updated workflow execution to load tools from patterns
- Enhanced AI builtin to resolve tool references at runtime
- Improved error messages for missing tool resources

### Technical Details

**New Files:**
- `src/core/tool_loader.rs` - Tool file loading and validation
- `src/services/tool_registry.rs` - Tool registration and merging
- `examples/tools/*.json` - Example tool definitions
- `docs/guide/tools.md` - Complete documentation

**Modified Files:**
- `src/core/agent.pest` - Added `list` support for tools field
- `src/core/agent_parser.rs` - Parse three tool reference formats
- `src/core/jwl.pest` - Added `tools` to workflow metadata
- `src/core/parser.rs` - Parse tool patterns in workflows
- `src/core/graph.rs` - Added `tool_patterns` field
- `src/core/executor.rs` - Load and provide tool registry
- `src/builtins/ai.rs` - Resolve tool references at runtime
- `src/builtins/mod.rs` - Expose executor for tool access

**Tests:**
- 6 new unit tests for tool loading, registry, and deduplication
- All tests passing

**Lines of Code:**
- 1000+ lines added across 8 new files
- Complete test coverage for core functionality

## [0.1.3] - 2026-01-30

### Added
- Conditional branch OR semantics with unreachable node detection
- Context save/restore for nested workflow execution
- Tools configuration in agent definitions (JSON array support)

### Fixed
- Workflow deadlock on conditional branch convergence
- Context pollution in nested workflows
- Agent tools priority (workflow > agent default)

## [0.1.2] - 2026-01-29

### Added
- Agent workflow association
- Stateless execution mode
- Multi-turn conversation support

## [0.1.1] - 2026-01-28

### Added
- Initial workflow engine
- Agent and prompt management
- Basic builtins (chat, notify, etc.)

[0.1.4]: https://github.com/juglans-ai/juglans/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/juglans-ai/juglans/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/juglans-ai/juglans/compare/v0.1.1...v0.1.4
[0.1.1]: https://github.com/juglans-ai/juglans/releases/tag/v0.1.1
