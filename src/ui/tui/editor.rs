use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};
use tui_textarea::TextArea;

use super::messages::{Attachment, AttachmentKind};
use super::theme::Theme;

pub struct EditorState {
    pub textarea: TextArea<'static>,
}

impl Default for EditorState {
    fn default() -> Self {
        Self::new()
    }
}

impl EditorState {
    pub fn new() -> Self {
        let mut textarea = TextArea::default();
        textarea.set_cursor_line_style(Style::default());
        textarea.set_placeholder_text("Type a message...");
        Self { textarea }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Option<String> {
        match (key.code, key.modifiers) {
            (KeyCode::Enter, KeyModifiers::SHIFT) => {
                // Shift+Enter inserts a newline
                let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
                self.textarea.input(enter);
                None
            }
            (KeyCode::Enter, KeyModifiers::NONE) => {
                let lines: Vec<String> = self
                    .textarea
                    .lines()
                    .iter()
                    .map(|s| s.to_string())
                    .collect();
                let text = lines.join("\n").trim().to_string();
                if text.is_empty() {
                    return None;
                }
                self.textarea = TextArea::default();
                self.textarea.set_cursor_line_style(Style::default());
                self.textarea.set_placeholder_text("Type a message...");
                Some(text)
            }
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                self.textarea = TextArea::default();
                self.textarea.set_cursor_line_style(Style::default());
                self.textarea.set_placeholder_text("Type a message...");
                None
            }
            (KeyCode::Esc, _) => {
                // Clear input
                self.textarea = TextArea::default();
                self.textarea.set_cursor_line_style(Style::default());
                self.textarea.set_placeholder_text("Type a message...");
                None
            }
            _ => {
                self.textarea.input(key);
                None
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.textarea.lines().iter().all(|l| l.is_empty())
    }

    /// Returns (row, col) of the cursor within the textarea
    pub fn cursor_pos(&self) -> (usize, usize) {
        self.textarea.cursor()
    }

    /// Move cursor to a specific (row, col) position
    pub fn set_cursor(&mut self, row: usize, col: usize) {
        use tui_textarea::CursorMove;
        // Move to top-left first
        self.textarea.move_cursor(CursorMove::Top);
        self.textarea.move_cursor(CursorMove::Head);
        // Move down to target row
        let max_row = self.textarea.lines().len().saturating_sub(1);
        let target_row = row.min(max_row);
        for _ in 0..target_row {
            self.textarea.move_cursor(CursorMove::Down);
        }
        // Move right to target col
        let line_len = self.textarea.lines().get(target_row).map_or(0, |l| l.len());
        let target_col = col.min(line_len);
        for _ in 0..target_col {
            self.textarea.move_cursor(CursorMove::Forward);
        }
    }
}

pub struct EditorWidget<'a> {
    pub state: &'a EditorState,
    pub theme: &'a Theme,
    pub focused: bool,
    #[allow(dead_code)]
    pub scroll_offset: usize,
    pub streaming: bool,
    pub attachments: &'a [Attachment],
    pub attachment_selected: bool,
}

impl<'a> Widget for EditorWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Fill background with input_bg color
        // Only right of blue border (x+1), exclude only the hints row (last row)
        let bg_y_end = area.y + area.height.saturating_sub(2); // exclude bottom padding + hints row
        for y in area.y..bg_y_end {
            for x in (area.x + 1)..area.x + area.width {
                if let Some(cell) = buf.cell_mut((x, y)) {
                    cell.set_style(Style::default().bg(self.theme.input_bg));
                }
            }
        }

        // Blue left border — no bg, just the colored line character
        let border_color = if self.focused {
            self.theme.input_border
        } else {
            self.theme.border
        };
        let border_height = area.height.saturating_sub(2); // exclude variant + hints rows
        for y in area.y..area.y + border_height {
            if let Some(cell) = buf.cell_mut((area.x, y)) {
                cell.set_symbol("│");
                cell.set_style(Style::default().fg(border_color));
            }
        }

        // Editor content area: 1 row top padding, offset by 2 for border + space
        let content_area = Rect {
            x: area.x + 2,
            y: area.y + 1,
            width: area.width.saturating_sub(3),
            height: area.height.saturating_sub(4), // Reserve: 1 top pad + 2 bottom (variant + hints) + 1 gap
        };

        // Render text content with inline block cursor, soft-wrap, and scroll
        let (cursor_row, cursor_col) = self.state.cursor_pos();
        let all_lines = self.state.textarea.lines();
        let max_w = content_area.width as usize;
        let visible_h = content_area.height as usize;

