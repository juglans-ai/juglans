use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use unicode_width::UnicodeWidthChar;

use super::theme::Theme;

const CODE_BG: Color = Color::Rgb(30, 30, 35);

/// Render markdown content into styled ratatui Lines.
/// Custom line-by-line parser — no external dependencies, streaming-friendly.
/// `width` is used to pre-wrap list items so continuation lines are indented.
pub fn render_markdown(
    content: &str,
    theme: &Theme,
    width: u16,
) -> (Vec<Line<'static>>, Vec<(String, String)>) {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut all_links: Vec<(String, String)> = Vec::new();
    let mut in_code_block = false;
    let w = width as usize;

    // Pre-scan for table blocks: collect consecutive lines starting with '|'
    let raw_lines: Vec<&str> = content.lines().collect();
    let mut table_rows: Vec<Vec<String>> = Vec::new();
    let mut table_start: Option<usize> = None;

    // We'll process line-by-line but need lookahead for tables, so use index
    let mut idx = 0;
    while idx < raw_lines.len() {
        let raw_line = raw_lines[idx];

        // --- Code block toggle ---
        if raw_line.trim_start().starts_with("```") {
            // Flush any pending table
            if !table_rows.is_empty() {
                render_table(&table_rows, theme, &mut lines);
                table_rows.clear();
                table_start = None;
            }
            if in_code_block {
                in_code_block = false;
            } else {
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
            idx += 1;
            continue;
        }

        // --- Inside code block ---
        if in_code_block {
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled("│ ", Style::default().fg(theme.border).bg(CODE_BG)),
                Span::styled(
                    raw_line.to_string(),
                    Style::default().fg(theme.code).bg(CODE_BG),
                ),
            ]));
            idx += 1;
            continue;
        }

        // --- Table detection ---
        let trimmed_line = raw_line.trim();
        if trimmed_line.starts_with('|') && trimmed_line.ends_with('|') {
            if table_start.is_none() {
                table_start = Some(idx);
            }
            // Check if this is a separator row (| --- | --- |)
            let is_separator = trimmed_line
                .split('|')
                .filter(|s| !s.is_empty())
                .all(|cell| {
                    let t = cell.trim();
                    t.chars().all(|c| c == '-' || c == ':' || c == ' ') && t.contains('-')
                });
            if !is_separator {
                let cells: Vec<String> = trimmed_line
                    .split('|')
                    .filter(|s| !s.is_empty())
                    .map(|s| s.trim().to_string())
                    .collect();
                table_rows.push(cells);
            }
            idx += 1;
            continue;
        }

        // Flush any pending table before processing non-table line
        if !table_rows.is_empty() {
            render_table(&table_rows, theme, &mut lines);
            table_rows.clear();
            table_start = None;
        }

        // --- Empty line: collapse consecutive blank lines into one ---
        if raw_line.trim().is_empty() {
            if lines.last().is_some_and(|l| l.width() <= 2) {
                idx += 1;
                continue;
            }
            lines.push(Line::from(Span::styled("", Style::default())));
            idx += 1;
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
            spans.extend(parse_inline_collecting(
                text,
                Style::default()
                    .fg(theme.heading)
                    .add_modifier(Modifier::BOLD),
                theme,
                &mut all_links,
            ));
            lines.push(Line::from(spans));
            idx += 1;
            continue;
        }
        if let Some(text) = raw_line.strip_prefix("## ") {
            let mut spans = vec![Span::styled(
                "  ## ",
                Style::default()
                    .fg(theme.heading)
                    .add_modifier(Modifier::BOLD),
            )];
            spans.extend(parse_inline_collecting(
                text,
                Style::default()
                    .fg(theme.heading)
                    .add_modifier(Modifier::BOLD),
                theme,
                &mut all_links,
            ));
            lines.push(Line::from(spans));
            idx += 1;
            continue;
        }
        if let Some(text) = raw_line.strip_prefix("# ") {
            let mut spans = vec![Span::styled(
                "  # ",
                Style::default()
                    .fg(theme.heading)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            )];
            spans.extend(parse_inline_collecting(
                text,
                Style::default()
                    .fg(theme.heading)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
                theme,
                &mut all_links,
            ));
            lines.push(Line::from(spans));
            idx += 1;
            continue;
        }

        // --- Horizontal rule ---
        if (trimmed_line.starts_with("---")
            || trimmed_line.starts_with("***")
            || trimmed_line.starts_with("___"))
            && trimmed_line
                .chars()
                .all(|c| c == '-' || c == '*' || c == '_' || c == ' ')
            && trimmed_line.len() >= 3
        {
            lines.push(Line::from(Span::styled(
                "  ───────────────────────────────",
                Style::default().fg(theme.border),
            )));
            idx += 1;
            continue;
        }

        // --- Blockquote ---
        if let Some(text) = raw_line.strip_prefix("> ") {
            let mut spans = vec![Span::styled("  ▌ ", Style::default().fg(theme.thinking))];
            spans.extend(parse_inline_collecting(
                text,
                Style::default().fg(theme.thinking),
                theme,
                &mut all_links,
            ));
            lines.push(Line::from(spans));
            idx += 1;
            continue;
        }

        // --- Unordered list ---
        if raw_line.starts_with("- ") || raw_line.starts_with("* ") {
            let text = &raw_line[2..];
            let prefix_w = 4; // "  ● " = 4 columns
            render_wrapped_list(
                &mut lines,
                "● ",
                "  ",
                prefix_w,
                text,
                w,
                Style::default().fg(theme.accent),
                Style::default().fg(theme.fg),
                theme,
                &mut all_links,
            );
            idx += 1;
            continue;
        }
        // Nested list (2-4 spaces + - or *)
        if let Some(rest) = raw_line
            .strip_prefix("  - ")
            .or_else(|| raw_line.strip_prefix("  * "))
        {
            let prefix_w = 6; // "    ◦ " = 6 columns
            render_wrapped_list(
                &mut lines,
                "◦ ",
                "    ",
                prefix_w,
                rest,
                w,
                Style::default().fg(theme.accent),
                Style::default().fg(theme.fg),
                theme,
                &mut all_links,
            );
            idx += 1;
            continue;
        }

        // --- Ordered list ---
        if let Some(pos) = raw_line.find(". ") {
            let prefix = &raw_line[..pos];
            if !prefix.is_empty() && prefix.chars().all(|c| c.is_ascii_digit()) {
                let text = &raw_line[pos + 2..];
                let bullet = format!("{}. ", prefix);
                let prefix_w = 2 + display_width(&bullet); // "  " + "N. "
                render_wrapped_list(
                    &mut lines,
                    &bullet,
                    "  ",
                    prefix_w,
                    text,
                    w,
                    Style::default().fg(theme.accent),
                    Style::default().fg(theme.fg),
                    theme,
                    &mut all_links,
                );
                idx += 1;
                continue;
            }
        }

        // --- Empty line ---
        if raw_line.trim().is_empty() {
            lines.push(Line::from(""));
            idx += 1;
            continue;
        }

        // --- Normal paragraph line (with inline formatting) ---
        let mut spans = vec![Span::styled("  ", Style::default())];
        spans.extend(parse_inline_collecting(
            raw_line,
            Style::default().fg(theme.fg),
            theme,
            &mut all_links,
        ));
        lines.push(Line::from(spans));
        idx += 1;
    }

    // Flush any trailing table
    if !table_rows.is_empty() {
        render_table(&table_rows, theme, &mut lines);
    }

    (lines, all_links)
}

