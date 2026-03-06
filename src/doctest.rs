use anyhow::Result;
use std::path::{Path, PathBuf};

use crate::core::parser::GraphParser;

pub struct DocSnippet {
    pub file: PathBuf,
    pub line: usize,
    pub content: String,
    pub line_count: usize,
    pub ignore: bool,
}

pub struct DocTestResult {
    pub snippet: DocSnippet,
    pub passed: bool,
    pub error: Option<String>,
}

/// Extract ```juglans code blocks from a markdown file
pub fn extract_snippets(file: &Path) -> Result<Vec<DocSnippet>> {
    let content = std::fs::read_to_string(file)?;
    let mut snippets = Vec::new();
    let mut in_block = false;
    let mut ignore = false;
    let mut block_start = 0;
    let mut block_lines = Vec::new();

    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if !in_block {
            if trimmed.starts_with("```juglans") {
                in_block = true;
                ignore = trimmed.contains("ignore");
                block_start = i + 1; // 1-based
                block_lines.clear();
            }
        } else if trimmed == "```" {
            let snippet_content = block_lines.join("\n");
            snippets.push(DocSnippet {
                file: file.to_path_buf(),
                line: block_start,
                line_count: block_lines.len(),
                content: snippet_content,
                ignore,
            });
            in_block = false;
        } else {
            block_lines.push(line.to_string());
        }
    }

    Ok(snippets)
}

/// Validate a single snippet by attempting to parse it
pub fn validate_snippet(snippet: DocSnippet) -> DocTestResult {
    if snippet.ignore {
        return DocTestResult {
            snippet,
            passed: true,
            error: None,
        };
    }

    match GraphParser::parse(&snippet.content) {
        Ok(_) => DocTestResult {
            snippet,
            passed: true,
            error: None,
        },
        Err(e) => DocTestResult {
            snippet,
            passed: false,
            error: Some(e.to_string()),
        },
    }
}

/// Run doctest on a path (file or directory)
pub fn run_doctest(path: &Path, format: &str) -> Result<()> {
    let md_files = collect_markdown_files(path)?;

    if md_files.is_empty() {
        println!("No markdown files found in {}", path.display());
        return Ok(());
    }

    let mut total_passed = 0usize;
    let mut total_failed = 0usize;
    let mut total_ignored = 0usize;
    let mut total_files = 0usize;
    let mut all_results: Vec<DocTestResult> = Vec::new();

    for file in &md_files {
        let snippets = extract_snippets(file)?;
        if snippets.is_empty() {
            continue;
        }
        total_files += 1;

        let results: Vec<DocTestResult> = snippets.into_iter().map(validate_snippet).collect();

        for r in &results {
            if r.snippet.ignore {
                total_ignored += 1;
            } else if r.passed {
                total_passed += 1;
            } else {
                total_failed += 1;
            }
        }

        all_results.extend(results);
    }

    match format {
        "json" => print_json(
            &all_results,
            total_passed,
            total_failed,
            total_ignored,
            total_files,
        ),
        _ => print_text(
            &all_results,
            total_passed,
            total_failed,
            total_ignored,
            total_files,
        ),
    }

    if total_failed > 0 {
        std::process::exit(1);
    }

    Ok(())
}

fn collect_markdown_files(path: &Path) -> Result<Vec<PathBuf>> {
    if path.is_file() {
        return Ok(vec![path.to_path_buf()]);
    }

    let pattern = format!("{}/**/*.md", path.display());
    let mut files: Vec<PathBuf> = glob::glob(&pattern)?.filter_map(|e| e.ok()).collect();
    files.sort();
    Ok(files)
}

fn print_text(
    results: &[DocTestResult],
    passed: usize,
    failed: usize,
    ignored: usize,
    file_count: usize,
) {
    let mut current_file: Option<&Path> = None;

    for r in results {
        if current_file != Some(&r.snippet.file) {
            current_file = Some(&r.snippet.file);
            println!("\n{}", r.snippet.file.display());
        }

        if r.snippet.ignore {
            println!(
                "  \x1b[90m- line {} ({} lines) (ignored)\x1b[0m",
                r.snippet.line, r.snippet.line_count
            );
        } else if r.passed {
            println!(
                "  \x1b[32m✓\x1b[0m line {} ({} lines)",
                r.snippet.line, r.snippet.line_count
            );
        } else {
            println!(
                "  \x1b[31m✗\x1b[0m line {} ({} lines) — {}",
                r.snippet.line,
                r.snippet.line_count,
                r.error.as_deref().unwrap_or("unknown error")
            );
        }
    }

    let total = passed + failed + ignored;
    println!();
    if failed > 0 {
        println!(
            "\x1b[31mResults: {} passed, {} failed, {} ignored ({} snippets in {} files)\x1b[0m",
            passed, failed, ignored, total, file_count
        );
    } else {
        println!(
            "\x1b[32mResults: {} passed, {} failed, {} ignored ({} snippets in {} files)\x1b[0m",
            passed, failed, ignored, total, file_count
        );
    }
}

fn print_json(
    results: &[DocTestResult],
    passed: usize,
    failed: usize,
    ignored: usize,
    file_count: usize,
) {
    let snippets: Vec<serde_json::Value> = results
        .iter()
        .map(|r| {
            serde_json::json!({
                "file": r.snippet.file.display().to_string(),
                "line": r.snippet.line,
                "lines": r.snippet.line_count,
                "ignored": r.snippet.ignore,
                "passed": r.passed,
                "error": r.error,
            })
        })
        .collect();

    let output = serde_json::json!({
        "passed": passed,
        "failed": failed,
        "ignored": ignored,
        "total": passed + failed + ignored,
        "files": file_count,
        "snippets": snippets,
    });

    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}
