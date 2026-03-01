use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::Rect;
use std::cell::{Cell, RefCell};
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use unicode_width::UnicodeWidthChar;

use super::claude_code::{
    self, ceil_char_boundary, floor_char_boundary, ClaudeEvent, ClaudeProcess,
};
use super::dialog::Dialog;
use super::editor::EditorState;
use super::event::AppEvent;
use super::messages::{ChatMessage, ToolStatus};
use super::theme::Theme;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Page {
    Welcome,
    Chat,
}

#[derive(Debug, Clone, Copy)]
pub struct TextSelection {
    pub start: (u16, u16),
    pub end: (u16, u16),
}

pub struct App {
    pub page: Page,
    pub messages: Vec<ChatMessage>,
    pub editor: EditorState,
    pub scroll_from_bottom: u16,
    pub active_dialog: Option<Dialog>,
    pub model_name: String,
    pub token_count: u64,
    pub token_pct: u8,
    pub cost: f64,
    pub status_message: Option<(String, Instant)>,
    pub should_quit: bool,
    pub theme: Theme,
    pub cwd: String,
    pub conversation_starter: Option<String>,
    pub editor_area: Cell<Rect>,
    pub editor_scroll: usize,

    // --- Mouse selection ---
    pub selection: Option<TextSelection>,
    pub messages_area: Cell<Rect>,
    pub rendered_lines: RefCell<Vec<String>>,
    pub user_msg_rows: RefCell<Vec<u16>>,

