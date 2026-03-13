use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Widget},
};
use std::path::PathBuf;

use super::claude_code::floor_char_boundary;
use super::messages::{format_file_size, read_directory_entries, FileEntry};
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
    LinkPreview {
        text: String,
        url: String,
        selected: usize,
    },
    FilePicker {
        current_dir: PathBuf,
        entries: Vec<FileEntry>,
        selected: usize,
        scroll_offset: usize,
        filter: String,
    },
    AgentPicker {
        agents: Vec<(String, PathBuf)>,
        selected: usize,
        scroll_offset: usize,
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
                "grok-3".to_string(),
                "grok-3-mini".to_string(),
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
            Dialog::LinkPreview { url, selected, .. } => {
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
                        let result = match *selected {
                            0 => format!("open_url:{}", url),
                            1 => format!("copy_url:{}", url),
                            _ => format!("open_url:{}", url),
                        };
                        (true, Some(result))
                    }
                    KeyCode::Esc => (true, None),
                    _ => (false, None),
                }
            }
            Dialog::AgentPicker {
                agents,
                selected,
                scroll_offset,
            } => match key.code {
                KeyCode::Up => {
                    if *selected > 0 {
                        *selected -= 1;
                    }
                    if *selected < *scroll_offset {
                        *scroll_offset = *selected;
                    }
                    (false, None)
                }
                KeyCode::Down => {
                    if *selected < agents.len().saturating_sub(1) {
                        *selected += 1;
                    }
                    if *selected >= *scroll_offset + 14 {
                        *scroll_offset = selected.saturating_sub(13);
                    }
                    (false, None)
                }
                KeyCode::Enter => {
                    if let Some((_, path)) = agents.get(*selected) {
                        (true, Some(path.to_string_lossy().to_string()))
                    } else {
                        (false, None)
                    }
                }
                KeyCode::Esc => (true, None),
                _ => (false, None),
            },
            Dialog::FilePicker {
                current_dir,
                entries,
                selected,
                scroll_offset,
                filter,
            } => {
                let filtered = filtered_entries(entries, filter);
                match key.code {
                    KeyCode::Up => {
                        if *selected > 0 {
                            *selected -= 1;
                        }
                        if *selected < *scroll_offset {
                            *scroll_offset = *selected;
                        }
                        (false, None)
                    }
                    KeyCode::Down => {
                        if *selected < filtered.len().saturating_sub(1) {
                            *selected += 1;
                        }
                        // Scroll if needed (visible height ~14)
                        if *selected >= *scroll_offset + 14 {
                            *scroll_offset = selected.saturating_sub(13);
                        }
                        (false, None)
                    }
                    KeyCode::Enter => {
                        if let Some(entry) = filtered.get(*selected) {
                            if entry.is_dir {
                                let new_dir = if entry.name == ".." {
                                    current_dir.parent().unwrap_or(current_dir).to_path_buf()
                                } else {
                                    current_dir.join(&entry.name)
                                };
                                *entries = read_directory_entries(&new_dir);
                                *current_dir = new_dir;
                                *selected = 0;
                                *scroll_offset = 0;
                                *filter = String::new();
                                (false, None)
                            } else {
                                let full_path = current_dir.join(&entry.name);
                                (true, Some(full_path.to_string_lossy().to_string()))
                            }
                        } else {
                            (false, None)
                        }
                    }
                    KeyCode::Backspace => {
                        if filter.is_empty() {
                            if let Some(parent) = current_dir.parent() {
                                let parent = parent.to_path_buf();
                                *entries = read_directory_entries(&parent);
                                *current_dir = parent;
                                *selected = 0;
                                *scroll_offset = 0;
                            }
                        } else {
                            filter.pop();
                            *selected = 0;
                            *scroll_offset = 0;
                        }
                        (false, None)
                    }
                    KeyCode::Char(c) => {
                        filter.push(c);
                        *selected = 0;
                        *scroll_offset = 0;
                        (false, None)
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
            Dialog::Help => (60, 20),
            Dialog::Quit => (40, 5),
            Dialog::ModelPicker { models, .. } => (50, models.len() as u16 + 4),
            Dialog::_PermissionRequest {
                ref _input_display, ..
            } => {
                let input_lines = _input_display.lines().count().min(10) as u16;
                (64, 6 + input_lines)
            }
            Dialog::MessageActions { .. } => (44, 7),
            Dialog::LinkPreview { ref url, .. } => {
                let url_w = url.len().min(70) + 4;
                (url_w.max(44) as u16, 9)
            }
            Dialog::FilePicker {
                ref entries,
                ref filter,
                ..
            } => {
                let count = filtered_entries(entries, filter).len();
                let h = (count as u16 + 6).clamp(10, 22);
                (62, h)
            }
            Dialog::AgentPicker { ref agents, .. } => {
                let h = (agents.len() as u16 + 4).clamp(6, 20);
                (54, h)
            }
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
            Dialog::LinkPreview {
                ref text,
                ref url,
                selected,
            } => self.render_link_preview(dialog_area, buf, text, url, *selected),
            Dialog::FilePicker {
                ref current_dir,
                ref entries,
                selected,
                scroll_offset,
                ref filter,
            } => self.render_file_picker(
                dialog_area,
                buf,
                current_dir,
                entries,
                *selected,
                *scroll_offset,
                filter,
            ),
            Dialog::AgentPicker {
                ref agents,
                selected,
                scroll_offset,
            } => self.render_agent_picker(dialog_area, buf, agents, *selected, *scroll_offset),
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
            ("Ctrl+A", "Attach file"),
            ("Ctrl+X", "Remove last attachment"),
            ("Ctrl+C", "Cancel / Quit"),
            ("Ctrl+H", "Toggle help"),
            ("Ctrl+O", "Select model"),
            ("Ctrl+L", "Clear messages"),
            ("Page Up/Down", "Scroll messages"),
            ("Ctrl+U/D", "Half-page scroll"),
            ("Ctrl+Y", "Copy last response"),
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
    fn render_link_preview(
        &self,
        area: Rect,
        buf: &mut Buffer,
        text: &str,
        url: &str,
        selected: usize,
    ) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.accent))
            .title(Span::styled(
                " Link ",
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ));

        let inner_w = area.width.saturating_sub(2) as usize;
        let display_text = if text.len() > inner_w - 2 {
            let end = floor_char_boundary(text, inner_w - 5);
            format!("  {}...", &text[..end])
        } else {
            format!("  {}", text)
        };
        let display_url = if url.len() > inner_w - 2 {
            let end = floor_char_boundary(url, inner_w - 5);
            format!("  {}...", &url[..end])
        } else {
            format!("  {}", url)
        };

        let actions = ["Open in browser", "Copy URL"];
        let mut lines = vec![
            Line::from(Span::styled(
                display_text,
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                display_url,
                Style::default().fg(self.theme.muted),
            )),
            Line::raw(""),
        ];
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

    #[allow(clippy::too_many_arguments)]
    fn render_file_picker(
        &self,
        area: Rect,
        buf: &mut Buffer,
        current_dir: &std::path::Path,
        entries: &[FileEntry],
        selected: usize,
        scroll_offset: usize,
        filter: &str,
    ) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.accent))
            .title(Span::styled(
                " Attach File ",
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ));

        let inner = block.inner(area);
        block.render(area, buf);

        if inner.height < 4 || inner.width < 10 {
            return;
        }

        let inner_w = inner.width as usize;

        // Line 0: current directory
        let dir_str = current_dir.display().to_string();
        let dir_display = if dir_str.len() > inner_w - 2 {
            let end = floor_char_boundary(&dir_str, inner_w - 5);
            format!(" ...{}", &dir_str[dir_str.len() - end..])
        } else {
            format!(" {}", dir_str)
        };
        let dir_line = Line::from(Span::styled(
            dir_display,
            Style::default().fg(self.theme.muted),
        ));
        Paragraph::new(dir_line).render(
            Rect {
                x: inner.x,
                y: inner.y,
                width: inner.width,
                height: 1,
            },
            buf,
        );

        // Line 1: filter input
        let filter_display = if filter.is_empty() {
            " Search: ".to_string()
        } else {
            format!(" Search: {}", filter)
        };
        let filter_line = Line::from(vec![
            Span::styled(filter_display, Style::default().fg(self.theme.fg)),
            Span::styled("█", Style::default().fg(self.theme.accent)),
        ]);
        Paragraph::new(filter_line).render(
            Rect {
                x: inner.x,
                y: inner.y + 1,
                width: inner.width,
                height: 1,
            },
            buf,
        );

        // Lines 2..h-2: file list
        let list_height = inner.height.saturating_sub(4) as usize;
        let filtered = filtered_entries(entries, filter);

        for (i, entry) in filtered
            .iter()
            .skip(scroll_offset)
            .take(list_height)
            .enumerate()
        {
            let idx = scroll_offset + i;
            let is_sel = idx == selected;
            let prefix = if is_sel { " > " } else { "   " };
            let icon = if entry.is_dir { "/" } else { " " };
            let size_str = if entry.is_dir {
                String::new()
            } else {
                format_file_size(entry.size)
            };

            let name_w = inner_w.saturating_sub(prefix.len() + 1 + size_str.len() + 1);
            let name = if entry.name.len() > name_w {
                let end = floor_char_boundary(&entry.name, name_w.saturating_sub(3));
                format!("{}...", &entry.name[..end])
            } else {
                entry.name.clone()
            };
            let pad = name_w.saturating_sub(name.len());

            let style = if is_sel {
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(self.theme.fg)
            };

            let line = Line::from(vec![
                Span::styled(prefix.to_string(), style),
                Span::styled(format!("{}{}", name, icon), style),
                Span::styled(" ".repeat(pad), Style::default()),
                Span::styled(size_str, Style::default().fg(self.theme.muted)),
            ]);

            Paragraph::new(line).render(
                Rect {
                    x: inner.x,
                    y: inner.y + 2 + i as u16,
                    width: inner.width,
                    height: 1,
                },
                buf,
            );
        }

        // Empty state
        if filtered.is_empty() {
            let empty_line = Line::from(Span::styled(
                "   No files found",
                Style::default().fg(self.theme.muted),
            ));
            Paragraph::new(empty_line).render(
                Rect {
                    x: inner.x,
                    y: inner.y + 2,
                    width: inner.width,
                    height: 1,
                },
                buf,
            );
        }

        // Footer hints
        let hints_y = inner.y + inner.height.saturating_sub(1);
        let hints_line = Line::from(Span::styled(
            " ↑↓ navigate │ Enter select │ Bksp parent │ Esc cancel",
            Style::default().fg(self.theme.muted),
        ));
        Paragraph::new(hints_line).render(
            Rect {
                x: inner.x,
                y: hints_y,
                width: inner.width,
                height: 1,
            },
            buf,
        );
    }

    fn render_agent_picker(
        &self,
        area: Rect,
        buf: &mut Buffer,
        agents: &[(String, PathBuf)],
        selected: usize,
        scroll_offset: usize,
    ) {
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(self.theme.accent))
            .title(Span::styled(
                " Select Agent ",
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD),
            ));

        let inner = block.inner(area);
        block.render(area, buf);

        if inner.height < 3 || inner.width < 10 {
            return;
        }

        let list_height = inner.height.saturating_sub(2) as usize;

        for (i, (name, path)) in agents
            .iter()
            .skip(scroll_offset)
            .take(list_height)
            .enumerate()
        {
            let idx = scroll_offset + i;
            let is_sel = idx == selected;
            let prefix = if is_sel { " ▸ " } else { "   " };
            let style = if is_sel {
                Style::default()
                    .fg(self.theme.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(self.theme.fg)
            };

            let fname = path
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_default();
            let display = if name.is_empty() || name == &fname {
                fname
            } else {
                format!("{} ({})", name, fname)
            };

            let line = Line::from(Span::styled(format!("{}{}", prefix, display), style));
            Paragraph::new(line).render(
                Rect {
                    x: inner.x,
                    y: inner.y + i as u16,
                    width: inner.width,
                    height: 1,
                },
                buf,
            );
        }

        // Footer
        let hints_y = inner.y + inner.height.saturating_sub(1);
        let hints_line = Line::from(Span::styled(
            " ↑↓ navigate │ Enter select │ Esc cancel",
            Style::default().fg(self.theme.muted),
        ));
        Paragraph::new(hints_line).render(
            Rect {
                x: inner.x,
                y: hints_y,
                width: inner.width,
                height: 1,
            },
            buf,
        );
    }
}

