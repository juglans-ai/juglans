use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget, Wrap},
};
use std::time::{Duration, Instant};

use super::theme::Theme;

#[derive(Debug, Clone)]
pub enum ToolStatus {
    InProgress,
    Completed,
    Error,
}

#[derive(Debug, Clone)]
pub enum ChatMessage {
    User {
        content: String,
    },
    Assistant {
        thinking: Option<String>,
        content: String,
        model: String,
        elapsed: Option<Duration>,
    },
    ToolCall {
        name: String,
        status: ToolStatus,
        params: String,
        response: Option<String>,
    },
}

impl ChatMessage {
    pub fn render_lines(&self, width: u16, theme: &Theme) -> Vec<Line<'static>> {
        match self {
            ChatMessage::User { content } => {
                let mut lines = Vec::new();
                // User message with blue left border on dark bg (like OpenCode screenshot)
                let user_bg = Style::default().fg(theme.fg).bg(theme.input_bg);
                for line in content.lines() {
                    lines.push(Line::from(vec![
                        Span::styled("  ", user_bg),
                        Span::styled(line.to_string(), user_bg),
                    ]));
                }
                lines.push(Line::from(Span::styled("", Style::default())));
                lines
            }
            ChatMessage::Assistant {
                thinking,
                content,
                model,
                elapsed,
            } => {
                let mut lines = Vec::new();

                // Thinking block (italic, golden color, like OpenCode screenshot 3)
                if let Some(think) = thinking {
                    lines.push(Line::from(vec![
                        Span::styled(
                            "  Thinking: ",
                            Style::default()
                                .fg(theme.thinking)
                                .add_modifier(Modifier::ITALIC),
                        ),
                        Span::styled(
                            think.to_string(),
                            Style::default()
                                .fg(theme.thinking)
                                .add_modifier(Modifier::ITALIC),
                        ),
                    ]));
                    lines.push(Line::from(Span::styled("", Style::default())));
                }

                // Content — full markdown rendering
                if !content.is_empty() {
                    let md_lines = super::markdown::render_markdown(content, theme);
                    lines.extend(md_lines);
                }
                lines.push(Line::from(Span::styled("", Style::default())));

                // Model + elapsed line — only show when generation is complete
                if let Some(d) = elapsed {
                    let elapsed_str = format_duration(*d);
                    let short_model = model.split('-').next().unwrap_or(model).to_string();
                    lines.push(Line::from(vec![
                        Span::styled("  ", Style::default()),
                        Span::styled("■ ", Style::default().fg(theme.accent)),
                        Span::styled(
                            short_model,
                            Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(
                            format!(" · {} · {}", model, elapsed_str),
                            Style::default().fg(theme.muted),
                        ),
                    ]));
                    lines.push(Line::from(Span::styled("", Style::default())));
                }
                lines
            }
            ChatMessage::ToolCall {
                name,
                status,
                params,
                response,
            } => {
                let (status_text, status_color) = match status {
                    ToolStatus::InProgress => ("running", theme.tool_pending),
                    ToolStatus::Completed => ("completed", theme.tool_ok),
                    ToolStatus::Error => ("error", theme.tool_err),
                };

                let w = width as usize;
                let header = format!("  ┌ {} ", name);
                let pad_len = w.saturating_sub(header.len() + status_text.len() + 3);
                let pad = "─".repeat(pad_len);

                let mut lines = vec![Line::from(vec![
                    Span::styled(header, Style::default().fg(theme.border)),
                    Span::styled(pad, Style::default().fg(theme.border)),
                    Span::styled(
                        format!(" {} ", status_text),
                        Style::default().fg(status_color),
                    ),
                    Span::styled("┐", Style::default().fg(theme.border)),
                ])];

                for line in params.lines() {
                    lines.push(Line::from(vec![
                        Span::styled("  │ ", Style::default().fg(theme.border)),
                        Span::styled(line.to_string(), Style::default().fg(theme.muted)),
                    ]));
                }

                if let Some(resp) = response {
                    lines.push(Line::from(vec![
                        Span::styled("  │ ", Style::default().fg(theme.border)),
                        Span::styled(
                            "─".repeat(w.saturating_sub(6)),
                            Style::default().fg(theme.border),
                        ),
                    ]));
                    for line in resp.lines().take(5) {
                        lines.push(Line::from(vec![
                            Span::styled("  │ ", Style::default().fg(theme.border)),
                            Span::styled(line.to_string(), Style::default().fg(theme.fg)),
                        ]));
                    }
                    if resp.lines().count() > 5 {
                        lines.push(Line::from(vec![
                            Span::styled("  │ ", Style::default().fg(theme.border)),
                            Span::styled(
                                format!("... ({} more lines)", resp.lines().count() - 5),
                                Style::default().fg(theme.muted),
                            ),
                        ]));
                    }
                }

                lines.push(Line::from(vec![Span::styled(
                    format!("  └{}┘", "─".repeat(w.saturating_sub(4))),
                    Style::default().fg(theme.border),
                )]));
                lines.push(Line::from(Span::styled("", Style::default())));
                lines
            }
        }
    }
}

