use anyhow::{Context, Result};
use base64::Engine;
use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Widget, Wrap},
};
use std::path::Path;
use std::time::{Duration, Instant};

use super::theme::Theme;

// -----------------------------------------------------------------------
// Attachment types
// -----------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Attachment {
    pub file_name: String,
    #[allow(dead_code)]
    pub file_path: String,
    pub kind: AttachmentKind,
    pub size_bytes: u64,
}

#[derive(Debug, Clone)]
pub enum AttachmentKind {
    Image {
        media_type: String,
        base64_data: String,
    },
    TextFile {
        content: String,
    },
}

const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "webp"];

fn is_image_file(ext: &str) -> bool {
    IMAGE_EXTENSIONS.contains(&ext.to_lowercase().as_str())
}

fn media_type_for_extension(ext: &str) -> &'static str {
    match ext.to_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => "application/octet-stream",
    }
}

pub fn format_file_size(bytes: u64) -> String {
    if bytes >= 1_048_576 {
        format!("{:.1}MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.0}KB", bytes as f64 / 1024.0)
    } else {
        format!("{}B", bytes)
    }
}

pub fn load_attachment(path: &Path) -> Result<Attachment> {
    let metadata =
        std::fs::metadata(path).with_context(|| format!("Cannot read: {}", path.display()))?;
    let size_bytes = metadata.len();
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    let kind = if is_image_file(&ext) {
        if size_bytes > 20 * 1024 * 1024 {
            anyhow::bail!("Image too large (>20MB): {}", file_name);
        }
        let bytes = std::fs::read(path)?;
        let encoded = base64::engine::general_purpose::STANDARD.encode(&bytes);
        AttachmentKind::Image {
            media_type: media_type_for_extension(&ext).to_string(),
            base64_data: encoded,
        }
    } else {
        let content =
            std::fs::read_to_string(path).with_context(|| "File is not valid UTF-8 text")?;
        AttachmentKind::TextFile { content }
    };

    Ok(Attachment {
        file_name,
        file_path: path.to_string_lossy().to_string(),
        kind,
        size_bytes,
    })
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
}

pub fn read_directory_entries(dir: &Path) -> Vec<FileEntry> {
    let mut entries = Vec::new();

    // Parent directory
    if dir.parent().is_some() {
        entries.push(FileEntry {
            name: "..".to_string(),
            is_dir: true,
            size: 0,
        });
    }

    if let Ok(read_dir) = std::fs::read_dir(dir) {
        let mut file_entries: Vec<FileEntry> = read_dir
            .filter_map(|e| e.ok())
            .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
            .map(|e| {
                let meta = e.metadata().ok();
                let is_dir = meta.as_ref().is_some_and(|m| m.is_dir());
                let size = meta.as_ref().map_or(0, |m| m.len());
                let name = e.file_name().to_string_lossy().to_string();
                FileEntry { name, is_dir, size }
            })
            .collect();

        // Directories first, then alphabetical
        file_entries.sort_by(|a, b| {
            b.is_dir
                .cmp(&a.is_dir)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });

        entries.extend(file_entries);
    }

    entries
}