/// Scan a directory for .jgagent files and return (name, path) pairs
pub fn scan_agents(dir: &std::path::Path) -> Vec<(String, PathBuf)> {
    let mut results = Vec::new();
    let pattern = dir.join("**/*.jgagent");
    let pat_str = pattern.to_string_lossy();
    if let Ok(paths) = glob::glob(&pat_str) {
        for entry in paths.flatten() {
            let name = std::fs::read_to_string(&entry)
                .ok()
                .and_then(|content| {
                    // Extract name field from .jgagent
                    for line in content.lines() {
                        let trimmed = line.trim();
                        if let Some(rest) = trimmed.strip_prefix("name:") {
                            let val = rest.trim().trim_matches('"');
                            if !val.is_empty() {
                                return Some(val.to_string());
                            }
                        }
                    }
                    None
                })
                .unwrap_or_default();
            results.push((name, entry));
        }
    }
    results.sort_by(|a, b| a.1.cmp(&b.1));
    results
}

fn filtered_entries<'a>(entries: &'a [FileEntry], filter: &str) -> Vec<&'a FileEntry> {
    if filter.is_empty() {
        entries.iter().collect()
    } else {
        let lower = filter.to_lowercase();
        entries
            .iter()
            .filter(|e| e.name.to_lowercase().contains(&lower) || e.name == "..")
            .collect()
    }
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}