pub const SPINNER: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

fn format_duration(d: Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{:.1}s", d.as_secs_f64())
    } else {
        format!("{}m {}s", secs / 60, secs % 60)
    }
}

pub struct MessagesWidget<'a> {
    pub messages: &'a [ChatMessage],
    pub scroll_from_bottom: u16,
    pub theme: &'a Theme,
    pub streaming: bool,
    pub waiting_for_response: bool,
    pub _tick_counter: u32,
    pub _request_start: Option<Instant>,
}

impl<'a> Widget for MessagesWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let msg_count = self.messages.len();
        let mut all_lines: Vec<Line<'static>> = Vec::new();
        for (idx, msg) in self.messages.iter().enumerate() {
            let is_last = idx == msg_count - 1;
            // For user messages, render with blue left border background
            match msg {
                ChatMessage::User { content } => {
                    // Empty line before
                    all_lines.push(Line::from(Span::styled("", Style::default())));
                    let user_bg = Style::default().fg(self.theme.fg).bg(self.theme.input_bg);
                    let border_style = Style::default()
                        .fg(self.theme.input_border)
                        .bg(self.theme.input_bg);
                    for line in content.lines() {
                        let text_w = unicode_width::UnicodeWidthStr::width(line);
                        let pad = (area.width as usize).saturating_sub(3 + text_w);
                        all_lines.push(Line::from(vec![
                            Span::styled(" ", Style::default()),
                            Span::styled("│ ", border_style),
                            Span::styled(line.to_string(), user_bg),
                            Span::styled(" ".repeat(pad), user_bg),
                        ]));
                    }
                    all_lines.push(Line::from(Span::styled("", Style::default())));
                }
                ChatMessage::Assistant { .. }
                    if is_last && (self.waiting_for_response || self.streaming) =>
                {
                    // Streaming/waiting: render content normally (indicator is in fixed gap area)
                    all_lines.extend(msg.render_lines(area.width, self.theme));
                }
                _ => {
                    all_lines.extend(msg.render_lines(area.width, self.theme));
                }
            }
        }

        // Count physical lines (after word-wrap) so scroll offset is correct.
        let w = area.width.max(1) as usize;
        let total_lines: u16 = all_lines
            .iter()
            .map(|line| {
                let lw = line.width();
                if lw <= w {
                    1u16
                } else {
                    lw.div_ceil(w) as u16
                }
            })
            .sum();
        let visible = area.height;
        let max_scroll = total_lines.saturating_sub(visible);
        let scroll = max_scroll.saturating_sub(self.scroll_from_bottom);

        let paragraph = Paragraph::new(all_lines)
            .scroll((scroll, 0))
            .wrap(Wrap { trim: false });

        paragraph.render(area, buf);
    }
}
