use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

use super::theme::Theme;

const CODE_BG: Color = Color::Rgb(30, 30, 35);

/// Render markdown content into styled ratatui Lines.
/// Custom line-by-line parser — no external dependencies, streaming-friendly.
pub fn render_markdown(content: &str, theme: &Theme) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut in_code_block = false;

    for raw_line in content.lines() {
        // --- Code block toggle ---
        if raw_line.trim_start().starts_with("```") {
            if in_code_block {
                // Close code block
                in_code_block = false;
            } else {
                // Open code block — extract language label
                in_code_block = true;
                let lang = raw_line
                    .trim_start()
                    .strip_prefix("```")
                    .unwrap_or("")
                    .trim();
                if !lang.is_empty() {
                    lines.push(Line::from(vec![
                        Span::styled("  ", Style::default()),
                        Span::styled(
                            format!(" {} ", lang),
                            Style::default().fg(theme.muted).bg(CODE_BG),
                        ),
                    ]));
                }
            }
            continue;
        }

        // --- Inside code block ---
        if in_code_block {
            lines.push(Line::from(vec![
                Span::styled("  │ ", Style::default().fg(theme.border).bg(CODE_BG)),
                Span::styled(
                    raw_line.to_string(),
                    Style::default().fg(theme.code).bg(CODE_BG),
                ),
            ]));
            continue;
        }

        // --- Headers ---
        if let Some(text) = raw_line.strip_prefix("### ") {
            let mut spans = vec![Span::styled(
                "  ### ",
                Style::default()
                    .fg(theme.heading)
                    .add_modifier(Modifier::BOLD),
            )];
            spans.extend(parse_inline(
                text,
                Style::default()
                    .fg(theme.heading)
                    .add_modifier(Modifier::BOLD),
                theme,
            ));
            lines.push(Line::from(spans));
            continue;
        }
        if let Some(text) = raw_line.strip_prefix("## ") {
            let mut spans = vec![Span::styled(
                "  ## ",
                Style::default()
                    .fg(theme.heading)
                    .add_modifier(Modifier::BOLD),
            )];
            spans.extend(parse_inline(
                text,
                Style::default()
                    .fg(theme.heading)
                    .add_modifier(Modifier::BOLD),
                theme,
            ));
            lines.push(Line::from(spans));
            continue;
        }
        if let Some(text) = raw_line.strip_prefix("# ") {
            let mut spans = vec![Span::styled(
                "  # ",
                Style::default()
                    .fg(theme.heading)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            )];
            spans.extend(parse_inline(
                text,
                Style::default()
                    .fg(theme.heading)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                theme,
            ));
            lines.push(Line::from(spans));
            continue;
        }

        // --- Horizontal rule ---
        let trimmed = raw_line.trim();
        if (trimmed.starts_with("---") || trimmed.starts_with("***") || trimmed.starts_with("___"))
            && trimmed
                .chars()
                .all(|c| c == '-' || c == '*' || c == '_' || c == ' ')
            && trimmed.len() >= 3
        {
            lines.push(Line::from(Span::styled(
                "  ───────────────────────────────",
                Style::default().fg(theme.border),
            )));
            continue;
        }

        // --- Blockquote ---
        if let Some(text) = raw_line.strip_prefix("> ") {
            let mut spans = vec![Span::styled("  ▌ ", Style::default().fg(theme.thinking))];
            spans.extend(parse_inline(
                text,
                Style::default().fg(theme.thinking),
                theme,
            ));
            lines.push(Line::from(spans));
            continue;
        }

        // --- Unordered list ---
        if raw_line.starts_with("- ") || raw_line.starts_with("* ") {
            let text = &raw_line[2..];
            let mut spans = vec![
                Span::styled("  ", Style::default()),
                Span::styled("● ", Style::default().fg(theme.accent)),
            ];
            spans.extend(parse_inline(text, Style::default().fg(theme.fg), theme));
            lines.push(Line::from(spans));
            continue;
        }
        // Nested list (2-4 spaces + - or *)
        if let Some(rest) = raw_line
            .strip_prefix("  - ")
            .or_else(|| raw_line.strip_prefix("  * "))
        {
            let mut spans = vec![
                Span::styled("    ", Style::default()),
                Span::styled("◦ ", Style::default().fg(theme.accent)),
            ];
            spans.extend(parse_inline(rest, Style::default().fg(theme.fg), theme));
            lines.push(Line::from(spans));
            continue;
        }

        // --- Ordered list ---
        if let Some(pos) = raw_line.find(". ") {
            let prefix = &raw_line[..pos];
            if !prefix.is_empty() && prefix.chars().all(|c| c.is_ascii_digit()) {
                let text = &raw_line[pos + 2..];
                let mut spans = vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(format!("{}. ", prefix), Style::default().fg(theme.accent)),
                ];
                spans.extend(parse_inline(text, Style::default().fg(theme.fg), theme));
                lines.push(Line::from(spans));
                continue;
            }
        }

        // --- Empty line ---
        if raw_line.trim().is_empty() {
            lines.push(Line::from(""));
            continue;
        }

        // --- Normal paragraph line (with inline formatting) ---
        let mut spans = vec![Span::styled("  ", Style::default())];
        spans.extend(parse_inline(raw_line, Style::default().fg(theme.fg), theme));
        lines.push(Line::from(spans));
    }

    lines
}

