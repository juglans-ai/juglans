// src/builtins/devtools.rs
//
// Claude Code 风格的开发者工具集
// 提供文件操作、搜索、命令执行等 builtin tools
// 同时作为 workflow 节点和 LLM function calling 工具使用

use super::Tool;
use crate::core::context::WorkflowContext;
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::collections::HashMap;
use tracing::{debug, info};

// ============================================================
// ReadFile - 读取文件内容
// ============================================================

pub struct ReadFile;

#[async_trait]
impl Tool for ReadFile {
    fn name(&self) -> &str {
        "read_file"
    }

    fn schema(&self) -> Option<Value> {
        Some(json!({
            "type": "function",
            "function": {
                "name": "read_file",
                "description": "Read a file from the filesystem. Returns contents with line numbers (cat -n format). Supports text files, returns error for binary files.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "file_path": {
                            "type": "string",
                            "description": "The absolute or relative path to the file to read"
                        },
                        "offset": {
                            "type": "integer",
                            "description": "Line number to start reading from (1-based). Default: 1"
                        },
                        "limit": {
                            "type": "integer",
                            "description": "Maximum number of lines to return. Default: 2000"
                        }
                    },
                    "required": ["file_path"]
                }
            }
        }))
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let path = params
            .get("file_path")
            .ok_or_else(|| anyhow!("read_file() requires 'file_path' parameter"))?;

        let offset: usize = params
            .get("offset")
            .and_then(|v| v.parse().ok())
            .unwrap_or(1)
            .max(1);

        let limit: usize = params
            .get("limit")
            .and_then(|v| v.parse().ok())
            .unwrap_or(2000);

        let content = tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("Failed to read file: {}", path))?;

        let total_lines = content.lines().count();

        let lines: Vec<String> = content
            .lines()
            .skip(offset.saturating_sub(1))
            .take(limit)
            .enumerate()
            .map(|(i, line)| {
                let line_num = offset + i;
                let truncated = if line.len() > 2000 {
                    &line[..2000]
                } else {
                    line
                };
                format!("{:>6}\t{}", line_num, truncated)
            })
            .collect();

        let lines_returned = lines.len();

        info!(
            "📄 read_file: {} ({} lines, showing {}-{})",
            path,
            total_lines,
            offset,
            offset + lines_returned.saturating_sub(1)
        );

        Ok(Some(json!({
            "content": lines.join("\n"),
            "total_lines": total_lines,
            "lines_returned": lines_returned,
            "offset": offset
        })))
    }
}

// ============================================================
// WriteFile - 写入文件
// ============================================================

pub struct WriteFile;

#[async_trait]
impl Tool for WriteFile {
    fn name(&self) -> &str {
        "write_file"
    }

    fn schema(&self) -> Option<Value> {
        Some(json!({
            "type": "function",
            "function": {
                "name": "write_file",
                "description": "Write content to a file. Creates parent directories if needed. Overwrites existing file.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "file_path": {
                            "type": "string",
                            "description": "The absolute or relative path to the file to write"
                        },
                        "content": {
                            "type": "string",
                            "description": "The content to write to the file"
                        }
                    },
                    "required": ["file_path", "content"]
                }
            }
        }))
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let path = params
            .get("file_path")
            .ok_or_else(|| anyhow!("write_file() requires 'file_path' parameter"))?;

        let content = params
            .get("content")
            .ok_or_else(|| anyhow!("write_file() requires 'content' parameter"))?;

        // 创建父目录
        let file_path = std::path::Path::new(path);
        if let Some(parent) = file_path.parent() {
            if !parent.exists() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .with_context(|| format!("Failed to create directory: {}", parent.display()))?;
            }
        }

        tokio::fs::write(path, content)
            .await
            .with_context(|| format!("Failed to write file: {}", path))?;

        let line_count = content.lines().count();
        info!("📝 write_file: {} ({} lines)", path, line_count);

        Ok(Some(json!({
            "status": "ok",
            "file_path": path,
            "lines_written": line_count,
            "bytes_written": content.len()
        })))
    }
}

// ============================================================
// EditFile - 精确字符串替换
// ============================================================

pub struct EditFile;

#[async_trait]
impl Tool for EditFile {
    fn name(&self) -> &str {
        "edit_file"
    }

