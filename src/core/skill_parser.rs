// src/core/skill_parser.rs
//
// Parser for Agent Skills spec (SKILL.md files).
// Converts SKILL.md → .jgprompt format for use in the juglans ecosystem.

use anyhow::{anyhow, Result};
use std::collections::HashMap;
use std::fs;
use std::path::Path;

/// Parsed SKILL.md frontmatter fields.
#[derive(Debug, Clone)]
pub struct SkillFrontmatter {
    pub name: String,
    pub description: String,
    pub license: Option<String>,
    pub compatibility: Option<String>,
    pub allowed_tools: Option<String>,
    pub metadata: HashMap<String, String>,
}

/// A fully parsed skill with body content and optional resources.
#[derive(Debug, Clone)]
pub struct SkillResource {
    pub name: String,
    pub description: String,
    pub body: String,
    pub references: Vec<(String, String)>,
    pub scripts: Vec<(String, String)>,
}

/// Parse SKILL.md content into a SkillResource.
///
/// Format: YAML frontmatter between `---` delimiters, followed by Markdown body.
/// ```text
/// ---
/// name: skill-name
/// description: What this skill does.
/// ---
/// # Instructions
/// ...
/// ```
pub fn parse_skill_md(content: &str) -> Result<SkillResource> {
    let trimmed = content.trim();

    // Must start with ---
    if !trimmed.starts_with("---") {
        return Err(anyhow!("SKILL.md must start with '---' (YAML frontmatter delimiter)"));
    }

    // Find the closing ---
    let after_first = &trimmed[3..];
    let closing_pos = after_first.find("\n---")
        .ok_or_else(|| anyhow!("Missing closing '---' for YAML frontmatter"))?;

    let frontmatter_str = &after_first[..closing_pos].trim();
    let body_start = 3 + closing_pos + 4; // skip "\n---"
    let body = if body_start < trimmed.len() {
        trimmed[body_start..].trim().to_string()
    } else {
        String::new()
    };

    // Parse frontmatter (simple YAML key-value)
    let fm = parse_frontmatter(frontmatter_str)?;

    Ok(SkillResource {
        name: fm.name,
        description: fm.description,
        body,
        references: Vec::new(),
        scripts: Vec::new(),
    })
}

/// Parse simple YAML frontmatter key-value pairs.
/// Supports: name, description, license, compatibility, allowed-tools, metadata.
fn parse_frontmatter(input: &str) -> Result<SkillFrontmatter> {
    let mut name = None;
    let mut description = None;
    let mut license = None;
    let mut compatibility = None;
    let mut allowed_tools = None;
    let mut metadata = HashMap::new();
    let mut in_metadata = false;

    for line in input.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Detect metadata block (indented key-value under "metadata:")
        if in_metadata {
            if line.starts_with("  ") || line.starts_with("\t") {
                if let Some((k, v)) = trimmed.split_once(':') {
                    let key = k.trim().to_string();
                    let val = strip_yaml_quotes(v.trim());
                    metadata.insert(key, val);
                    continue;
                }
            }
            in_metadata = false;
        }

        if let Some((key, value)) = trimmed.split_once(':') {
            let key = key.trim();
            let value = value.trim();

            match key {
                "name" => name = Some(strip_yaml_quotes(value)),
                "description" => description = Some(strip_yaml_quotes(value)),
                "license" => license = Some(strip_yaml_quotes(value)),
                "compatibility" => compatibility = Some(strip_yaml_quotes(value)),
                "allowed-tools" => allowed_tools = Some(strip_yaml_quotes(value)),
                "metadata" => {
                    in_metadata = true;
                }
                _ => {}
            }
        }
    }

    Ok(SkillFrontmatter {
        name: name.ok_or_else(|| anyhow!("SKILL.md frontmatter missing required 'name' field"))?,
        description: description.ok_or_else(|| anyhow!("SKILL.md frontmatter missing required 'description' field"))?,
        license,
        compatibility,
        allowed_tools,
        metadata,
    })
}

