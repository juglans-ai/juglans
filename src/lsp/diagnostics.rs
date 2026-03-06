use tower_lsp::lsp_types::*;

use crate::core::parser::GraphParser;
use crate::core::validator::{ValidationSeverity, WorkflowValidator};

/// Compute LSP diagnostics from source content
pub fn compute_diagnostics(content: &str) -> Vec<Diagnostic> {
    let mut diagnostics = Vec::new();

    // 1. Parse
    let graph = match GraphParser::parse(content) {
        Ok(g) => g,
        Err(e) => {
            let (line, col) = extract_pest_position(&e.to_string());
            diagnostics.push(Diagnostic {
                range: Range::new(Position::new(line, col), Position::new(line, col + 1)),
                severity: Some(DiagnosticSeverity::ERROR),
                code: Some(NumberOrString::String("parse".into())),
                source: Some("juglans".into()),
                message: e.to_string(),
                ..Default::default()
            });
            return diagnostics;
        }
    };

    // 2. Validate
    let result = WorkflowValidator::validate(&graph);

    for issue in result.errors.iter().chain(result.warnings.iter()) {
        let severity = match issue.severity {
            ValidationSeverity::Error => DiagnosticSeverity::ERROR,
            ValidationSeverity::Warning => DiagnosticSeverity::WARNING,
        };

        let range = if let Some(ref node_id) = issue.node_id {
            find_node_range(content, node_id)
        } else {
            Range::new(Position::new(0, 0), Position::new(0, 0))
        };

        diagnostics.push(Diagnostic {
            range,
            severity: Some(severity),
            code: Some(NumberOrString::String(issue.code.clone())),
            source: Some("juglans".into()),
            message: issue.message.clone(),
            ..Default::default()
        });
    }

    diagnostics
}

/// Find `[node_id]` definition in source, return its Range
fn find_node_range(content: &str, node_id: &str) -> Range {
    let def_pattern = format!("[{}]", node_id);
    for (line_num, line) in content.lines().enumerate() {
        if let Some(col) = line.find(&def_pattern) {
            return Range::new(
                Position::new(line_num as u32, col as u32),
                Position::new(line_num as u32, (col + def_pattern.len()) as u32),
            );
        }
    }
    Range::new(Position::new(0, 0), Position::new(0, 0))
}

/// Extract line/col from pest error string like " --> 5:12"
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