    // --- Claude Code streaming state ---
    pub streaming: bool,
    pub claude_rx: Option<mpsc::UnboundedReceiver<ClaudeEvent>>,
    pub claude_process: Option<ClaudeProcess>,
    pub session_id: Option<String>,
    pub request_start: Option<Instant>,
    tool_input_buf: String,
    needs_new_assistant_message: bool,
    pub pending_permission_response: Option<String>,
    pub pending_send_message: Option<String>,
    pub tick_counter: u32,
    pub waiting_for_response: bool,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        let cwd = std::env::current_dir()
            .map(|p| {
                if let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) {
                    if let Ok(stripped) = p.strip_prefix(&home) {
                        return format!("~/{}", stripped.display());
                    }
                }
                p.display().to_string()
            })
            .unwrap_or_else(|_| "~".to_string());

        Self {
            page: Page::Welcome,
            messages: Vec::new(),
            editor: EditorState::new(),
            scroll_from_bottom: 0,
            active_dialog: None,
            model_name: "claude-sonnet-4-20250514".to_string(),
            token_count: 0,
            token_pct: 0,
            cost: 0.0,
            status_message: None,
            should_quit: false,
            theme: Theme::default(),
            cwd,
            conversation_starter: None,
            editor_area: Cell::new(Rect::default()),
            editor_scroll: 0,
            // Mouse selection
            selection: None,
            messages_area: Cell::new(Rect::default()),
            rendered_lines: RefCell::new(Vec::new()),
            user_msg_rows: RefCell::new(Vec::new()),
            // Claude Code state
            streaming: false,
            claude_rx: None,
            claude_process: None,
            session_id: None,
            request_start: None,
            tool_input_buf: String::new(),
            needs_new_assistant_message: false,
            pending_permission_response: None,
            pending_send_message: None,
            tick_counter: 0,
            waiting_for_response: false,
        }
    }

    pub fn update(&mut self, event: AppEvent) {
        // Clear expired status messages
        if let Some((_, created)) = &self.status_message {
            if created.elapsed() > Duration::from_secs(5) {
                self.status_message = None;
            }
        }

        match event {
            AppEvent::Key(key) => self.handle_key(key),
            AppEvent::MouseScrollUp { x, y } => {
                let ea = self.editor_area.get();
                if y >= ea.y && y < ea.y + ea.height && x >= ea.x && x < ea.x + ea.width {
                    self.editor_scroll = self.editor_scroll.saturating_sub(2);
                } else {
                    self.scroll_up(3);
                }
            }
            AppEvent::MouseScrollDown { x, y } => {
                let ea = self.editor_area.get();
                if y >= ea.y && y < ea.y + ea.height && x >= ea.x && x < ea.x + ea.width {
                    let max = self.editor.textarea.lines().len().saturating_sub(1);
                    self.editor_scroll = (self.editor_scroll + 2).min(max);
                } else {
                    self.scroll_down(3);
                }
            }
            AppEvent::MouseClick { x, y } => self.handle_mouse_click(x, y),
            AppEvent::MouseDrag { x, y } => {
                if let Some(ref mut sel) = self.selection {
                    sel.end = (x, y);
                }
            }
            AppEvent::MouseUp { x, y } => self.handle_mouse_up(x, y),
            AppEvent::Resize(_, _) => {}
            AppEvent::Tick => {
                self.tick_counter = self.tick_counter.wrapping_add(1);
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        // Dialog takes priority (even during streaming, e.g. permission dialogs)
        if let Some(ref mut dialog) = self.active_dialog {
            let (close, result) = dialog.handle_key(key);
            if close {
                if let Some(ref result) = result {
                    if result.starts_with("allow:") || result.starts_with("deny:") {
                        // Permission response — schedule for async send
                        let allow = result.starts_with("allow:");
                        let req_id = if allow { &result[6..] } else { &result[5..] };
                        let json = claude_code::build_permission_response(req_id, allow);
                        self.pending_permission_response = Some(json);
                    } else {
                        match self.active_dialog.as_ref() {
                            Some(Dialog::Quit) => {
                                self.should_quit = true;
                                self.active_dialog = None;
                                return;
                            }
                            Some(Dialog::ModelPicker { .. }) => {
                                self.model_name = result.clone();
                                self.status_message =
                                    Some(("Model changed".to_string(), Instant::now()));
                            }
                            Some(Dialog::MessageActions { .. }) => {
                                self.handle_message_action(result);
                            }
                            _ => {}
                        }
                    }
                }
                self.active_dialog = None;
            }
            return;
        }

        // During streaming: only allow interrupt and scrolling
        if self.streaming {
            match (key.code, key.modifiers) {
                (KeyCode::Esc, _) | (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                    self.interrupt_streaming();
                }
                (KeyCode::PageUp, _) => self.scroll_up(10),
                (KeyCode::PageDown, _) => self.scroll_down(10),
                (KeyCode::Char('u'), KeyModifiers::CONTROL) => self.scroll_up(15),
                (KeyCode::Char('d'), KeyModifiers::CONTROL) => self.scroll_down(15),
                _ => {} // swallow all other keys during streaming
            }
            return;
        }

        // Global shortcuts
        match (key.code, key.modifiers) {
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                if self.editor.is_empty() {
                    self.active_dialog = Some(Dialog::Quit);
                } else {
                    self.editor.handle_key(key);
                }
                return;
            }
            (KeyCode::Char('h'), KeyModifiers::CONTROL) => {
                self.active_dialog = Some(Dialog::Help);
                return;
            }
            (KeyCode::Char('o'), KeyModifiers::CONTROL) => {
                self.active_dialog = Some(Dialog::model_picker());
                return;
            }
            (KeyCode::Char('l'), KeyModifiers::CONTROL) => {
                if self.streaming {
                    self.interrupt_streaming();
                }
                // Kill existing subprocess for a fresh start
                if let Some(mut proc) = self.claude_process.take() {
                    proc.kill();
                }
                self.claude_rx = None;
                self.messages.clear();
                self.scroll_from_bottom = 0;
                self.conversation_starter = None;
                self.token_count = 0;
                self.token_pct = 0;
                self.cost = 0.0;
                self.session_id = None;
                self.page = Page::Welcome;
                return;
            }
            (KeyCode::Esc, _) => {
                if self.page == Page::Chat && !self.editor.is_empty() {
                    self.editor.handle_key(key);
                }
                return;
            }
            (KeyCode::PageUp, _) => {
                self.scroll_up(10);
                return;
            }
            (KeyCode::PageDown, _) => {
                self.scroll_down(10);
                return;
            }
            (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                self.scroll_up(15);
                return;
            }
            (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                self.scroll_down(15);
                return;
            }
            (KeyCode::Char('y'), KeyModifiers::CONTROL) => {
                self.copy_last_response();
                return;
            }
            _ => {}
        }

        // Forward to editor
        if let Some(text) = self.editor.handle_key(key) {
            self.send_message(text);
        }
        self.editor_scroll = 0;
    }

    // -----------------------------------------------------------------------
    // send_message — spawn claude subprocess
    // -----------------------------------------------------------------------

    fn send_message(&mut self, text: String) {
        if self.streaming {
            self.status_message = Some((
                "Please wait for the current response".to_string(),
                Instant::now(),
            ));
            return;
        }

        if self.page == Page::Welcome {
            self.page = Page::Chat;
            self.conversation_starter = Some(text.clone());
        }

        self.messages.push(ChatMessage::User {
            content: text.clone(),
        });

        // Streaming setup
        self.streaming = true;
        self.waiting_for_response = true;
        self.request_start = Some(Instant::now());
        self.tool_input_buf.clear();
        self.needs_new_assistant_message = false;

        // Push placeholder assistant message
        self.messages.push(ChatMessage::Assistant {
            thinking: None,
            content: String::new(),
            model: self.model_name.clone(),
            elapsed: None,
        });

        // Queue the message for async spawning in the event loop
        self.pending_send_message = Some(text);
        self.scroll_from_bottom = 0;
    }

    /// Actually spawn the claude subprocess (called from the async event loop)
    pub async fn do_spawn_claude(&mut self, text: String) {
        let real_cwd = self.resolve_cwd();

        match claude_code::spawn_claude(
            &text,
            &self.model_name,
            self.session_id.as_deref(),
            &real_cwd,
        )
        .await
        {
            Ok((process, rx)) => {
                self.claude_process = Some(process);
                self.claude_rx = Some(rx);
            }
            Err(e) => {
                self.update_last_assistant_content(&format!(
                    "Error: Failed to start Claude Code: {}",
                    e
                ));
                self.streaming = false;
                self.request_start = None;
            }
        }
    }

    /// Send a follow-up user message to an existing subprocess (multi-turn)
    pub async fn send_user_message_to_subprocess(&mut self, text: String) {
        let user_msg = serde_json::json!({
            "type": "user",
            "session_id": self.session_id.as_deref().unwrap_or(""),
            "message": {
                "role": "user",
                "content": text
            },
            "parent_tool_use_id": null
        });
        if let Some(proc) = &mut self.claude_process {
            if let Err(e) = proc.send_response(&user_msg.to_string()).await {
                self.update_last_assistant_content(&format!("Error sending message: {}", e));
                self.streaming = false;
                self.request_start = None;
            }
        }
    }

    // -----------------------------------------------------------------------
    // handle_claude_event — process events from the subprocess
    // -----------------------------------------------------------------------

    pub fn handle_claude_event(&mut self, event: ClaudeEvent) {
        match event {
            ClaudeEvent::TextDelta(text) => {
                self.waiting_for_response = false;
                if self.needs_new_assistant_message {
                    self.messages.push(ChatMessage::Assistant {
                        thinking: None,
                        content: String::new(),
                        model: self.model_name.clone(),
                        elapsed: None,
                    });
                    self.needs_new_assistant_message = false;
                }
                self.append_to_last_assistant_content(&text);
                self.scroll_from_bottom = 0;
            }

            ClaudeEvent::ThinkingDelta(text) => {
                self.waiting_for_response = false;
                if self.needs_new_assistant_message {
                    self.messages.push(ChatMessage::Assistant {
                        thinking: None,
                        content: String::new(),
                        model: self.model_name.clone(),
                        elapsed: None,
                    });
                    self.needs_new_assistant_message = false;
                }
                self.append_to_last_assistant_thinking(&text);
            }

            ClaudeEvent::ToolUseStart { name, .. } => {
                self.tool_input_buf.clear();
                self.messages.push(ChatMessage::ToolCall {
                    name,
                    status: ToolStatus::InProgress,
                    params: String::new(),
                    response: None,
                });
                self.scroll_from_bottom = 0;
            }

            ClaudeEvent::ToolInputDelta(chunk) => {
                self.tool_input_buf.push_str(&chunk);
                // Update the last ToolCall message's params
                if let Some(ChatMessage::ToolCall { params, .. }) = self.messages.last_mut() {
                    *params = claude_code::format_tool_params(&self.tool_input_buf);
                }
            }

            ClaudeEvent::ContentBlockStop { .. } => {
                // Mark the most recent InProgress tool call as Completed
                for msg in self.messages.iter_mut().rev() {
                    if let ChatMessage::ToolCall { status, .. } = msg {
                        if matches!(status, ToolStatus::InProgress) {
                            *status = ToolStatus::Completed;
                            break;
                        }
                    }
                    // Stop searching once we hit a non-ToolCall
                    if matches!(
                        msg,
                        ChatMessage::Assistant { .. } | ChatMessage::User { .. }
                    ) {
                        break;
                    }
                }
            }

            ClaudeEvent::AssistantSnapshot { content } => {
                self.update_last_assistant_content(&content);
                self.scroll_from_bottom = 0;
            }

            ClaudeEvent::MessageStop => {
                // Mark that the next TextDelta should create a new assistant message.
                // (For tool-use flows, Claude continues after tool execution.)
                self.needs_new_assistant_message = true;
            }

            ClaudeEvent::Result {
                cost_usd,
                input_tokens,
                output_tokens,
                session_id,
                ..
            } => {
                self.cost += cost_usd;
                self.token_count += input_tokens + output_tokens;
                self.token_pct = ((self.token_count as f64 / 200_000.0) * 100.0).min(100.0) as u8;
                if !session_id.is_empty() {
                    self.session_id = Some(session_id);
                }
                // Clean up empty placeholders first, then set elapsed on the
                // real assistant message (MessageStop pushes empty placeholders).
                self.cleanup_empty_assistant_messages();
                if let Some(elapsed) = self.request_start.map(|s| s.elapsed()) {
                    self.update_last_assistant_elapsed(elapsed);
                }
                // Turn complete — unlock UI (subprocess stays alive for multi-turn)
                self.streaming = false;
                self.waiting_for_response = false;
                self.request_start = None;
            }

            ClaudeEvent::PermissionRequest { request_id, .. } => {
                // Auto-allow all tool uses without confirmation dialog
                let json = claude_code::build_permission_response(&request_id, true);
                self.pending_permission_response = Some(json);
            }

            ClaudeEvent::ProcessExited { error, .. } => {
                self.streaming = false;
                self.waiting_for_response = false;
                self.claude_process = None;
                self.claude_rx = None;

                if let Some(err) = error {
                    let trimmed = err.trim();
                    if !trimmed.is_empty() {
                        let last_content = self.get_last_assistant_content().to_string();
                        if last_content.is_empty() {
                            self.update_last_assistant_content(&format!(
                                "Error from Claude Code:\n{}",
                                trimmed
                            ));
                        } else {
                            self.status_message = Some((
                                format!("stderr: {}", trimmed.lines().next().unwrap_or("")),
                                Instant::now(),
                            ));
                        }
                    }
                }

                self.cleanup_empty_assistant_messages();
                if let Some(start) = self.request_start.take() {
                    self.update_last_assistant_elapsed(start.elapsed());
                }
            }
        }
    }

    // -----------------------------------------------------------------------
    // interrupt_streaming — kill subprocess, mark state
    // -----------------------------------------------------------------------

    fn interrupt_streaming(&mut self) {
        if let Some(mut proc) = self.claude_process.take() {
            proc.kill();
        }
        self.claude_rx = None;
        self.streaming = false;
        self.waiting_for_response = false;

        self.cleanup_empty_assistant_messages();

        if let Some(start) = self.request_start.take() {
            self.update_last_assistant_elapsed(start.elapsed());
        }

        self.append_to_last_assistant_content("\n\n*[interrupted]*");

        for msg in self.messages.iter_mut() {
            if let ChatMessage::ToolCall { status, .. } = msg {
                if matches!(status, ToolStatus::InProgress) {
                    *status = ToolStatus::Error;
                }
            }
        }

        self.status_message = Some(("Generation interrupted".to_string(), Instant::now()));
    }

    // -----------------------------------------------------------------------
    // Helpers
    // -----------------------------------------------------------------------

    fn resolve_cwd(&self) -> String {
        if self.cwd.starts_with("~/") {
            if let Some(home) = std::env::var_os("HOME") {
                return format!("{}/{}", home.to_string_lossy(), &self.cwd[2..]);
            }
        }
        self.cwd.clone()
    }

    fn append_to_last_assistant_content(&mut self, text: &str) {
        for msg in self.messages.iter_mut().rev() {
            if let ChatMessage::Assistant { content, .. } = msg {
                content.push_str(text);
                return;
            }
        }
    }

    fn append_to_last_assistant_thinking(&mut self, text: &str) {
        for msg in self.messages.iter_mut().rev() {
            if let ChatMessage::Assistant { thinking, .. } = msg {
                match thinking {
                    Some(t) => t.push_str(text),
                    None => *thinking = Some(text.to_string()),
                }
                return;
            }
        }
    }

    fn update_last_assistant_content(&mut self, text: &str) {
        for msg in self.messages.iter_mut().rev() {
            if let ChatMessage::Assistant { content, .. } = msg {
                *content = text.to_string();
                return;
            }
        }
    }

    fn update_last_assistant_elapsed(&mut self, elapsed: Duration) {
        for msg in self.messages.iter_mut().rev() {
            if let ChatMessage::Assistant { elapsed: el, .. } = msg {
                *el = Some(elapsed);
                return;
            }
        }
    }

    fn get_last_assistant_content(&self) -> &str {
        for msg in self.messages.iter().rev() {
            if let ChatMessage::Assistant { content, .. } = msg {
                return content;
            }
        }
        ""
    }

    fn copy_last_response(&mut self) {
        let content = self.get_last_assistant_content();
        if content.is_empty() {
            self.status_message = Some(("No response to copy".to_string(), Instant::now()));
            return;
        }
        let text = content.to_string();
        match arboard::Clipboard::new().and_then(|mut cb| cb.set_text(&text)) {
            Ok(_) => {
                self.status_message = Some(("Copied to clipboard".to_string(), Instant::now()));
            }
            Err(e) => {
                self.status_message = Some((format!("Copy failed: {}", e), Instant::now()));
            }
        }
    }

    fn cleanup_empty_assistant_messages(&mut self) {
        while let Some(ChatMessage::Assistant {
            content, thinking, ..
        }) = self.messages.last()
        {
            if content.is_empty() && thinking.is_none() {
                self.messages.pop();
            } else {
                break;
            }
        }
    }

    fn handle_mouse_click(&mut self, x: u16, y: u16) {
        // Clear any existing selection
        self.selection = None;

        // Check if click is in messages area → start text selection
        let ma = self.messages_area.get();
        if ma.width > 0 && x >= ma.x && x < ma.x + ma.width && y >= ma.y && y < ma.y + ma.height {
            self.selection = Some(TextSelection {
                start: (x, y),
                end: (x, y),
            });
            return;
        }

        let area = self.editor_area.get();
        let content_x = area.x + 2;
        let content_y = area.y + 1;
        let content_h = area.height.saturating_sub(4);
        let content_w = area.width.saturating_sub(3);
        if x >= content_x
            && x < content_x + content_w
            && y >= content_y
            && y < content_y + content_h
        {
            let row = (y - content_y) as usize;
            let display_col = (x - content_x) as usize;

            // Convert display column to char index (CJK chars are double-width)
            let line = self
                .editor
                .textarea
                .lines()
                .get(row)
                .cloned()
                .unwrap_or_default();
            let mut char_idx = 0;
            let mut width_acc = 0;
            for ch in line.chars() {
                let w = ch.width().unwrap_or(1);
                if width_acc + w > display_col {
                    break;
                }
                width_acc += w;
                char_idx += 1;
            }
            self.editor.set_cursor(row, char_idx);
        }
    }

    fn handle_mouse_up(&mut self, x: u16, y: u16) {
        if let Some(ref mut sel) = self.selection {
            sel.end = (x, y);
            // If no drag (single click) — clear selection
            if sel.start == sel.end {
                self.selection = None;
                // Single click on user message → open Message Actions
                if self.user_msg_rows.borrow().contains(&y) {
                    self.active_dialog = Some(Dialog::MessageActions { selected: 0 });
                }
                return;
            }
            // Extract text and copy to clipboard
            let text = self.extract_selected_text();
            if !text.is_empty() {
                match arboard::Clipboard::new().and_then(|mut cb| cb.set_text(&text)) {
                    Ok(_) => {
                        self.status_message =
                            Some(("Copied to clipboard".to_string(), Instant::now()));
                    }
                    Err(e) => {
                        self.status_message = Some((format!("Copy failed: {}", e), Instant::now()));
                    }
                }
            }
            // Keep selection visible (cleared on next click)
        }
    }

    fn extract_selected_text(&self) -> String {
        let sel = match &self.selection {
            Some(s) => *s,
            None => return String::new(),
        };
        let lines = self.rendered_lines.borrow();
        let ma = self.messages_area.get();
        if lines.is_empty() || ma.width == 0 {
            return String::new();
        }

        // Normalize: ensure start is before end
        let (start, end) =
            if sel.start.1 < sel.end.1 || (sel.start.1 == sel.end.1 && sel.start.0 <= sel.end.0) {
                (sel.start, sel.end)
            } else {
                (sel.end, sel.start)
            };

        let mut result = Vec::new();
        for y in start.1..=end.1 {
            if y < ma.y || y >= ma.y + ma.height {
                continue;
            }
            let row = (y - ma.y) as usize;
            if let Some(line) = lines.get(row) {
                let from_col = if y == start.1 {
                    start.0.saturating_sub(ma.x) as usize
                } else {
                    0
                };
                let to_col = if y == end.1 {
                    (end.0.saturating_sub(ma.x) as usize) + 1
                } else {
                    line.len()
                };
                // Safe char-boundary slicing
                let safe_from = if from_col >= line.len() {
                    line.len()
                } else {
                    floor_char_boundary(line, from_col)
                };
                let safe_to = if to_col >= line.len() {
                    line.len()
                } else {
                    ceil_char_boundary(line, to_col)
                };
                if safe_from < safe_to {
                    result.push(line[safe_from..safe_to].to_string());
                }
            }
        }
        result.join("\n").trim_end().to_string()
    }

    fn handle_message_action(&mut self, action: &str) {
        let text = match action {
            "copy_last" => {
                let content = self.get_last_assistant_content();
                if content.is_empty() {
                    None
                } else {
                    Some(content.to_string())
                }
            }
            "copy_all" => {
                let all: Vec<String> = self
                    .messages
                    .iter()
                    .filter_map(|m| match m {
                        ChatMessage::User { content } => Some(format!("> {}", content)),
                        ChatMessage::Assistant { content, .. } if !content.is_empty() => {
                            Some(content.clone())
                        }
                        _ => None,
                    })
                    .collect();
                if all.is_empty() {
                    None
                } else {
                    Some(all.join("\n\n"))
                }
            }
            _ => None,
        };

        if let Some(text) = text {
            match arboard::Clipboard::new().and_then(|mut cb| cb.set_text(&text)) {
                Ok(_) => {
                    self.status_message = Some(("Copied to clipboard".to_string(), Instant::now()));
                }
                Err(e) => {
                    self.status_message = Some((format!("Copy failed: {}", e), Instant::now()));
                }
            }
        } else {
            self.status_message = Some(("Nothing to copy".to_string(), Instant::now()));
        }
    }

    fn scroll_up(&mut self, amount: u16) {
        let approx_lines: u16 = self
            .messages
            .iter()
            .map(|m| match m {
                ChatMessage::User { content } => content.lines().count() as u16 + 2,
                ChatMessage::Assistant {
                    content, thinking, ..
                } => {
                    content.lines().count() as u16
                        + thinking
                            .as_ref()
                            .map_or(0, |t| t.lines().count() as u16 + 2)
                        + 4
                }
                _ => 6,
            })
            .sum();
        self.scroll_from_bottom = self
            .scroll_from_bottom
            .saturating_add(amount)
            .min(approx_lines);
    }

    fn scroll_down(&mut self, amount: u16) {
        self.scroll_from_bottom = self.scroll_from_bottom.saturating_sub(amount);
    }
}
