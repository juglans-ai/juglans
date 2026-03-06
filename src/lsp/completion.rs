use tower_lsp::lsp_types::*;

use super::document::DocumentState;

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
    ("set_context", "Set context variable"),
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

pub fn completions(doc: &DocumentState, position: Position) -> Vec<CompletionItem> {
    let line_text = doc
        .content
        .lines()
        .nth(position.line as usize)
        .unwrap_or("");
    let col = (position.character as usize).min(line_text.len());
    let prefix = &line_text[..col];
    let trimmed = prefix.trim_start();

    let mut items = Vec::new();

    // 1. Metadata keys — at line start in metadata region
    if (trimmed.is_empty() || trimmed.chars().all(|c| c.is_alphanumeric() || c == '_'))
        && !line_text.contains('[')
        && is_metadata_region(&doc.content, position.line)
    {
        for (key, desc) in METADATA_KEYS {
            items.push(CompletionItem {
                label: format!("{}: ", key),
                kind: Some(CompletionItemKind::PROPERTY),
                detail: Some(desc.to_string()),
                insert_text: Some(format!("{}: ", key)),
                ..Default::default()
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
                    kind: Some(CompletionItemKind::FUNCTION),
                    detail: Some(desc.to_string()),
                    insert_text: Some(format!("{}($1)", tool)),
                    insert_text_format: Some(InsertTextFormat::SNIPPET),
                    ..Default::default()
                });
            }
            // User-defined functions
            if let Some(ref graph) = doc.graph {
                for fname in graph.functions.keys() {
                    items.push(CompletionItem {
                        label: fname.clone(),
                        kind: Some(CompletionItemKind::FUNCTION),
                        detail: Some("User function".into()),
                        ..Default::default()
                    });
                }
            }
        }
    }

    // 3. Node references — after -> [ or standalone [
    if prefix.ends_with("-> [") || (trimmed.starts_with('[') && !trimmed.contains("]:")) {
        if let Some(ref graph) = doc.graph {
            for node_id in graph.node_map.keys() {
                items.push(CompletionItem {
                    label: node_id.clone(),
                    kind: Some(CompletionItemKind::REFERENCE),
                    detail: Some("Node".into()),
                    ..Default::default()
                });
            }
        }
    }

    // 4. Variables — after $
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
                kind: Some(CompletionItemKind::VARIABLE),
                detail: Some(desc.to_string()),
                ..Default::default()
            });
        }
    }

    // 5. Keywords + snippets on empty line
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
                kind: Some(CompletionItemKind::KEYWORD),
                detail: Some(desc.to_string()),
                ..Default::default()
            });
        }

        // Snippets
        items.push(CompletionItem {
            label: "node".into(),
            kind: Some(CompletionItemKind::SNIPPET),
            detail: Some("Node definition".into()),
            insert_text: Some("[${1:name}]: ${2:tool}(${3:params})".into()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        });
        items.push(CompletionItem {
            label: "edge".into(),
            kind: Some(CompletionItemKind::SNIPPET),
            detail: Some("Edge definition".into()),
            insert_text: Some("[${1:from}] -> [${2:to}]".into()),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        });
    }

    items
}

/// True if cursor is before any node definition line
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
