use tower_lsp::lsp_types::*;

use super::document::DocumentState;

/// Go-to-definition: resolve [node_ref] → definition line
pub fn goto_definition(
    doc: &DocumentState,
    uri: &Url,
    position: Position,
) -> Option<GotoDefinitionResponse> {
    let line_text = doc.content.lines().nth(position.line as usize)?;
    let col = position.character as usize;

    // Find [identifier] around cursor
    let target = extract_bracket_id(line_text, col)?;

    // Search for definition: [target]: or [target(...)]
    for (line_num, line) in doc.content.lines().enumerate() {
        let trimmed = line.trim();
        // Match [target]: ... (node definition)
        let def_pat = format!("[{}]:", target);
        if let Some(c) = trimmed.find(&def_pat) {
            let abs_col = line.find(&def_pat).unwrap_or(c);
            return Some(GotoDefinitionResponse::Scalar(Location {
                uri: uri.clone(),
                range: Range::new(
                    Position::new(line_num as u32, abs_col as u32),
                    Position::new(line_num as u32, (abs_col + def_pat.len()) as u32),
                ),
            }));
        }
        // Match function def: [target(params)]:
        let fn_prefix = format!("[{}(", target);
        if let Some(c) = trimmed.find(&fn_prefix) {
            let abs_col = line.find(&fn_prefix).unwrap_or(c);
            return Some(GotoDefinitionResponse::Scalar(Location {
                uri: uri.clone(),
                range: Range::new(
                    Position::new(line_num as u32, abs_col as u32),
                    Position::new(line_num as u32, (abs_col + fn_prefix.len()) as u32),
                ),
            }));
        }
    }

    None
}

/// Hover: show info about the symbol under cursor
pub fn hover(doc: &DocumentState, position: Position) -> Option<Hover> {
    let line_text = doc.content.lines().nth(position.line as usize)?;
    let col = position.character as usize;

    // Hover on [node_id]
    if let Some(node_id) = extract_bracket_id(line_text, col) {
        if let Some(ref graph) = doc.graph {
            if let Some(idx) = graph.node_map.get(&node_id) {
                let node = &graph.graph[*idx];
                let type_str = match &node.node_type {
                    crate::core::graph::NodeType::Task(action) => {
                        format!("**Task**: `{}`", action.name)
                    }
                    crate::core::graph::NodeType::Foreach { .. } => "**Foreach** loop".into(),
                    crate::core::graph::NodeType::Loop { .. } => "**While** loop".into(),
                    crate::core::graph::NodeType::Literal(v) => {
                        format!("**Literal**: `{}`", v)
                    }
                    _ => "Node".into(),
                };
                return Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: format!("### [{}]\n{}", node_id, type_str),
                    }),
                    range: None,
                });
            }
            // Check functions
            if let Some(func) = graph.functions.get(&node_id) {
                let params = func.params.join(", ");
                return Some(Hover {
                    contents: HoverContents::Markup(MarkupContent {
                        kind: MarkupKind::Markdown,
                        value: format!("### Function `{}`\nParams: `({})`", node_id, params),
                    }),
                    range: None,
                });
            }
        }
    }

    // Hover on $variable
    if let Some(var) = extract_variable(line_text, col) {
        let desc = match var.as_str() {
            "$input" => "Input data passed to the workflow",
            "$output" => "Output from the previous node",
            "$ctx" => "Workflow context (set via set_context)",
            "$reply" => "Agent reply metadata (output, status)",
            "$error" => "Error information from on_error edge",
            _ => "Variable",
        };
        return Some(Hover {
            contents: HoverContents::Markup(MarkupContent {
                kind: MarkupKind::Markdown,
                value: format!("`{}`\n\n{}", var, desc),
            }),
            range: None,
        });
    }

    // Hover on tool name (word before `(`)
    if let Some(tool_name) = extract_tool_name(line_text, col) {
        let desc = tool_description(&tool_name);
        if let Some(d) = desc {
            return Some(Hover {
                contents: HoverContents::Markup(MarkupContent {
                    kind: MarkupKind::Markdown,
                    value: format!("### `{}`\n{}", tool_name, d),
                }),
                range: None,
            });
        }
    }

    None
}

/// Extract identifier inside [...] at cursor position
fn extract_bracket_id(line: &str, col: usize) -> Option<String> {
    let bytes = line.as_bytes();
    if col >= bytes.len() {
        return None;
    }

    // Search left for [
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
        // Also handle if cursor is right at [
        if bytes[0] != b'[' {
            return None;
        }
    }

    // Search right for ]
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
    // Strip function params: "name(a, b)" -> "name"
    let id = content.split('(').next().unwrap_or(content).trim();

    if id.is_empty() {
        return None;
    }
    Some(id.to_string())
}

/// Extract $variable at cursor position
fn extract_variable(line: &str, col: usize) -> Option<String> {
    let bytes = line.as_bytes();
    if col >= bytes.len() {
        return None;
    }

    // Search left for $
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

    // Extend right
    let mut end = col + 1;
    while end < bytes.len()
        && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_' || bytes[end] == b'.')
    {
        end += 1;
    }

    let var = &line[start..end];
    // Return base variable (e.g. $ctx from $ctx.some.path)
    let base = var.split('.').next().unwrap_or(var);
    Some(base.to_string())
}

/// Extract tool name at cursor: the word before `(`
fn extract_tool_name(line: &str, col: usize) -> Option<String> {
    let trimmed = line.trim();
    // Find ]: tool_name(...)
    if let Some(after) = trimmed
        .strip_prefix(|_: char| true)
        .and(None::<&str>)
        .or_else(|| {
            if let Some(idx) = trimmed.find("]:") {
                Some(trimmed[idx + 2..].trim_start())
            } else {
                None
            }
        })
    {
        // Extract tool name (word before parenthesis)
        if let Some(paren) = after.find('(') {
            let name = after[..paren].trim();
            if !name.is_empty() && col >= line.find(name).unwrap_or(usize::MAX) {
                return Some(name.to_string());
            }
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
        "set_context" | "set" => Some("Set a context variable.\n\nParams: `key`, `value`"),
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