// -----------------------------------------------------------------------
// Chat message types
// -----------------------------------------------------------------------

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
        attachments: Vec<Attachment>,
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
    pub fn render_lines(
        &self,
        width: u16,
        theme: &Theme,
    ) -> (Vec<Line<'static>>, Vec<(String, String)>) {
        match self {
            ChatMessage::User {
                content,
                attachments,
            } => {
                let mut lines = Vec::new();
                let user_bg = Style::default().fg(theme.fg).bg(theme.input_bg);

                // Attachment chips
                if !attachments.is_empty() {
                    let chip_style = Style::default().fg(theme.bg).bg(theme.accent);
                    let mut spans: Vec<Span> = vec![Span::styled("  ", user_bg)];
                    for att in attachments {
                        let icon = match &att.kind {
                            AttachmentKind::Image { .. } => "img",
                            AttachmentKind::TextFile { .. } => "txt",
                        };
                        let size_str = format_file_size(att.size_bytes);
                        spans.push(Span::styled(
                            format!(" {} {} {} ", icon, att.file_name, size_str),
                            chip_style,
                        ));
                        spans.push(Span::styled(" ", user_bg));
                    }
                    lines.push(Line::from(spans));
                }

                // Content lines
                for line in content.lines() {
                    lines.push(Line::from(vec![
                        Span::styled("  ", user_bg),
                        Span::styled(line.to_string(), user_bg),
                    ]));
                }
                lines.push(Line::from(Span::styled("", Style::default())));
                (lines, Vec::new())
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
                let mut msg_links = Vec::new();
                if !content.is_empty() {
                    let (md_lines, links) = super::markdown::render_markdown(content, theme, width);
                    lines.extend(md_lines);
                    msg_links = links;
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
                (lines, msg_links)
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
                let header_w = unicode_width::UnicodeWidthStr::width(header.as_str());
                let status_span = format!(" {} ┐", status_text);
                let status_w = unicode_width::UnicodeWidthStr::width(status_span.as_str());
                let pad_len = w.saturating_sub(header_w + status_w);
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
                (lines, Vec::new())
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
    pub link_registry: &'a std::cell::RefCell<Vec<(String, String)>>,
}

impl<'a> Widget for MessagesWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let msg_count = self.messages.len();
        let mut all_lines: Vec<Line<'static>> = Vec::new();
        let mut collected_links: Vec<(String, String)> = Vec::new();
        for (idx, msg) in self.messages.iter().enumerate() {
            let is_last = idx == msg_count - 1;
            // For user messages, render with blue left border background
            match msg {
                ChatMessage::User {
                    content,
                    attachments,
                } => {
                    // Empty line before
                    all_lines.push(Line::from(Span::styled("", Style::default())));
                    let user_bg = Style::default().fg(self.theme.fg).bg(self.theme.input_bg);
                    let border_style = Style::default()
                        .fg(self.theme.input_border)
                        .bg(self.theme.input_bg);

                    // Attachment chips line
                    if !attachments.is_empty() {
                        let chip_style = Style::default().fg(self.theme.bg).bg(self.theme.accent);
                        let mut spans: Vec<Span> = vec![
                            Span::styled(" ", Style::default()),
                            Span::styled("│ ", border_style),
                        ];
                        for att in attachments {
                            let icon = match &att.kind {
                                AttachmentKind::Image { .. } => "img",
                                AttachmentKind::TextFile { .. } => "txt",
                            };
                            let size_str = format_file_size(att.size_bytes);
                            spans.push(Span::styled(
                                format!(" {} {} {} ", icon, att.file_name, size_str),
                                chip_style,
                            ));
                            spans.push(Span::styled(" ", user_bg));
                        }
                        all_lines.push(Line::from(spans));
                    }

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
                    let (lines, links) = msg.render_lines(area.width, self.theme);
                    all_lines.extend(lines);
                    collected_links.extend(links);
                }
                _ => {
                    let (lines, links) = msg.render_lines(area.width, self.theme);
                    all_lines.extend(lines);
                    collected_links.extend(links);
                }
            }
        }
        // Bottom padding to prevent last content from being obscured
        all_lines.push(Line::from(Span::styled("", Style::default())));
        all_lines.push(Line::from(Span::styled("", Style::default())));

        *self.link_registry.borrow_mut() = collected_links;

        // Count physical lines (after word-wrap) so scroll offset is correct.
        // Use u32 to prevent overflow in long conversations.
        let w = area.width.max(1) as usize;
        let total_lines: u32 = all_lines
            .iter()
            .map(|line| {
                let lw = line.width();
                if lw <= w {
                    1u32
                } else {
                    lw.div_ceil(w) as u32
                }
            })
            .sum();
        let visible = area.height as u32;
        // Fixed buffer for word-wrap discrepancy
        let total_lines = total_lines + 5;
        let max_scroll = total_lines.saturating_sub(visible);
        let scroll = max_scroll.saturating_sub(self.scroll_from_bottom as u32);
        let scroll = scroll.min(u16::MAX as u32) as u16;

        let paragraph = Paragraph::new(all_lines)
            .scroll((scroll, 0))
            .wrap(Wrap { trim: false });

        paragraph.render(area, buf);
    }
}
