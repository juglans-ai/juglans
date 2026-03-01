use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget},
};

use super::claude_code::floor_char_boundary;
use super::theme::Theme;

#[derive(Debug, Clone)]
pub enum Dialog {
    Help,
    Quit,
    ModelPicker {
        models: Vec<String>,
        selected: usize,
    },
    _PermissionRequest {
        _request_id: String,
        _tool_name: String,
        _input_display: String,
    },
    MessageActions {
        selected: usize,
    },
}

impl Dialog {
    pub fn model_picker() -> Self {
        Dialog::ModelPicker {
            models: vec![
                "claude-sonnet-4-20250514".to_string(),
                "claude-opus-4-20250514".to_string(),
                "claude-haiku-4-5-20251001".to_string(),
                "gpt-4o".to_string(),
                "gpt-4o-mini".to_string(),
            ],
            selected: 0,
        }
    }

    /// Handle key event. Returns true if dialog should close, and optionally a selected model.
    pub fn handle_key(&mut self, key: KeyEvent) -> (bool, Option<String>) {
        match self {
            Dialog::Help => match key.code {
                KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => (true, None),
                _ => (false, None),
            },
            Dialog::Quit => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    (true, Some("quit".to_string()))
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => (true, None),
                _ => (false, None),
            },
            Dialog::ModelPicker { models, selected } => match key.code {
                KeyCode::Up | KeyCode::Char('k') => {
                    if *selected > 0 {
                        *selected -= 1;
                    }
                    (false, None)
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if *selected < models.len() - 1 {
                        *selected += 1;
                    }
                    (false, None)
                }
                KeyCode::Enter => {
                    let model = models[*selected].clone();
                    (true, Some(model))
                }
                KeyCode::Esc => (true, None),
                _ => (false, None),
            },
            Dialog::_PermissionRequest { _request_id, .. } => match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => {
                    (true, Some(format!("allow:{}", _request_id)))
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    (true, Some(format!("deny:{}", _request_id)))
                }
                _ => (false, None),
            },
            Dialog::MessageActions { selected } => {
                const ACTION_COUNT: usize = 2;
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        if *selected > 0 {
                            *selected -= 1;
                        }
                        (false, None)
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if *selected < ACTION_COUNT - 1 {
                            *selected += 1;
                        }
                        (false, None)
                    }
                    KeyCode::Enter => {
                        let action = match *selected {
                            0 => "copy_last",
                            1 => "copy_all",
                            _ => "copy_last",
                        };
                        (true, Some(action.to_string()))
                    }
                    KeyCode::Esc => (true, None),
                    _ => (false, None),
                }
            }
        }
    }
}

pub struct DialogWidget<'a> {
    pub dialog: &'a Dialog,
    pub theme: &'a Theme,
}

impl<'a> Widget for DialogWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Calculate centered dialog area
        let (width, height) = match self.dialog {
            Dialog::Help => (60, 18),
            Dialog::Quit => (40, 5),
            Dialog::ModelPicker { models, .. } => (50, models.len() as u16 + 4),
            Dialog::_PermissionRequest {
                ref _input_display, ..
            } => {
                let input_lines = _input_display.lines().count().min(10) as u16;
                (64, 6 + input_lines)
            }
            Dialog::MessageActions { .. } => (44, 7),
        };

        let dialog_area = centered_rect(width, height, area);

        // Clear background
        Clear.render(dialog_area, buf);

        match self.dialog {
            Dialog::Help => self.render_help(dialog_area, buf),
            Dialog::Quit => self.render_quit(dialog_area, buf),
            Dialog::ModelPicker { models, selected } => {
                self.render_model_picker(dialog_area, buf, models, *selected)
            }
            Dialog::_PermissionRequest {
                ref _tool_name,
                ref _input_display,
                ..
            } => self.render_permission_request(dialog_area, buf, _tool_name, _input_display),
            Dialog::MessageActions { selected } => {
                self.render_message_actions(dialog_area, buf, *selected)
            }
        }
    }
}