        let text_style = Style::default().fg(self.theme.fg).bg(self.theme.input_bg);
        let inverted_style = Style::default().fg(self.theme.input_bg).bg(self.theme.fg);
        let is_empty = self.state.is_empty();

        // Build visual lines with soft-wrapping
        let mut visual_lines: Vec<Line> = Vec::new();
        let mut visual_cursor_row: usize = 0;

        for (i, logical_line) in all_lines.iter().enumerate() {
            let chars: Vec<char> = logical_line.chars().collect();
            let is_cursor_line = i == cursor_row && self.focused;

            // Placeholder for empty editor
            if i == 0 && logical_line.is_empty() && is_empty {
                if is_cursor_line {
                    visual_cursor_row = visual_lines.len();
                }
                if self.focused {
                    visual_lines.push(Line::from(vec![
                        Span::styled("█", text_style),
                        Span::styled(
                            " Type a message...",
                            Style::default()
                                .fg(self.theme.muted)
                                .bg(self.theme.input_bg),
                        ),
                    ]));
                } else {
                    visual_lines.push(Line::from(Span::styled(
                        "Type a message...",
                        Style::default()
                            .fg(self.theme.muted)
                            .bg(self.theme.input_bg),
                    )));
                }
                continue;
            }

            if max_w == 0 {
                if is_cursor_line {
                    visual_cursor_row = visual_lines.len();
                }
                visual_lines.push(Line::from(Span::styled(
                    logical_line.to_string(),
                    text_style,
                )));
                continue;
            }

            // Empty logical line
            if chars.is_empty() {
                if is_cursor_line {
                    visual_cursor_row = visual_lines.len();
                    visual_lines.push(Line::from(vec![Span::styled(" ", inverted_style)]));
                } else {
                    visual_lines.push(Line::from(Span::styled("", text_style)));
                }
                continue;
            }

            // Soft-wrap: split logical line by display width (CJK chars = 2 cols)
            let mut pos = 0;
            let mut last_chunk_full = false;
            while pos < chars.len() {
                // Accumulate chars until display width exceeds max_w
                let mut col_w = 0usize;
                let mut end = pos;
                while end < chars.len() {
                    let cw = unicode_width::UnicodeWidthChar::width(chars[end]).unwrap_or(1);
                    if col_w + cw > max_w {
                        break;
                    }
                    col_w += cw;
                    end += 1;
                }
                if end == pos {
                    end = pos + 1; // at least one char to avoid infinite loop
                }
                last_chunk_full = col_w == max_w;
                let chunk = &chars[pos..end];

                // Determine if cursor is in this chunk
                let mut has_cursor = false;
                let mut vcol = 0;
                if is_cursor_line {
                    if cursor_col >= pos && cursor_col < end {
                        has_cursor = true;
                        vcol = cursor_col - pos;
                    } else if cursor_col == end && end == chars.len() && col_w < max_w {
                        // Cursor at end of line, fits in last non-full chunk
                        has_cursor = true;
                        vcol = chunk.len();
                    }
                }

                if has_cursor {
                    visual_cursor_row = visual_lines.len();
                    if vcol >= chunk.len() {
                        let before: String = chunk.iter().collect();
                        visual_lines.push(Line::from(vec![
                            Span::styled(before, text_style),
                            Span::styled(" ", inverted_style),
                        ]));
                    } else {
                        let before: String = chunk[..vcol].iter().collect();
                        let cursor_char: String = chunk[vcol..vcol + 1].iter().collect();
                        let rest: String = chunk[vcol + 1..].iter().collect();
                        visual_lines.push(Line::from(vec![
                            Span::styled(before, text_style),
                            Span::styled(cursor_char, inverted_style),
                            Span::styled(rest, text_style),
                        ]));
                    }
                } else {
                    let chunk_text: String = chunk.iter().collect();
                    visual_lines.push(Line::from(Span::styled(chunk_text, text_style)));
                }
                pos = end;
            }

            // Cursor at end of line where last chunk exactly filled the width
            if is_cursor_line && cursor_col == chars.len() && !chars.is_empty() && last_chunk_full {
                visual_cursor_row = visual_lines.len();
                visual_lines.push(Line::from(vec![Span::styled(" ", inverted_style)]));
            }
        }

        // Scroll to keep visual cursor row visible
        let total_visual = visual_lines.len();
        let scroll_start = if total_visual <= visible_h || visual_cursor_row < visible_h {
            0
        } else {
            visual_cursor_row
                .saturating_sub(visible_h - 1)
                .min(total_visual.saturating_sub(visible_h))
        };

