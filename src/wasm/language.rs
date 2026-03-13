// src/wasm/language.rs — LSP intelligence for WASM (Monaco Editor integration)
//
// Pure computation versions of diagnostics, completions, hover, goto-definition.
// No tower-lsp dependency — returns simple Serialize structs for JS consumption.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::core::graph::NodeType;
use crate::core::parser::GraphParser;
use crate::core::validator::{ValidationSeverity, WorkflowValidator};

// ================================================================
// WASM-native types (replaces tower-lsp types)
// ================================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Range {
    pub start: Position,
    pub end: Position,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Diagnostic {
    pub range: Range,
    pub severity: String,
    pub code: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompletionItem {
    pub label: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub insert_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub insert_text_format: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HoverResult {
    pub contents: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocationResult {
    pub range: Range,
}

// ================================================================
// Diagnostics
// ================================================================

pub fn compute_diagnostics(content: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    let graph = match GraphParser::parse(content) {
        Ok(g) => g,
        Err(e) => {
            let (line, col) = extract_pest_position(&e.to_string());
            diagnostics.push(Diagnostic {
                range: Range {
                    start: Position {
                        line,
                        character: col,
                    },
                    end: Position {
                        line,
                        character: col + 1,
                    },
                },
                severity: "error".into(),
                code: "parse".into(),
                message: e.to_string(),
            });
            return diagnostics;
        }
    };

    let result = WorkflowValidator::validate(&graph);

    for issue in result.errors.iter().chain(result.warnings.iter()) {
        let severity = match issue.severity {
            ValidationSeverity::Error => "error",
            ValidationSeverity::Warning => "warning",
        };

        let range = if let Some(ref node_id) = issue.node_id {
            find_node_range(content, node_id)
        } else {
            Range {
                start: Position {
                    line: 0,
                    character: 0,
                },
                end: Position {
                    line: 0,
                    character: 0,
                },
            }
        };

        diagnostics.push(Diagnostic {
            range,
            severity: severity.into(),
            code: issue.code.clone(),
            message: issue.message.clone(),
        });
    }

    diagnostics
}

fn find_node_range(content: &str, node_id: &str) -> Range {
    let def_pattern = format!("[{}]", node_id);
    for (line_num, line) in content.lines().enumerate() {
        if let Some(col) = line.find(&def_pattern) {
            return Range {
                start: Position {
                    line: line_num as u32,
                    character: col as u32,
                },
                end: Position {
                    line: line_num as u32,
                    character: (col + def_pattern.len()) as u32,
                },
            };
        }
    }
    Range {
        start: Position {
            line: 0,
            character: 0,
        },
        end: Position {
            line: 0,
            character: 0,
        },
    }
}

fn extract_pest_position(msg: &str) -> (u32, u32) {
    if let Some(pos) = msg.find(" --> ") {
        let after = &msg[pos + 5..];
        let parts: Vec<&str> = after.splitn(3, ':').collect();
        if parts.len() >= 2 {
            let line = parts[0].trim().parse::<u32>().unwrap_or(1);
            let col = parts[1]
                .split(|c: char| !c.is_ascii_digit())
                .next()
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(1);
            return (line.saturating_sub(1), col.saturating_sub(1));
        }
    }
    (0, 0)
}

// ================================================================
// Completions
// ================================================================

const METADATA_KEYS: &[(&str, &str)] = &[
    ("slug", "Unique identifier"),
    ("name", "Display name"),
    ("version", "Semantic version"),
    ("source", "Source URL or path"),
    ("author", "Author name"),
    ("description", "Workflow description"),
    ("entry", "Entry node ID"),
    ("exit", "Exit node ID(s)"),
    ("libs", "Library imports"),
    ("flows", "Flow imports"),
    ("prompts", "Prompt file patterns"),
    ("agents", "Agent file patterns"),
    ("tools", "Tool file patterns"),
    ("python", "Python module imports"),
    ("is_public", "Public visibility flag"),
];

const BUILTIN_TOOLS: &[(&str, &str)] = &[
    ("chat", "AI chat completion"),
    ("p", "Render prompt template"),
    ("fetch", "HTTP request"),
    ("fetch_url", "Fetch URL content"),
    ("notify", "Send notification"),
    ("print", "Print to output"),
    ("reply", "Send reply message"),
    ("return", "Return value from function"),
    ("timer", "Delay execution"),
    ("serve", "HTTP server entry point"),
    ("response", "HTTP response builder"),
    ("assert", "Test assertion"),
    ("config", "Test configuration"),
    ("read_file", "Read file contents"),
    ("write_file", "Write file contents"),
    ("edit_file", "Edit file contents"),
    ("glob", "File pattern matching"),
    ("grep", "Search file contents"),
    ("bash", "Execute shell command"),
    ("sh", "Execute shell command (alias)"),
    ("execute_workflow", "Run sub-workflow"),
    ("memory_search", "Search memory store"),
    ("history", "Chat history"),
    ("vector_create_space", "Create vector space"),
    ("vector_upsert", "Upsert vectors"),
    ("vector_search", "Search vectors"),
    ("vector_list_spaces", "List vector spaces"),
    ("vector_delete_space", "Delete vector space"),
    ("vector_delete", "Delete vectors by ID"),
    ("feishu_webhook", "Send Feishu webhook"),
    ("db_connect", "Connect to database"),
    ("db_disconnect", "Disconnect database"),
    ("db_query", "Raw SQL query"),
    ("db_exec", "Raw SQL execution"),
    ("db_find", "Find records"),
    ("db_find_one", "Find single record"),
    ("db_create", "Create record"),
    ("db_create_many", "Create multiple records"),
    ("db_upsert", "Upsert record"),
    ("db_update", "Update records"),
    ("db_delete", "Delete records"),
    ("db_count", "Count records"),
    ("db_aggregate", "Aggregate query"),
    ("db_begin", "Begin transaction"),
    ("db_commit", "Commit transaction"),
    ("db_rollback", "Rollback transaction"),
    ("db_create_table", "Create table"),
    ("db_drop_table", "Drop table"),
    ("db_alter_table", "Alter table"),
    ("db_tables", "List tables"),
    ("db_columns", "List table columns"),
];

pub fn completions(source: &str, line: u32, col: u32) -> Vec<CompletionItem> {
    let line_text = source.lines().nth(line as usize).unwrap_or("");
    let col = (col as usize).min(line_text.len());
    let prefix = &line_text[..col];
    let trimmed = prefix.trim_start();

    let mut items = Vec::new();

    // Try to parse for graph info (functions, nodes)
    let graph = GraphParser::parse(source).ok();

    // 1. Metadata keys
    if (trimmed.is_empty() || trimmed.chars().all(|c| c.is_alphanumeric() || c == '_'))
        && !line_text.contains('[')
        && is_metadata_region(source, line)
    {
        for (key, desc) in METADATA_KEYS {
            items.push(CompletionItem {
                label: format!("{}: ", key),
                kind: "property".into(),
                detail: Some(desc.to_string()),
                insert_text: Some(format!("{}: ", key)),
                insert_text_format: None,
            });
        }
    }

    // 2. Tool names — after [node]:
    if trimmed.contains("]:") {
        let after_colon = trimmed
            .split_once("]:")
            .map(|x| x.1)
            .unwrap_or("")
            .trim_start();
        if after_colon.is_empty() || (!after_colon.contains('(') && !after_colon.contains("->")) {
            for (tool, desc) in BUILTIN_TOOLS {
                items.push(CompletionItem {
                    label: tool.to_string(),
                    kind: "function".into(),
                    detail: Some(desc.to_string()),
                    insert_text: Some(format!("{}($1)", tool)),
                    insert_text_format: Some("snippet".into()),
                });
            }
            if let Some(ref g) = graph {
                for fname in g.functions.keys() {
                    items.push(CompletionItem {
                        label: fname.clone(),
                        kind: "function".into(),
                        detail: Some("User function".into()),
                        insert_text: None,
                        insert_text_format: None,
                    });
                }
            }
        }
    }

    // 3. Node references
    if prefix.ends_with("-> [") || (trimmed.starts_with('[') && !trimmed.contains("]:")) {
        if let Some(ref g) = graph {
            for node_id in g.node_map.keys() {
                items.push(CompletionItem {
                    label: node_id.clone(),
                    kind: "reference".into(),
                    detail: Some("Node".into()),
                    insert_text: None,
                    insert_text_format: None,
                });
            }
        }
    }

    // 4. Variables
    if prefix.contains('$') {
        let base_vars = [
            ("$input", "Input data"),
            ("$output", "Previous node output"),
            ("$ctx", "Workflow context"),
            ("$reply", "Agent reply metadata"),
            ("$error", "Error information"),
        ];
        for (var, desc) in base_vars {
            items.push(CompletionItem {
                label: var.to_string(),
                kind: "variable".into(),
                detail: Some(desc.to_string()),
                insert_text: None,
                insert_text_format: None,
            });
        }
    }

    // 5. Keywords + snippets
    if trimmed.is_empty() {
        let keywords = [
            ("foreach", "Iterate over collection"),
            ("while", "Loop with condition"),
            ("switch", "Multi-branch routing"),
            ("class", "Class definition"),
            ("struct", "Struct definition"),
        ];
        for (kw, desc) in keywords {
            items.push(CompletionItem {
                label: kw.to_string(),
                kind: "keyword".into(),
                detail: Some(desc.to_string()),
                insert_text: None,
                insert_text_format: None,
            });
        }
        items.push(CompletionItem {
            label: "node".into(),
            kind: "snippet".into(),
            detail: Some("Node definition".into()),
            insert_text: Some("[${1:name}]: ${2:tool}(${3:params})".into()),
            insert_text_format: Some("snippet".into()),
        });
        items.push(CompletionItem {
            label: "edge".into(),
            kind: "snippet".into(),
            detail: Some("Edge definition".into()),
            insert_text: Some("[${1:from}] -> [${2:to}]".into()),
            insert_text_format: Some("snippet".into()),
        });
    }

    items
}

fn is_metadata_region(content: &str, line: u32) -> bool {
    for (i, l) in content.lines().enumerate() {
        if i >= line as usize {
            return true;
        }
        let t = l.trim();
        if t.starts_with('[') && t.contains("]:") {
            return false;
        }
    }
    true
}

// ================================================================
// Hover
// ================================================================

pub fn hover(source: &str, line: u32, col: u32) -> Option<HoverResult> {
    let line_text = source.lines().nth(line as usize)?;
    let col = col as usize;
    let graph = GraphParser::parse(source).ok();

    // Hover on [node_id]
    if let Some(node_id) = extract_bracket_id(line_text, col) {
        if let Some(ref g) = graph {
            if let Some(idx) = g.node_map.get(&node_id) {
                let node = &g.graph[*idx];
                let type_str = match &node.node_type {
                    NodeType::Task(action) => format!("**Task**: `{}`", action.name),
                    NodeType::Foreach { .. } => "**Foreach** loop".into(),
                    NodeType::Loop { .. } => "**While** loop".into(),
                    NodeType::Literal(v) => format!("**Literal**: `{}`", v),
                    _ => "Node".into(),
                };
                return Some(HoverResult {
                    contents: format!("### [{}]\n{}", node_id, type_str),
                });
            }
            if let Some(func) = g.functions.get(&node_id) {
                let params = func.params.join(", ");
                return Some(HoverResult {
                    contents: format!("### Function `{}`\nParams: `({})`", node_id, params),
                });
            }
        }
    }

    // Hover on $variable
    if let Some(var) = extract_variable(line_text, col) {
        let desc = match var.as_str() {
            "$input" => "Input data passed to the workflow",
            "$output" => "Output from the previous node",
            "$ctx" => "Workflow context variables",
            "$reply" => "Agent reply metadata (output, status)",
            "$error" => "Error information from on_error edge",
            _ => "Variable",
        };
        return Some(HoverResult {
            contents: format!("`{}`\n\n{}", var, desc),
        });
    }

    // Hover on tool name
    if let Some(tool_name) = extract_tool_name(line_text, col) {
        if let Some(d) = tool_description(&tool_name) {
            return Some(HoverResult {
                contents: format!("### `{}`\n{}", tool_name, d),
            });
        }
    }

    None
}

// ================================================================
// Go-to-Definition
// ================================================================

pub fn goto_definition(source: &str, line: u32, col: u32) -> Option<LocationResult> {
    let line_text = source.lines().nth(line as usize)?;
    let col = col as usize;
    let target = extract_bracket_id(line_text, col)?;

    for (line_num, src_line) in source.lines().enumerate() {
        let trimmed = src_line.trim();
        // Match [target]: ... (node definition)
        let def_pat = format!("[{}]:", target);
        if let Some(c) = trimmed.find(&def_pat) {
            let abs_col = src_line.find(&def_pat).unwrap_or(c);
            return Some(LocationResult {
                range: Range {
                    start: Position {
                        line: line_num as u32,
                        character: abs_col as u32,
                    },
                    end: Position {
                        line: line_num as u32,
                        character: (abs_col + def_pat.len()) as u32,
                    },
                },
            });
        }
        // Match function def: [target(params)]:
        let fn_prefix = format!("[{}(", target);
        if let Some(c) = trimmed.find(&fn_prefix) {
            let abs_col = src_line.find(&fn_prefix).unwrap_or(c);
            return Some(LocationResult {
                range: Range {
                    start: Position {
                        line: line_num as u32,
                        character: abs_col as u32,
                    },
                    end: Position {
                        line: line_num as u32,
                        character: (abs_col + fn_prefix.len()) as u32,
                    },
                },
            });
        }
    }

    None
}

// ================================================================
// Pure text helpers (ported from lsp/navigation.rs)
// ================================================================

fn extract_bracket_id(line: &str, col: usize) -> Option<String> {
    let bytes = line.as_bytes();
    if col >= bytes.len() {
        return None;
    }
    let mut start = col;
    while start > 0 {
        if bytes[start - 1] == b'[' {
            break;
        }
        if bytes[start - 1] == b']' {
            return None;
        }
        start -= 1;
    }
    if start == 0 && bytes.first() != Some(&b'[') {
        if bytes[0] != b'[' {
            return None;
        }
    }
    let mut end = col;
    while end < bytes.len() {
        if bytes[end] == b']' {
            break;
        }
        if bytes[end] == b'[' && end != start.saturating_sub(1) {
            return None;
        }
        end += 1;
    }
    if end >= bytes.len() {
        return None;
    }
    let content = &line[start..end];
    let id = content.split('(').next().unwrap_or(content).trim();
    if id.is_empty() {
        return None;
    }
    Some(id.to_string())
}

fn extract_variable(line: &str, col: usize) -> Option<String> {
    let bytes = line.as_bytes();
    if col >= bytes.len() {
        return None;
    }
    let mut start = col;
    while start > 0 {
        if bytes[start - 1] == b'$' {
            start -= 1;
            break;
        }
        if !bytes[start - 1].is_ascii_alphanumeric()
            && bytes[start - 1] != b'_'
            && bytes[start - 1] != b'.'
        {
            return None;
        }
        start -= 1;
    }
    if bytes[start] != b'$' {
        return None;
    }
    let mut end = col + 1;
    while end < bytes.len()
        && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_' || bytes[end] == b'.')
    {
        end += 1;
    }
    let var = &line[start..end];
    let base = var.split('.').next().unwrap_or(var);
    Some(base.to_string())
}

fn extract_tool_name(line: &str, col: usize) -> Option<String> {
    let trimmed = line.trim();
    let after = if let Some(idx) = trimmed.find("]:") {
        trimmed[idx + 2..].trim_start()
    } else {
        return None;
    };
    if let Some(paren) = after.find('(') {
        let name = after[..paren].trim();
        if !name.is_empty() && col >= line.find(name).unwrap_or(usize::MAX) {
            return Some(name.to_string());
        }
    }
    None
}

fn tool_description(name: &str) -> Option<&'static str> {
    match name {
        "chat" => Some("AI chat completion.\n\nParams: `model`, `prompt`, `system`, `state`, `agent`, `tools`, `temperature`"),
        "p" => Some("Render a prompt template.\n\nParams: `prompt` (slug or path)"),
        "fetch" => Some("HTTP request.\n\nParams: `url`, `method`, `headers`, `body`"),
        "notify" => Some("Send notification.\n\nParams: `message`"),
        "print" => Some("Print value to output.\n\nParams: (value expression)"),
        "reply" => Some("Send reply to client.\n\nParams: `message`, `type`"),
        "set" => Some("Set a context variable.\n\nParams: `key`, `value`"),
        "serve" => Some("Mark node as HTTP entry point.\n\nParams: `method`, `path`"),
        "response" => Some("Build HTTP response.\n\nParams: `status`, `body`, `headers`"),
        "assert" => Some("Test assertion.\n\nParams: `contains`, `eq`, `true`"),
        "bash" | "sh" => Some("Execute shell command.\n\nParams: `command`"),
        "read_file" => Some("Read file contents.\n\nParams: `path`"),
        "write_file" => Some("Write file contents.\n\nParams: `path`, `content`"),
        _ if name.starts_with("db_") => Some("Database ORM operation"),
        _ if name.starts_with("vector_") => Some("Vector store operation"),
        _ => None,
    }
}