/// Render a list item with manual wrapping so continuation lines are indented.
/// Parses inline markdown FIRST, then wraps the resulting spans by display width.
#[allow(clippy::too_many_arguments)]
fn render_wrapped_list(
    lines: &mut Vec<Line<'static>>,
    bullet: &str,
    outer_pad: &str,
    prefix_w: usize,
    text: &str,
    width: usize,
    bullet_style: Style,
    text_style: Style,
    theme: &Theme,
    all_links: &mut Vec<(String, String)>,
) {
    let avail = if width > prefix_w {
        width - prefix_w
    } else {
        width.max(1)
    };

    // Parse inline markdown first (links, bold, etc. stay intact)
    let parsed_spans = parse_inline_collecting(text, text_style, theme, all_links);
    let wrapped = wrap_spans(parsed_spans, avail);

    if wrapped.is_empty() {
        lines.push(Line::from(vec![
            Span::styled(outer_pad.to_string(), Style::default()),
            Span::styled(bullet.to_string(), bullet_style),
        ]));
        return;
    }

    for (i, visual_spans) in wrapped.into_iter().enumerate() {
        let mut row_spans = if i == 0 {
            vec![
                Span::styled(outer_pad.to_string(), Style::default()),
                Span::styled(bullet.to_string(), bullet_style),
            ]
        } else {
            vec![Span::styled(" ".repeat(prefix_w), Style::default())]
        };
        row_spans.extend(visual_spans);
        lines.push(Line::from(row_spans));
    }
}