/// Remove surrounding quotes from a YAML string value.
fn strip_yaml_quotes(s: &str) -> String {
    let s = s.trim();
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        s[1..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

/// Load a skill from a directory (reads SKILL.md + scripts/ + references/).
pub fn load_skill_dir(dir: &Path) -> Result<SkillResource> {
    let skill_md_path = dir.join("SKILL.md");
    if !skill_md_path.exists() {
        return Err(anyhow!("No SKILL.md found in {}", dir.display()));
    }

    let content = fs::read_to_string(&skill_md_path)?;
    let mut skill = parse_skill_md(&content)?;

    // Load references/ directory
    let refs_dir = dir.join("references");
    if refs_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(&refs_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Ok(content) = fs::read_to_string(&path) {
                        let fname = path.file_name().unwrap().to_string_lossy().to_string();
                        skill.references.push((fname, content));
                    }
                }
            }
        }
    }

    // Load scripts/ directory
    let scripts_dir = dir.join("scripts");
    if scripts_dir.is_dir() {
        if let Ok(entries) = fs::read_dir(&scripts_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() {
                    if let Ok(content) = fs::read_to_string(&path) {
                        let fname = path.file_name().unwrap().to_string_lossy().to_string();
                        skill.scripts.push((fname, content));
                    }
                }
            }
        }
    }

    Ok(skill)
}

/// Convert a SkillResource to .jgprompt file content.
pub fn skill_to_jgprompt(skill: &SkillResource) -> String {
    let mut out = String::new();

    // Frontmatter
    out.push_str("---\n");
    out.push_str(&format!("slug: \"{}\"\n", skill.name));
    out.push_str(&format!(
        "name: \"{}\"\n",
        skill.name.replace('-', " ")
    ));
    out.push_str("type: \"system\"\n");
    out.push_str(&format!(
        "description: \"{}\"\n",
        skill.description.replace('"', "\\\"")
    ));
    out.push_str("---\n");

    // Body (skill instructions)
    out.push_str(&skill.body);

    // Embed references
    for (name, content) in &skill.references {
        out.push_str(&format!("\n\n---\n## Reference: {}\n{}", name, content));
    }

    // Embed scripts as code blocks
    for (name, content) in &skill.scripts {
        let ext = name.rsplit('.').next().unwrap_or("sh");
        out.push_str(&format!(
            "\n\n---\n## Script: {}\n```{}\n{}\n```",
            name, ext, content
        ));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_skill() {
        let content = r#"---
name: pdf
description: Extract text and tables from PDF files.
---
# PDF Processing

Use pypdf for basic operations.
"#;
        let skill = parse_skill_md(content).unwrap();
        assert_eq!(skill.name, "pdf");
        assert_eq!(skill.description, "Extract text and tables from PDF files.");
        assert!(skill.body.contains("PDF Processing"));
        assert!(skill.body.contains("pypdf"));
    }

    #[test]
    fn test_parse_quoted_fields() {
        let content = r#"---
name: "web-artifacts-builder"
description: "Build multi-component HTML artifacts"
license: Apache-2.0
---
Instructions here.
"#;
        let skill = parse_skill_md(content).unwrap();
        assert_eq!(skill.name, "web-artifacts-builder");
        assert_eq!(skill.description, "Build multi-component HTML artifacts");
    }

    #[test]
    fn test_skill_to_jgprompt() {
        let skill = SkillResource {
            name: "pdf".to_string(),
            description: "Extract PDF content.".to_string(),
            body: "# Instructions\nUse pypdf.".to_string(),
            references: vec![("REFERENCE.md".to_string(), "Ref content".to_string())],
            scripts: vec![("extract.py".to_string(), "print('hello')".to_string())],
        };

        let output = skill_to_jgprompt(&skill);
        assert!(output.contains("slug: \"pdf\""));
        assert!(output.contains("type: \"system\""));
        assert!(output.contains("# Instructions"));
        assert!(output.contains("## Reference: REFERENCE.md"));
        assert!(output.contains("```py\nprint('hello')\n```"));
    }

    #[test]
    fn test_missing_name_fails() {
        let content = r#"---
description: No name here.
---
Body.
"#;
        assert!(parse_skill_md(content).is_err());
    }
}
