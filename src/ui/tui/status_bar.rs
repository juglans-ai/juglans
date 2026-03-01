use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use super::theme::Theme;

pub struct StatusBarWidget<'a> {
    pub cwd: &'a str,
    pub version: &'a str,
    pub theme: &'a Theme,
}

impl<'a> Widget for StatusBarWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let width = area.width as usize;

        // Left: working directory
        let left = Span::styled(
            format!(" {}", self.cwd),
            Style::default().fg(self.theme.muted),
        );

        // Right: "● Juglans 0.2.4"
        let right_spans = vec![
            Span::styled("● ", Style::default().fg(self.theme.tool_ok)),
            Span::styled("Jug", Style::default().fg(self.theme.logo_dim)),
            Span::styled(
                "lans",
                Style::default()
                    .fg(self.theme.logo_bright)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" {} ", self.version),
                Style::default().fg(self.theme.muted),
            ),
        ];

        let left_w = left.width();
        let right_w: usize = right_spans.iter().map(|s| s.width()).sum();
        let gap = width.saturating_sub(left_w + right_w);

        let mut spans = vec![left];
        spans.push(Span::raw(" ".repeat(gap)));
        spans.extend(right_spans);

        Paragraph::new(Line::from(spans)).render(area, buf);
    }
}
