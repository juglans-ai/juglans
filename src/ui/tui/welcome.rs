use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget},
};

use super::app::{App, TuiMode};
use super::editor::EditorWidget;

// ASCII art logo for Juglans — "jug" dim, "lans" bright
const LOGO: &[&str] = &[
    r"     ╻         ╻                  ",
    r"     ┃╻ ╻╺━┓╻  ┏━┫╺━┓┏━╸╺━┓      ",
    r"     ┃┃ ┃┏━┓┃  ┃ ┃┏━┓┃╺┓┏━┛      ",
    r"  ┗━━┛┗━┛┗━┛┗━╸┗━┛┗━┛┗━┛╹        ",
];

const LOGO_BLOCK: &[&str] = &[
    "       ██╗██╗   ██╗ ██████╗ ██╗      █████╗ ███╗   ██╗███████╗",
    "       ██║██║   ██║██╔════╝ ██║     ██╔══██╗████╗  ██║██╔════╝",
    "       ██║██║   ██║██║  ███╗██║     ███████║██╔██╗ ██║███████╗",
    "  ██   ██║██║   ██║██║   ██║██║     ██╔══██║██║╚██╗██║╚════██║",
    "  ╚█████╔╝╚██████╔╝╚██████╔╝██████╗██║  ██║██║ ╚████║███████║",
    "   ╚════╝  ╚═════╝  ╚═════╝ ╚═════╝╚═╝  ╚═╝╚═╝  ╚═══╝╚══════╝",
];

pub struct WelcomeWidget<'a> {
    pub app: &'a App,
}

impl<'a> Widget for WelcomeWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let theme = &self.app.theme;
        let width = area.width as usize;
        let height = area.height;

        // Choose logo based on terminal width
        let logo = if width >= 68 { LOGO_BLOCK } else { LOGO };
        let logo_height = logo.len() as u16;

        // Editor height
        let text_lines = self.app.editor.textarea.lines().len() as u16;
        let editor_height = (text_lines + 5).clamp(6, 11);

        // Content block: logo + gap + editor + gap + shortcuts + gap + tip
        let content_height = logo_height + 2 + editor_height + 1 + 1 + 2 + 1;
        let top_pad = height.saturating_sub(content_height) / 3;

        // Split the area into rows:
        // [top_pad] [logo] [gap=2] [editor] [gap=1] [shortcuts] [gap=2] [tip] [rest]
        let chunks = Layout::vertical([
            Constraint::Length(top_pad),       // top padding
            Constraint::Length(logo_height),   // logo
            Constraint::Length(2),             // gap
            Constraint::Length(editor_height), // editor
            Constraint::Length(1),             // gap
            Constraint::Length(1),             // shortcuts
            Constraint::Length(2),             // gap
            Constraint::Length(1),             // tip
            Constraint::Min(0),                // rest
        ])
        .split(area);

        // --- Logo (centered) ---
        let mut logo_lines: Vec<Line<'static>> = Vec::new();
        for logo_line in logo {
            let logo_w = logo_line.chars().count();
            let pad = width.saturating_sub(logo_w) / 2;
            let padding = " ".repeat(pad);

            let split_point = logo_w * 5 / 12;
            let chars: Vec<char> = logo_line.chars().collect();
            let dim_part: String = chars[..split_point.min(chars.len())].iter().collect();
            let bright_part: String = chars[split_point.min(chars.len())..].iter().collect();

            logo_lines.push(Line::from(vec![
                Span::raw(padding),
                Span::styled(dim_part, Style::default().fg(theme.logo_dim)),
                Span::styled(
                    bright_part,
                    Style::default()
                        .fg(theme.logo_bright)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        }
        Paragraph::new(logo_lines).render(chunks[1], buf);

        // --- Editor (centered horizontally, capped width) ---
        let max_editor_w = 80u16.min(area.width.saturating_sub(4));
        let editor_x = area.x + (area.width.saturating_sub(max_editor_w)) / 2;
        let editor_rect = Rect {
            x: editor_x,
            y: chunks[3].y,
            width: max_editor_w,
            height: chunks[3].height,
        };
        self.app.editor_area.set(editor_rect);
        let editor = EditorWidget {
            state: &self.app.editor,
            theme,
            focused: self.app.active_dialog.is_none(),
            scroll_offset: self.app.editor_scroll,
            streaming: self.app.streaming,
            attachments: &self.app.attachments,
            attachment_selected: self.app.attachment_selected,
        };
        editor.render(editor_rect, buf);

        // --- Keyboard shortcuts (centered, mode-aware) ---
        let shortcuts_spans = match self.app.mode {
            TuiMode::Agent => vec![
                Span::styled(
                    "tab",
                    Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" select agent   ", Style::default().fg(theme.muted)),
                Span::styled(
                    "ctrl+t",
                    Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" claude mode   ", Style::default().fg(theme.muted)),
                Span::styled(
                    "ctrl+h",
                    Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" help", Style::default().fg(theme.muted)),
            ],
            TuiMode::ClaudeCode => vec![
                Span::styled(
                    "ctrl+o",
                    Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" model   ", Style::default().fg(theme.muted)),
                Span::styled(
                    "ctrl+t",
                    Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" agent mode   ", Style::default().fg(theme.muted)),
                Span::styled(
                    "ctrl+h",
                    Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" help", Style::default().fg(theme.muted)),
            ],
        };
        let sc_len: usize = shortcuts_spans.iter().map(|s| s.width()).sum();
        let sc_pad = width.saturating_sub(sc_len) / 2;
        let mut sc_line = vec![Span::raw(" ".repeat(sc_pad))];
        sc_line.extend(shortcuts_spans);
        Paragraph::new(Line::from(sc_line)).render(chunks[5], buf);

        // --- Tip (centered) ---
        let tip_spans = vec![
            Span::styled("● ", Style::default().fg(theme.tip)),
            Span::styled(
                "Tip",
                Style::default().fg(theme.tip).add_modifier(Modifier::BOLD),
            ),
            Span::styled(" Run ", Style::default().fg(theme.muted)),
            Span::styled(
                "juglans web",
                Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " for headless API access to Juglans",
                Style::default().fg(theme.muted),
            ),
        ];
        let tip_len: usize = tip_spans.iter().map(|s| s.width()).sum();
        let tip_pad = width.saturating_sub(tip_len) / 2;
        let mut tip_line = vec![Span::raw(" ".repeat(tip_pad))];
        tip_line.extend(tip_spans);
        Paragraph::new(Line::from(tip_line)).render(chunks[7], buf);
    }
}
