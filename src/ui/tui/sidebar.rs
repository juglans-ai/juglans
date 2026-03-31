use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use super::app::{App, TuiMode};
use super::theme::Theme;

pub struct SidebarWidget<'a> {
    pub app: &'a App,
    pub theme: &'a Theme,
}

impl<'a> Widget for SidebarWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let mut lines: Vec<Line<'static>> = Vec::new();

        // Conversation starter
        if let Some(starter) = &self.app.conversation_starter {
            lines.push(Line::from(vec![
                Span::styled(
                    "Conversation starter: ",
                    Style::default()
                        .fg(self.theme.fg)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    truncate_str(starter, area.width as usize - 24),
                    Style::default().fg(self.theme.fg),
                ),
            ]));
        } else {
            lines.push(Line::from(Span::styled(
                "New conversation",
                Style::default()
                    .fg(self.theme.fg)
                    .add_modifier(Modifier::BOLD),
            )));
        }

        lines.push(Line::raw(""));

        // Context section
        lines.push(Line::from(Span::styled(
            "Context",
            Style::default()
                .fg(self.theme.fg)
                .add_modifier(Modifier::BOLD),
        )));

        // Token count
        let token_str = format_tokens(self.app.token_count);
        lines.push(Line::from(Span::styled(
            format!("{} tokens", token_str),
            Style::default().fg(self.theme.muted),
        )));

        // Usage percentage
        lines.push(Line::from(Span::styled(
            format!("{}% used", self.app.token_pct),
            Style::default().fg(self.theme.muted),
        )));

        // Cost
        lines.push(Line::from(Span::styled(
            format!("${:.2} spent", self.app.cost),
            Style::default().fg(self.theme.muted),
        )));

        lines.push(Line::raw(""));

        // Mode section
        match self.app.mode {
            TuiMode::Agent => {
                lines.push(Line::from(Span::styled(
                    "Agent",
                    Style::default()
                        .fg(self.theme.fg)
                        .add_modifier(Modifier::BOLD),
                )));
                if let Some(name) = &self.app.agent_name {
                    lines.push(Line::from(Span::styled(
                        name.clone(),
                        Style::default().fg(self.theme.accent),
                    )));
                    if let Some(state) = &self.app.agent_state {
                        lines.push(Line::from(Span::styled(
                            state.model.clone(),
                            Style::default().fg(self.theme.muted),
                        )));
                    }
                } else {
                    lines.push(Line::from(Span::styled(
                        "No agent loaded",
                        Style::default().fg(self.theme.muted),
                    )));
                    lines.push(Line::from(Span::styled(
                        "Press Tab to select",
                        Style::default().fg(self.theme.muted),
                    )));
                }
            }
            TuiMode::ClaudeCode => {
                lines.push(Line::from(Span::styled(
                    "Claude Code",
                    Style::default()
                        .fg(self.theme.fg)
                        .add_modifier(Modifier::BOLD),
                )));
                lines.push(Line::from(Span::styled(
                    self.app.model_name.clone(),
                    Style::default().fg(self.theme.muted),
                )));
            }
        }

        // Top content
        let top_paragraph = Paragraph::new(lines);
        top_paragraph.render(area, buf);

        // Bottom: cwd + brand (pinned to bottom of sidebar)
        let version = env!("CARGO_PKG_VERSION");
        if area.height >= 4 {
            // cwd line
            let cwd_y = area.y + area.height - 3;
            let cwd_line = Line::from(Span::styled(
                self.app.cwd.clone(),
                Style::default().fg(self.theme.muted),
            ));
            Paragraph::new(cwd_line).render(Rect::new(area.x, cwd_y, area.width, 1), buf);

            // brand line: ● Juglans 0.2.4
            let brand_y = area.y + area.height - 1;
            let brand_line = Line::from(vec![
                Span::styled("● ", Style::default().fg(self.theme.tool_ok)),
                Span::styled("Jug", Style::default().fg(self.theme.logo_dim)),
                Span::styled(
                    "lans",
                    Style::default()
                        .fg(self.theme.logo_bright)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" {}", version),
                    Style::default().fg(self.theme.muted),
                ),
            ]);
            Paragraph::new(brand_line).render(Rect::new(area.x, brand_y, area.width, 1), buf);
        }
    }
}

fn format_tokens(count: u64) -> String {
    if count >= 1_000_000 {
        format!("{:.1}M", count as f64 / 1_000_000.0)
    } else if count >= 1_000 {
        format!("{:.1}K", count as f64 / 1_000.0)
    } else {
        format!("{}", count)
    }
}

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        let end = max_len.saturating_sub(3);
        let truncated: String = s.chars().take(end).collect();
        format!("{}...", truncated)
    }
}
