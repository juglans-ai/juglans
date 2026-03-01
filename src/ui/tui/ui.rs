use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph},
    Frame,
};

use super::app::{App, Page};
use super::dialog::DialogWidget;
use super::editor::EditorWidget;
use super::messages::MessagesWidget;
use super::sidebar::SidebarWidget;
use super::status_bar::StatusBarWidget;
use super::welcome::WelcomeWidget;

pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();

    // Fill entire terminal with theme background
    f.render_widget(Clear, area);
    f.render_widget(
        Block::default().style(Style::default().bg(app.theme.bg)),
        area,
    );

    match app.page {
        Page::Welcome => draw_welcome(f, app),
        Page::Chat => draw_chat(f, app),
    }

    // Dialog overlay (always on top)
    if let Some(dialog) = &app.active_dialog {
        let widget = DialogWidget {
            dialog,
            theme: &app.theme,
        };
        f.render_widget(widget, area);
    }
}

fn draw_welcome(f: &mut Frame, app: &App) {
    let area = f.area();
    let version = env!("CARGO_PKG_VERSION");

    // Layout: [welcome content (with embedded editor)] [status bar]
    let chunks = Layout::vertical([
        Constraint::Min(10),   // Welcome content
        Constraint::Length(1), // Status bar
    ])
    .split(area);

    // Welcome page (renders logo + real editor + shortcuts + tip)
    let welcome = WelcomeWidget { app };
    f.render_widget(welcome, chunks[0]);

    // Status bar
    let status = StatusBarWidget {
        cwd: &app.cwd,
        version,
        theme: &app.theme,
    };
    f.render_widget(status, chunks[1]);
}

fn draw_chat(f: &mut Frame, app: &App) {
    let area = f.area();

    // Horizontal: [left panel ~72%] [right sidebar ~28%]
    let h_chunks =
        Layout::horizontal([Constraint::Percentage(72), Constraint::Percentage(28)]).split(area);

    // Dynamic editor height: expands with content lines, max 6 text lines
    let text_lines = app.editor.textarea.lines().len() as u16;
    let editor_height = (text_lines + 5).clamp(6, 11); // +5 = top pad + gap + variant + bottom pad + hints

    // Left panel: [messages] [gap] [editor]
    let left_chunks = Layout::vertical([
        Constraint::Min(3),                // Messages (fills remaining)
        Constraint::Length(1),             // Gap between messages and editor
        Constraint::Length(editor_height), // Editor input area (dynamic)
    ])
    .split(h_chunks[0]);

    // Messages
    let messages = MessagesWidget {
        messages: &app.messages,
        scroll_from_bottom: app.scroll_from_bottom,
        theme: &app.theme,
        streaming: app.streaming,
        waiting_for_response: app.waiting_for_response,
        _tick_counter: app.tick_counter,
        _request_start: app.request_start,
    };
    f.render_widget(messages, left_chunks[0]);

    // Cache messages area and rendered text for mouse selection
    let ma = left_chunks[0];
    app.messages_area.set(ma);
    {
        let mut cached = Vec::with_capacity(ma.height as usize);
        for y in ma.y..ma.y + ma.height {
            let mut line = String::new();
            for x in ma.x..ma.x + ma.width {
                if let Some(cell) = f.buffer_mut().cell((x, y)) {
                    line.push_str(cell.symbol());
                }
            }
            cached.push(line);
        }
        *app.rendered_lines.borrow_mut() = cached;
    }

    // Detect user message rows by checking for │ with input_border color
    {
        let mut rows = Vec::new();
        let check_x = ma.x + 1; // " │" — the │ is at offset 1
        for y in ma.y..ma.y + ma.height {
            if let Some(cell) = f.buffer_mut().cell((check_x, y)) {
                if cell.symbol() == "│" && cell.fg == app.theme.input_border {
                    rows.push(y);
                }
            }
        }
        *app.user_msg_rows.borrow_mut() = rows;
    }

    // Selection highlight
    if let Some(sel) = &app.selection {
        let (start, end) =
            if sel.start.1 < sel.end.1 || (sel.start.1 == sel.end.1 && sel.start.0 <= sel.end.0) {
                (sel.start, sel.end)
            } else {
                (sel.end, sel.start)
            };
        for y in start.1..=end.1 {
            if y < ma.y || y >= ma.y + ma.height {
                continue;
            }
            let sx = if y == start.1 { start.0 } else { ma.x };
            let ex = if y == end.1 {
                end.0
            } else {
                ma.x + ma.width - 1
            };
            for x in sx..=ex {
                if x >= ma.x && x < ma.x + ma.width {
                    if let Some(cell) = f.buffer_mut().cell_mut((x, y)) {
                        cell.set_style(
                            Style::default()
                                .bg(Color::Rgb(60, 60, 100))
                                .fg(Color::White),
                        );
                    }
                }
            }
        }
    }

    // Generating indicator (fixed between messages and editor)
    if app.streaming {
        let spinner = super::messages::SPINNER;
        let frame = spinner[(app.tick_counter as usize) % spinner.len()];
        let elapsed_str = app
            .request_start
            .map(|s| {
                let d = s.elapsed();
                let secs = d.as_secs();
                if secs < 60 {
                    format!(" · {:.1}s", d.as_secs_f64())
                } else {
                    format!(" · {}m {}s", secs / 60, secs % 60)
                }
            })
            .unwrap_or_default();
        let label = if app.waiting_for_response {
            "Generating..."
        } else {
            "Generating"
        };
        let gen_line = Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(frame.to_string(), Style::default().fg(app.theme.accent)),
            Span::styled(
                format!(" {}{}", label, elapsed_str),
                Style::default().fg(app.theme.muted),
            ),
        ]);
        f.render_widget(Paragraph::new(gen_line), left_chunks[1]);
    }

    // Editor
    app.editor_area.set(left_chunks[2]);
    let editor = EditorWidget {
        state: &app.editor,
        theme: &app.theme,
        focused: app.active_dialog.is_none(),
        scroll_offset: app.editor_scroll,
        streaming: app.streaming,
    };
    f.render_widget(editor, left_chunks[2]);

    // Sidebar separator line (gray vertical line at left edge of sidebar)
    let sep_x = h_chunks[1].x;
    for y in h_chunks[1].y..h_chunks[1].y + h_chunks[1].height {
        if let Some(cell) = f.buffer_mut().cell_mut((sep_x, y)) {
            cell.set_symbol("│");
            cell.set_style(ratatui::style::Style::default().fg(app.theme.border));
        }
    }

    // Right sidebar content (padded from separator)
    let sidebar = SidebarWidget {
        app,
        theme: &app.theme,
    };
    let sidebar_area = ratatui::layout::Rect {
        x: h_chunks[1].x + 2,
        y: h_chunks[1].y,
        width: h_chunks[1].width.saturating_sub(3),
        height: h_chunks[1].height,
    };
    f.render_widget(sidebar, sidebar_area);
}