    fn schema(&self) -> Option<Value> {
        Some(json!({
            "type": "function",
            "function": {
                "name": "edit_file",
                "description": "Perform exact string replacement in a file. The old_string must be unique in the file unless replace_all is true. Fails if old_string is not found or found multiple times (ambiguous).",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "file_path": {
                            "type": "string",
                            "description": "The absolute or relative path to the file to modify"
                        },
                        "old_string": {
                            "type": "string",
                            "description": "The exact text to find and replace. Must be unique in the file."
                        },
                        "new_string": {
                            "type": "string",
                            "description": "The replacement text"
                        },
                        "replace_all": {
                            "type": "boolean",
                            "description": "Replace all occurrences instead of requiring uniqueness. Default: false"
                        }
                    },
                    "required": ["file_path", "old_string", "new_string"]
                }
            }
        }))
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let path = params
            .get("file_path")
            .ok_or_else(|| anyhow!("edit_file() requires 'file_path' parameter"))?;

        let old_string = params
            .get("old_string")
            .ok_or_else(|| anyhow!("edit_file() requires 'old_string' parameter"))?;

        let new_string = params
            .get("new_string")
            .ok_or_else(|| anyhow!("edit_file() requires 'new_string' parameter"))?;

        let replace_all = params
            .get("replace_all")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);

        // 读取文件
        let content = tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("Failed to read file: {}", path))?;

        // 检查匹配次数
        let match_count = content.matches(old_string).count();

        if match_count == 0 {
            return Err(anyhow!(
                "edit_file: old_string not found in {}. Make sure the text matches exactly.",
                path
            ));
        }

        if match_count > 1 && !replace_all {
            return Err(anyhow!(
                "edit_file: old_string found {} times in {}. Use replace_all=true or provide more context to make the match unique.",
                match_count, path
            ));
        }

        // 执行替换
        let new_content = if replace_all {
            content.replace(old_string, new_string)
        } else {
            content.replacen(old_string, new_string, 1)
        };

        tokio::fs::write(path, &new_content)
            .await
            .with_context(|| format!("Failed to write file: {}", path))?;

        info!("✏️ edit_file: {} ({} replacement(s))", path, match_count);

        Ok(Some(json!({
            "status": "ok",
            "file_path": path,
            "replacements": if replace_all { match_count } else { 1 }
        })))
    }
}

// ============================================================
// GlobSearch - 文件模式匹配
// ============================================================

pub struct GlobSearch;

#[async_trait]
impl Tool for GlobSearch {
    fn name(&self) -> &str {
        "glob"
    }

    fn schema(&self) -> Option<Value> {
        Some(json!({
            "type": "function",
            "function": {
                "name": "glob",
                "description": "Fast file pattern matching. Find files by glob patterns like '**/*.rs' or 'src/**/*.ts'. Returns matching file paths.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "pattern": {
                            "type": "string",
                            "description": "Glob pattern to match files (e.g., '**/*.rs', 'src/**/*.json')"
                        },
                        "path": {
                            "type": "string",
                            "description": "Base directory to search in. Defaults to current working directory."
                        }
                    },
                    "required": ["pattern"]
                }
            }
        }))
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let pattern = params
            .get("pattern")
            .ok_or_else(|| anyhow!("glob() requires 'pattern' parameter"))?;

        let base_path = params.get("path").map(|s| s.as_str()).unwrap_or(".");

        let full_pattern = if pattern.starts_with('/') {
            pattern.to_string()
        } else {
            format!("{}/{}", base_path, pattern)
        };

        let mut matches: Vec<String> = Vec::new();

        for entry in glob::glob(&full_pattern)
            .with_context(|| format!("Invalid glob pattern: {}", full_pattern))?
        {
            match entry {
                Ok(path) => {
                    matches.push(path.display().to_string());
                }
                Err(e) => {
                    debug!("Glob error for entry: {}", e);
                }
            }
        }

        info!("🔍 glob: {} → {} match(es)", full_pattern, matches.len());

        Ok(Some(json!({
            "matches": matches,
            "count": matches.len(),
            "pattern": full_pattern
        })))
    }
}

// ============================================================
// GrepSearch - 正则搜索文件内容
// ============================================================

pub struct GrepSearch;

#[async_trait]
impl Tool for GrepSearch {
    fn name(&self) -> &str {
        "grep"
    }

    fn schema(&self) -> Option<Value> {
        Some(json!({
            "type": "function",
            "function": {
                "name": "grep",
                "description": "Search file contents using regex patterns. Recursively searches through files and returns matching lines with context.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "pattern": {
                            "type": "string",
                            "description": "Regular expression pattern to search for"
                        },
                        "path": {
                            "type": "string",
                            "description": "File or directory to search in. Defaults to current directory."
                        },
                        "include": {
                            "type": "string",
                            "description": "Glob pattern to filter which files to search (e.g., '*.rs', '*.{ts,tsx}')"
                        },
                        "context_lines": {
                            "type": "integer",
                            "description": "Number of context lines before and after each match. Default: 0"
                        },
                        "max_matches": {
                            "type": "integer",
                            "description": "Maximum number of matches to return. Default: 50"
                        }
                    },
                    "required": ["pattern"]
                }
            }
        }))
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let pattern_str = params
            .get("pattern")
            .ok_or_else(|| anyhow!("grep() requires 'pattern' parameter"))?;

        let search_path = params.get("path").map(|s| s.as_str()).unwrap_or(".");
        let include = params.get("include").map(|s| s.as_str());
        let context_lines: usize = params
            .get("context_lines")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let max_matches: usize = params
            .get("max_matches")
            .and_then(|v| v.parse().ok())
            .unwrap_or(50);

        let regex = regex::Regex::new(pattern_str)
            .with_context(|| format!("Invalid regex pattern: {}", pattern_str))?;

        // 收集要搜索的文件
        let files = collect_files(search_path, include)?;

        let mut results: Vec<Value> = Vec::new();
        let mut total_matches = 0;

        'outer: for file_path in &files {
            let content = match std::fs::read_to_string(file_path) {
                Ok(c) => c,
                Err(_) => continue, // 跳过无法读取的文件（二进制等）
            };

            let lines: Vec<&str> = content.lines().collect();

            for (line_idx, line) in lines.iter().enumerate() {
                if regex.is_match(line) {
                    total_matches += 1;

                    let start = line_idx.saturating_sub(context_lines);
                    let end = (line_idx + context_lines + 1).min(lines.len());

                    let context: Vec<String> = (start..end)
                        .map(|i| format!("{:>6}\t{}", i + 1, lines[i]))
                        .collect();

                    results.push(json!({
                        "file": file_path,
                        "line": line_idx + 1,
                        "match": line.trim(),
                        "context": context.join("\n")
                    }));

                    if results.len() >= max_matches {
                        break 'outer;
                    }
                }
            }
        }

        info!(
            "🔎 grep: '{}' in {} → {} match(es) across {} file(s)",
            pattern_str,
            search_path,
            total_matches,
            files.len()
        );

        Ok(Some(json!({
            "matches": results,
            "total_matches": total_matches,
            "files_searched": files.len(),
            "truncated": total_matches > max_matches
        })))
    }
}

