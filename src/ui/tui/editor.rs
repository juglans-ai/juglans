use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};
use tui_textarea::TextArea;

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
    pub scroll_offset: usize,
    pub streaming: bool,
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

        // Render text content with inline block cursor and scroll
        let (cursor_row, cursor_col) = self.state.cursor_pos();
        let all_lines = self.state.textarea.lines();
        let total_lines = all_lines.len();
        let visible_h = content_area.height as usize;

        // Calculate scroll offset to keep cursor visible
        let scroll_start = if self.scroll_offset > 0 {
            self.scroll_offset
                .min(total_lines.saturating_sub(visible_h))
        } else if total_lines <= visible_h {
            0
        } else if cursor_row >= visible_h {
            cursor_row - visible_h + 1
        } else {
            0
        };

        let visible_range = scroll_start..total_lines.min(scroll_start + visible_h);

        let lines: Vec<Line> = visible_range
            .map(|i| {
                let line = &all_lines[i];
                if i == 0 && line.is_empty() && self.state.is_empty() {
                    // Empty state: show cursor then placeholder
                    if self.focused {
                        Line::from(vec![
                            Span::styled(
                                "█",
                                Style::default().fg(self.theme.fg).bg(self.theme.input_bg),
                            ),
                            Span::styled(
                                " Type a message...",
                                Style::default()
                                    .fg(self.theme.muted)
                                    .bg(self.theme.input_bg),
                            ),
                        ])
                    } else {
                        Line::from(Span::styled(
                            "Type a message...",
                            Style::default()
                                .fg(self.theme.muted)
                                .bg(self.theme.input_bg),
                        ))
                    }
                } else if i == cursor_row && self.focused {
                    // Line with cursor: split text around cursor position
                    let chars: Vec<char> = line.chars().collect();
                    let before: String = chars[..cursor_col.min(chars.len())].iter().collect();
                    let _after: String = chars[cursor_col.min(chars.len())..].iter().collect();
                    let text_style = Style::default().fg(self.theme.fg).bg(self.theme.input_bg);
                    let inverted_style = Style::default().fg(self.theme.input_bg).bg(self.theme.fg);

                    if cursor_col >= chars.len() {
                        Line::from(vec![
                            Span::styled(before, text_style),
                            Span::styled(" ", inverted_style),
                        ])
                    } else {
                        let cursor_char: String =
                            chars[cursor_col..cursor_col + 1].iter().collect();
                        let rest: String = chars[cursor_col + 1..].iter().collect();
                        Line::from(vec![
                            Span::styled(before, text_style),
                            Span::styled(cursor_char, inverted_style),
                            Span::styled(rest, text_style),
                        ])
                    }
                } else {
                    Line::from(Span::styled(
                        line.as_str().to_string(),
                        Style::default().fg(self.theme.fg).bg(self.theme.input_bg),
                    ))
                }
            })
            .collect();

        let paragraph = Paragraph::new(lines);
        paragraph.render(content_area, buf);

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