impl<'a> DialogWidget<'a> {
    fn render_help(&self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.accent))
            .title(Span::styled(
                " Keyboard Shortcuts ",
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ));

        let shortcuts = vec![
            ("Enter", "Send message"),
            ("Shift+Enter", "New line"),
            ("Ctrl+C", "Cancel / Quit"),
            ("Ctrl+H", "Toggle help"),
            ("Ctrl+O", "Select model"),
            ("Ctrl+L", "Clear messages"),
            ("Page Up/Down", "Scroll messages"),
            ("Ctrl+U/D", "Half-page scroll"),
            ("Ctrl+Y", "Copy last response"),
            ("Shift+Drag", "Select text (terminal)"),
            ("Esc", "Close dialog"),
            ("", ""),
            ("Press Esc to close", ""),
        ];

        let lines: Vec<Line> = shortcuts
            .iter()
            .map(|(key, desc)| {
                if key.is_empty() {
                    Line::raw("")
                } else if desc.is_empty() {
                    Line::from(Span::styled(
                        key.to_string(),
                        Style::default().fg(self.theme.muted),
                    ))
                } else {
                    Line::from(vec![
                        Span::styled(
                            format!("  {:16}", key),
                            Style::default().fg(self.theme.accent),
                        ),
                        Span::raw(desc.to_string()),
                    ])
                }
            })
            .collect();

        let paragraph = Paragraph::new(lines).block(block);
        paragraph.render(area, buf);
    }

    fn render_quit(&self, area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.tool_err))
            .title(Span::styled(
                " Quit ",
                Style::default()
                    .fg(self.theme.tool_err)
                    .add_modifier(Modifier::BOLD),
            ));

        let lines = vec![
            Line::raw(""),
            Line::from(vec![
                Span::raw("  Are you sure? "),
                Span::styled("[Y]", Style::default().fg(self.theme.accent)),
                Span::raw("es / "),
                Span::styled("[N]", Style::default().fg(self.theme.accent)),
                Span::raw("o"),
            ]),
        ];

        let paragraph = Paragraph::new(lines).block(block);
        paragraph.render(area, buf);
    }

    fn render_model_picker(
        &self,
        area: Rect,
        buf: &mut Buffer,
        models: &[String],
        selected: usize,
    ) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.accent))
            .title(Span::styled(
                " Select Model ",
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ));

        let mut lines = vec![Line::raw("")];
        for (i, model) in models.iter().enumerate() {
            let prefix = if i == selected { " ▸ " } else { "   " };
            let style = if i == selected {
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(self.theme.fg)
            };
            lines.push(Line::from(Span::styled(
                format!("{}{}", prefix, model),
                style,
            )));
        }
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            "  ↑↓ navigate │ Enter select │ Esc cancel",
            Style::default().fg(self.theme.muted),
        )));

        let paragraph = Paragraph::new(lines).block(block);
        paragraph.render(area, buf);
    }

    fn render_permission_request(
        &self,
        area: Rect,
        buf: &mut Buffer,
        tool_name: &str,
        input_display: &str,
    ) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.tool_pending))
            .title(Span::styled(
                " Permission Request ",
                Style::default()
                    .fg(self.theme.tool_pending)
                    .add_modifier(Modifier::BOLD),
            ));

        let mut lines = vec![
            Line::raw(""),
            Line::from(vec![
                Span::styled("  Tool: ", Style::default().fg(self.theme.muted)),
                Span::styled(
                    tool_name.to_string(),
                    Style::default()
                        .fg(self.theme.accent)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
            Line::raw(""),
        ];

        // Show input params (truncated to 10 lines)
        for (i, line) in input_display.lines().enumerate() {
            if i >= 10 {
                lines.push(Line::from(Span::styled(
                    "  ...",
                    Style::default().fg(self.theme.muted),
                )));
                break;
            }
            let display = if line.len() > 56 {
                let end = floor_char_boundary(line, 56);
                format!("  {}...", &line[..end])
            } else {
                format!("  {}", line)
            };
            lines.push(Line::from(Span::styled(
                display,
                Style::default().fg(self.theme.fg),
            )));
        }

        lines.push(Line::raw(""));
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled("[Y]", Style::default().fg(self.theme.accent)),
            Span::raw("es allow / "),
            Span::styled("[N]", Style::default().fg(self.theme.tool_err)),
            Span::raw("o deny"),
        ]));

        let paragraph = Paragraph::new(lines).block(block);
        paragraph.render(area, buf);
    }

    fn render_message_actions(&self, area: Rect, buf: &mut Buffer, selected: usize) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.accent))
            .title(Span::styled(
                " Message Actions ",
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ));

        let actions = ["Copy last response", "Copy all messages"];
        let mut lines = vec![Line::raw("")];
        for (i, action) in actions.iter().enumerate() {
            let prefix = if i == selected { " ▸ " } else { "   " };
            let style = if i == selected {
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(self.theme.fg)
            };
            lines.push(Line::from(Span::styled(
                format!("{}{}", prefix, action),
                style,
            )));
        }
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            "  ↑↓ navigate │ Enter select │ Esc cancel",
            Style::default().fg(self.theme.muted),
        )));

        let paragraph = Paragraph::new(lines).block(block);
        paragraph.render(area, buf);
    }
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}