/// 收集指定目录下的文件列表
fn collect_files(path: &str, include: Option<&str>) -> Result<Vec<String>> {
    let p = std::path::Path::new(path);

    // 如果是单个文件，直接返回
    if p.is_file() {
        return Ok(vec![path.to_string()]);
    }

    // 目录：使用 glob 递归
    let pattern = match include {
        Some(inc) => format!("{}/{}", path, inc),
        None => format!("{}/**/*", path),
    };

    let mut files = Vec::new();
    for p in (glob::glob(&pattern).with_context(|| format!("Invalid glob: {}", pattern))?).flatten()
    {
        if p.is_file() {
            files.push(p.display().to_string());
        }
    }

    Ok(files)
}

// ============================================================
// Bash - Shell 命令执行（替代旧 Shell 工具）
// ============================================================

pub struct Bash;

#[async_trait]
impl Tool for Bash {
    fn name(&self) -> &str {
        "bash"
    }

    fn schema(&self) -> Option<Value> {
        Some(json!({
            "type": "function",
            "function": {
                "name": "bash",
                "description": "Execute a bash command. Returns stdout, stderr, and exit code. Use for git, npm, docker, build tools, etc.",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "command": {
                            "type": "string",
                            "description": "The bash command to execute"
                        },
                        "timeout": {
                            "type": "integer",
                            "description": "Timeout in milliseconds. Default: 120000 (2 min), max: 600000 (10 min)"
                        },
                        "description": {
                            "type": "string",
                            "description": "Brief description of what the command does"
                        }
                    },
                    "required": ["command"]
                }
            }
        }))
    }

    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        // 兼容旧 sh(cmd=...) 语法
        let cmd = params
            .get("command")
            .or_else(|| params.get("cmd"))
            .ok_or_else(|| anyhow!("bash() requires 'command' parameter"))?
            .trim_matches('"');

        let timeout_ms: u64 = params
            .get("timeout")
            .and_then(|v| v.parse().ok())
            .unwrap_or(120_000)
            .min(600_000);

        let desc = params.get("description").map(|s| s.as_str()).unwrap_or("");

        if !desc.is_empty() {
            info!("🖥️ bash: {} ({})", desc, cmd);
        } else {
            info!("🖥️ bash: {}", cmd);
        }

        let timeout_duration = tokio::time::Duration::from_millis(timeout_ms);

        let output_result = tokio::time::timeout(
            timeout_duration,
            tokio::process::Command::new("sh")
                .arg("-c")
                .arg(cmd)
                .output(),
        )
        .await;

        match output_result {
            Ok(Ok(output)) => {
                let mut stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let mut stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let exit_code = output.status.code().unwrap_or(-1);

                // 输出截断
                const MAX_OUTPUT: usize = 30000;
                let stdout_truncated = stdout.len() > MAX_OUTPUT;
                let stderr_truncated = stderr.len() > MAX_OUTPUT;
                if stdout_truncated {
                    stdout.truncate(MAX_OUTPUT);
                    stdout.push_str("\n... (output truncated)");
                }
                if stderr_truncated {
                    stderr.truncate(MAX_OUTPUT);
                    stderr.push_str("\n... (output truncated)");
                }

                Ok(Some(json!({
                    "stdout": stdout.trim(),
                    "stderr": stderr.trim(),
                    "exit_code": exit_code,
                    "ok": output.status.success()
                })))
            }
            Ok(Err(e)) => Err(anyhow!("Failed to execute command: {}", e)),
            Err(_) => Err(anyhow!(
                "Command timed out after {} ms: {}",
                timeout_ms,
                cmd
            )),
        }
    }
}