        let visible_end = total_visual.min(scroll_start + visible_h);
        let visible: Vec<Line> = visual_lines
            .into_iter()
            .skip(scroll_start)
            .take(visible_end - scroll_start)
            .collect();

        let paragraph = Paragraph::new(visible);
        paragraph.render(content_area, buf);

        // Attachment chips row (top of editor)
        if !self.attachments.is_empty() {
            let attach_area = Rect {
                x: area.x + 2,
                y: area.y,
                width: area.width.saturating_sub(3),
                height: 1,
            };
            let chip_style = Style::default().fg(self.theme.bg).bg(self.theme.accent);
            let chip_selected = Style::default()
                .fg(self.theme.bg)
                .bg(self.theme.accent)
                .add_modifier(Modifier::BOLD);
            let mut spans: Vec<Span> = Vec::new();
            let count = self.attachments.len();
            for (i, att) in self.attachments.iter().enumerate() {
                let label = match &att.kind {
                    AttachmentKind::Image { .. } => format!(" [Image #{}] ", i + 1),
                    AttachmentKind::TextFile { .. } => format!(" [File #{}] ", i + 1),
                };
                let is_last = i == count - 1;
                let style = if self.attachment_selected && is_last {
                    chip_selected
                } else {
                    chip_style
                };
                spans.push(Span::styled(label, style));
                spans.push(Span::styled(" ", Style::default().bg(self.theme.input_bg)));
            }
            // Hint text
            if self.attachment_selected {
                spans.push(Span::styled(
                    "Delete to remove · Esc to cancel",
                    Style::default().fg(self.theme.muted),
                ));
            } else {
                spans.push(Span::styled(
                    "(↑ to select)",
                    Style::default().fg(self.theme.muted),
                ));
            }
            Paragraph::new(Line::from(spans)).render(attach_area, buf);
        }

        // Variant row (like "Build Big Pickle OpenCode Zen")
        let variant_y = area.y + area.height.saturating_sub(3);
        if variant_y < area.y + area.height {
            let variant_area = Rect {
                x: area.x + 2,
                y: variant_y,
                width: area.width.saturating_sub(3),
                height: 1,
            };
            let variant_line = Line::from(vec![
                Span::styled(
                    "Claude",
                    Style::default()
                        .fg(self.theme.accent)
                        .bg(self.theme.input_bg)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "  Juglans  Custom",
                    Style::default()
                        .fg(self.theme.muted)
                        .bg(self.theme.input_bg),
                ),
            ]);
            Paragraph::new(vec![variant_line]).render(variant_area, buf);
        }

        // Hints row at the very last row (no bg)
        let hints_y = area.y + area.height.saturating_sub(1);
        if hints_y < area.y + area.height {
            let hint_area = Rect {
                x: area.x,
                y: hints_y,
                width: area.width,
                height: 1,
            };

            // Build hints spanning the width (no background)
            let left_hints = if self.streaming {
                vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(
                        "esc",
                        Style::default()
                            .fg(self.theme.fg)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" stop generating", Style::default().fg(self.theme.muted)),
                ]
            } else {
                vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(
                        "esc",
                        Style::default()
                            .fg(self.theme.fg)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(" interrupt", Style::default().fg(self.theme.muted)),
                ]
            };

            let right_hints = vec![
                Span::styled(
                    "ctrl+a",
                    Style::default()
                        .fg(self.theme.fg)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" attach  ", Style::default().fg(self.theme.muted)),
                Span::styled(
                    "ctrl+t",
                    Style::default()
                        .fg(self.theme.fg)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" variants  ", Style::default().fg(self.theme.muted)),
                Span::styled(
                    "tab",
                    Style::default()
                        .fg(self.theme.fg)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" agents  ", Style::default().fg(self.theme.muted)),
                Span::styled(
                    "ctrl+p",
                    Style::default()
                        .fg(self.theme.fg)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" commands", Style::default().fg(self.theme.muted)),
            ];

            let left_w: usize = left_hints.iter().map(|s| s.width()).sum();
            let right_w: usize = right_hints.iter().map(|s| s.width()).sum();
            let gap = (area.width as usize).saturating_sub(left_w + right_w);

            let mut all = left_hints;
            all.push(Span::styled(" ".repeat(gap), Style::default()));
            all.extend(right_hints);

            Paragraph::new(Line::from(all)).render(hint_area, buf);
        }
    }
}