/// Wrap pre-parsed spans into visual lines that fit within `max_w` display columns.
/// Splits individual spans at character boundaries when needed.
fn wrap_spans(spans: Vec<Span<'static>>, max_w: usize) -> Vec<Vec<Span<'static>>> {
    if max_w == 0 {
        return vec![spans];
    }
    let mut result: Vec<Vec<Span<'static>>> = Vec::new();
    let mut current_line: Vec<Span<'static>> = Vec::new();
    let mut current_w = 0usize;

    for span in spans {
        let chars: Vec<char> = span.content.chars().collect();
        let style = span.style;
        let mut pos = 0;

        while pos < chars.len() {
            let remaining = max_w.saturating_sub(current_w);

            // If current line is full, start a new one
            if remaining == 0 {
                result.push(std::mem::take(&mut current_line));
                current_w = 0;
                continue;
            }

            // Take as many chars as fit in remaining width
            let mut col = 0usize;
            let mut end = pos;
            while end < chars.len() {
                let cw = UnicodeWidthChar::width(chars[end]).unwrap_or(1);
                if col + cw > remaining {
                    break;
                }
                col += cw;
                end += 1;
            }

            if end == pos {
                // Can't fit even one char on current line
                if !current_line.is_empty() {
                    result.push(std::mem::take(&mut current_line));
                    current_w = 0;
                    continue;
                }
                // Empty line but still can't fit — force one char
                let cw = UnicodeWidthChar::width(chars[pos]).unwrap_or(1);
                let ch: String = chars[pos..pos + 1].iter().collect();
                current_line.push(Span::styled(ch, style));
                current_w += cw;
                pos += 1;
                continue;
            }

            let chunk: String = chars[pos..end].iter().collect();
            current_line.push(Span::styled(chunk, style));
            current_w += col;
            pos = end;
        }
    }

    if !current_line.is_empty() {
        result.push(current_line);
    }

    result
}

/// Calculate display width of a string.
fn display_width(s: &str) -> usize {
    s.chars()
        .map(|c| UnicodeWidthChar::width(c).unwrap_or(1))
        .sum()
}

/// Render a table block.
fn render_table(rows: &[Vec<String>], theme: &Theme, lines: &mut Vec<Line<'static>>) {
    if rows.is_empty() {
        return;
    }
    // Compute column widths
    let ncols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    let mut col_widths = vec![0usize; ncols];
    for row in rows {
        for (ci, cell) in row.iter().enumerate() {
            if ci < ncols {
                col_widths[ci] = col_widths[ci].max(display_width(cell));
            }
        }
    }

    for (ri, row) in rows.iter().enumerate() {
        let is_header = ri == 0 && rows.len() > 1;
        let mut spans = vec![Span::styled("  ", Style::default())];

        for (ci, cell) in row.iter().enumerate() {
            let cw = if ci < ncols { col_widths[ci] } else { 0 };
            let cell_w = display_width(cell);
            let pad = cw.saturating_sub(cell_w);

            if ci > 0 {
                spans.push(Span::styled(" │ ", Style::default().fg(theme.border)));
            }

            let style = if is_header {
                Style::default().fg(theme.fg).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.fg)
            };
            spans.push(Span::styled(cell.clone(), style));
            if pad > 0 {
                spans.push(Span::styled(" ".repeat(pad), Style::default()));
            }
        }

        lines.push(Line::from(spans));

        // Separator after header
        if is_header {
            let total: usize = col_widths.iter().sum::<usize>() + (ncols.saturating_sub(1)) * 3;
            lines.push(Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled("─".repeat(total), Style::default().fg(theme.border)),
            ]));
        }
    }
}

fn parse_inline_collecting(
    text: &str,
    base_style: Style,
    theme: &Theme,
    links: &mut Vec<(String, String)>,
) -> Vec<Span<'static>> {
    parse_inline_with_links(text, base_style, theme, Some(links))
}

fn parse_inline_with_links(
    text: &str,
    base_style: Style,
    theme: &Theme,
    mut links_out: Option<&mut Vec<(String, String)>>,
) -> Vec<Span<'static>> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;
    let mut buf = String::new();

    while i < len {
        // --- Inline code: `...` ---
        if chars[i] == '`' {
            if !buf.is_empty() {
                spans.push(Span::styled(std::mem::take(&mut buf), base_style));
            }
            if let Some(end) = find_closing(&chars, i + 1, '`') {
                let code: String = chars[i + 1..end].iter().collect();
                spans.push(Span::styled(
                    format!(" {} ", code),
                    Style::default().fg(theme.code).bg(CODE_BG),
                ));
                i = end + 1;
                continue;
            }
            buf.push('`');
            i += 1;
            continue;
        }

        // --- Link: [text](url) ---
        if chars[i] == '[' {
            if let Some(close_bracket) = find_closing(&chars, i + 1, ']') {
                if close_bracket + 1 < len && chars[close_bracket + 1] == '(' {
                    if let Some(close_paren) = find_closing(&chars, close_bracket + 2, ')') {
                        if !buf.is_empty() {
                            spans.push(Span::styled(std::mem::take(&mut buf), base_style));
                        }
                        let link_text: String = chars[i + 1..close_bracket].iter().collect();
                        let link_url: String =
                            chars[close_bracket + 2..close_paren].iter().collect();
                        if let Some(ref mut links) = links_out {
                            links.push((link_text.clone(), link_url));
                        }
                        spans.push(Span::styled(
                            link_text,
                            base_style
                                .fg(theme.accent)
                                .add_modifier(Modifier::UNDERLINED),
                        ));
                        i = close_paren + 1;
                        continue;
                    }
                }
            }
            buf.push('[');
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
            if i + 1 < chars.len() && chars[i + 1] == delim {
                continue;
            }
            if i > start && chars[i - 1] == delim {
                continue;
            }
            return Some(i);
        }
    }
    None
}