/// Parse inline markdown formatting: **bold**, *italic*, `code`
fn parse_inline(text: &str, base_style: Style, theme: &Theme) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut buf = String::new();

    while i < len {
        // --- Inline code: `...` ---
        if chars[i] == '`' {
            // Flush buffer
            if !buf.is_empty() {
                spans.push(Span::styled(std::mem::take(&mut buf), base_style));
            }
            // Find closing backtick
            if let Some(end) = find_closing(&chars, i + 1, '`') {
                let code: String = chars[i + 1..end].iter().collect();
                spans.push(Span::styled(
                    format!(" {} ", code),
                    Style::default().fg(theme.code).bg(CODE_BG),
                ));
                i = end + 1;
                continue;
            }
            // No closing — treat as literal
            buf.push('`');
            i += 1;
            continue;
        }

        // --- Bold: **...** ---
        if i + 1 < len && chars[i] == '*' && chars[i + 1] == '*' {
            if !buf.is_empty() {
                spans.push(Span::styled(std::mem::take(&mut buf), base_style));
            }
            if let Some(end) = find_double_closing(&chars, i + 2, '*') {
                let inner: String = chars[i + 2..end].iter().collect();
                spans.push(Span::styled(inner, base_style.add_modifier(Modifier::BOLD)));
                i = end + 2;
                continue;
            }
            buf.push_str("**");
            i += 2;
            continue;
        }

        // --- Italic: *...* (single, not followed by another *) ---
        if chars[i] == '*' && (i + 1 >= len || chars[i + 1] != '*') {
            if !buf.is_empty() {
                spans.push(Span::styled(std::mem::take(&mut buf), base_style));
            }
            if let Some(end) = find_single_closing(&chars, i + 1, '*') {
                let inner: String = chars[i + 1..end].iter().collect();
                spans.push(Span::styled(
                    inner,
                    base_style.add_modifier(Modifier::ITALIC),
                ));
                i = end + 1;
                continue;
            }
            buf.push('*');
            i += 1;
            continue;
        }

        // --- Regular character ---
        buf.push(chars[i]);
        i += 1;
    }

    // Flush remaining
    if !buf.is_empty() {
        spans.push(Span::styled(buf, base_style));
    }

    spans
}

/// Find closing single delimiter (e.g., ` or *), returns index of the closing char.
fn find_closing(chars: &[char], start: usize, delim: char) -> Option<usize> {
    (start..chars.len()).find(|&i| chars[i] == delim)
}

/// Find closing ** (double delimiter), returns index of first * of **.
fn find_double_closing(chars: &[char], start: usize, delim: char) -> Option<usize> {
    let mut i = start;
    while i + 1 < chars.len() {
        if chars[i] == delim && chars[i + 1] == delim {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Find closing single * that is NOT followed by another * (for italic).
fn find_single_closing(chars: &[char], start: usize, delim: char) -> Option<usize> {
    for i in start..chars.len() {
        if chars[i] == delim {
            // Make sure it's not part of ** (bold)
            if i + 1 < chars.len() && chars[i + 1] == delim {
                continue;
            }
            // Also check it's not preceded by another delim forming **
            if i > start && chars[i - 1] == delim {
                continue;
            }
            return Some(i);
        }
    }
    None
}
