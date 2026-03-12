// src/ui/editor.rs
//! Custom multiline editor with Shift+Enter for newlines

use anyhow::Result;
use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute, queue,
    style::{Color, Print, SetForegroundColor, ResetColor},
    terminal::{self, Clear, ClearType},
};
use std::io::{stdout, Write};

pub struct MultilineEditor {
    lines: Vec<String>,
    cursor_row: usize,
    cursor_col: usize,
    start_row: Option<u16>,  // Tracks the starting row of the input box
}

impl MultilineEditor {
    pub fn new() -> Self {
        Self {
            lines: vec![String::new()],
            cursor_row: 0,
            cursor_col: 0,
            start_row: None,
        }
    }

    /// Edit multiline text
    /// Enter: submit
    /// Shift+Enter: new line
    /// Ctrl+C: cancel
    pub fn edit(&mut self, agent_name: &str, chat_id: Option<&str>) -> Result<Option<String>> {
        let mut stdout = stdout();

        // Get current cursor position
        let (_, term_height) = terminal::size()?;

        // Make room for the input box: calculate required lines
        let total_height = 5u16;  // top border + input line + bottom border + status bar + help

        // Ensure enough space
        for _ in 0..total_height {
            println!();
        }
        stdout.flush()?;

        // Record the starting row of the input box (fixed position from bottom)
        self.start_row = Some(term_height.saturating_sub(total_height));

        // Enter raw mode (keep cursor visible)
        terminal::enable_raw_mode()?;

        let result = self.edit_loop(agent_name, chat_id);

        // Exit raw mode, clear input area
        if let Some(start) = self.start_row {
            for i in 0..(total_height + self.lines.len() as u16) {
                queue!(stdout, cursor::MoveTo(0, start + i), Clear(ClearType::CurrentLine))?;
            }
        }

        terminal::disable_raw_mode()?;

        // Reset state
        self.start_row = None;

        result
    }

    fn edit_loop(&mut self, agent_name: &str, chat_id: Option<&str>) -> Result<Option<String>> {
        let mut stdout = stdout();

        // Get terminal size
        let (term_width, _) = terminal::size()?;

        loop {
            // Redraw interface
            self.render(&mut stdout, term_width, agent_name, chat_id)?;

            // Read keyboard event
            if let Event::Key(key) = event::read()? {
                match self.handle_key(key)? {
                    EditorAction::Submit => {
                        // Submit content
                        let content = self.lines.join("\n");
                        self.clear();
                        return Ok(Some(content));
                    }
                    EditorAction::Cancel => {
                        self.clear();
                        return Ok(None);
                    }
                    EditorAction::Exit => {
                        self.clear();
                        return Ok(None);
                    }
                    EditorAction::Continue => {
                        // Continue editing
                    }
                }
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> Result<EditorAction> {
        match (key.code, key.modifiers) {
            // Enter: submit
            (KeyCode::Enter, KeyModifiers::NONE) => {
                Ok(EditorAction::Submit)
            }

            // Shift+Enter: new line
            (KeyCode::Enter, KeyModifiers::SHIFT) => {
                self.insert_newline();
                Ok(EditorAction::Continue)
            }

            // Ctrl+C: cancel
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                Ok(EditorAction::Cancel)
            }

            // Ctrl+D: exit
            (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                Ok(EditorAction::Exit)
            }

            // Backspace: delete character
            (KeyCode::Backspace, _) => {
                self.backspace();
                Ok(EditorAction::Continue)
            }

            // Delete: delete character to the right
            (KeyCode::Delete, _) => {
                self.delete();
                Ok(EditorAction::Continue)
            }

            // Left arrow
            (KeyCode::Left, _) => {
                self.move_cursor_left();
                Ok(EditorAction::Continue)
            }

            // Right arrow
            (KeyCode::Right, _) => {
                self.move_cursor_right();
                Ok(EditorAction::Continue)
            }

            // Up arrow
            (KeyCode::Up, _) => {
                self.move_cursor_up();
                Ok(EditorAction::Continue)
            }

            // Down arrow
            (KeyCode::Down, _) => {
                self.move_cursor_down();
                Ok(EditorAction::Continue)
            }

            // Home: beginning of line
            (KeyCode::Home, _) => {
                self.cursor_col = 0;
                Ok(EditorAction::Continue)
            }

            // End: end of line
            (KeyCode::End, _) => {
                self.cursor_col = self.current_line().chars().count();
                Ok(EditorAction::Continue)
            }

            // Input character
            (KeyCode::Char(c), _) => {
                self.insert_char(c);
                Ok(EditorAction::Continue)
            }

            _ => Ok(EditorAction::Continue),
        }
    }

    fn render(&self, stdout: &mut impl Write, term_width: u16, agent_name: &str, chat_id: Option<&str>) -> Result<()> {
        let (_, term_height) = terminal::size()?;

        // Use the recorded start row, or calculate one if not set
        let start_row = self.start_row.unwrap_or_else(|| {
            let input_lines = self.lines.len() as u16;
            let total_height = input_lines + 4;
            term_height.saturating_sub(total_height)
        });

        let input_lines = self.lines.len() as u16;
        let total_height = input_lines + 4;  // top border + input lines + bottom border + status bar + help

        // Clear input area (including possibly expanded lines)
        for i in 0..(total_height + 5) {  // Clear a few extra lines to prevent artifacts
            queue!(
                stdout,
                cursor::MoveTo(0, start_row + i),
                Clear(ClearType::CurrentLine)
            )?;
        }

        // Draw top border
        queue!(
            stdout,
            cursor::MoveTo(0, start_row),
            SetForegroundColor(Color::DarkGrey),
            Print("─".repeat(term_width as usize)),
            ResetColor,
        )?;

        // Draw input content
        for (i, line) in self.lines.iter().enumerate() {
            queue!(
                stdout,
                cursor::MoveTo(0, start_row + 1 + i as u16)
            )?;
            if i == 0 {
                queue!(stdout, Print("> "), Print(line))?;
            } else {
                queue!(stdout, Print("  "), Print(line))?;
            }
        }

        // Draw bottom border
        let bottom_border = start_row + 1 + input_lines;
        queue!(
            stdout,
            cursor::MoveTo(0, bottom_border),
            SetForegroundColor(Color::DarkGrey),
            Print("─".repeat(term_width as usize)),
            ResetColor,
        )?;

        // Status bar position
        let status_row = bottom_border + 1;

        // Draw status bar (agent name and chat id)
        queue!(
            stdout,
            cursor::MoveTo(0, status_row),
            SetForegroundColor(Color::DarkGrey),
            Print(format!("● {}", agent_name)),
        )?;

        if let Some(id) = chat_id {
            queue!(
                stdout,
                Print(" ["),
                Print(&id[..8.min(id.len())]),
                Print("]"),
            )?;
        }

        queue!(stdout, ResetColor)?;

        // Draw help info
        queue!(
            stdout,
            cursor::MoveTo(0, status_row + 1),
            SetForegroundColor(Color::DarkGrey),
            Print("Enter: submit │ Shift+Enter: new line │ Ctrl+C: cancel │ Ctrl+D: exit"),
            ResetColor,
        )?;

        // Move cursor to editing position
        let cursor_x = 2 + self.cursor_col;
        let cursor_y = start_row + 1 + self.cursor_row as u16;
        queue!(stdout, cursor::MoveTo(cursor_x as u16, cursor_y))?;

        stdout.flush()?;
        Ok(())
    }

    fn insert_char(&mut self, c: char) {
        let line = &mut self.lines[self.cursor_row];
        // Find the correct byte index
        let byte_pos = line.char_indices()
            .nth(self.cursor_col)
            .map(|(pos, _)| pos)
            .unwrap_or(line.len());
        line.insert(byte_pos, c);
        self.cursor_col += 1;
    }

    fn insert_newline(&mut self) {
        let current_line = self.lines[self.cursor_row].clone();

        // Find the correct byte index to split at
        let byte_pos = current_line.char_indices()
            .nth(self.cursor_col)
            .map(|(pos, _)| pos)
            .unwrap_or(current_line.len());

        let (before, after) = current_line.split_at(byte_pos);

        self.lines[self.cursor_row] = before.to_string();
        self.lines.insert(self.cursor_row + 1, after.to_string());

        self.cursor_row += 1;
        self.cursor_col = 0;
    }

    fn backspace(&mut self) {
        if self.cursor_col > 0 {
            let line = &mut self.lines[self.cursor_row];
            // Find the byte position of the character to delete
            let byte_pos = line.char_indices()
                .nth(self.cursor_col - 1)
                .map(|(pos, _)| pos)
                .unwrap_or(0);
            line.remove(byte_pos);
            self.cursor_col -= 1;
        } else if self.cursor_row > 0 {
            // Merge with previous line
            let current = self.lines.remove(self.cursor_row);
            self.cursor_row -= 1;
            self.cursor_col = self.lines[self.cursor_row].chars().count();
            self.lines[self.cursor_row].push_str(&current);
        }
    }

    fn delete(&mut self) {
        let line = &mut self.lines[self.cursor_row];
        let char_count = line.chars().count();

        if self.cursor_col < char_count {
            // Find the byte position of the character to delete
            let byte_pos = line.char_indices()
                .nth(self.cursor_col)
                .map(|(pos, _)| pos)
                .unwrap_or(line.len());
            if byte_pos < line.len() {
                line.remove(byte_pos);
            }
        } else if self.cursor_row < self.lines.len() - 1 {
            // Merge with next line
            let next = self.lines.remove(self.cursor_row + 1);
            self.lines[self.cursor_row].push_str(&next);
        }
    }

    fn move_cursor_left(&mut self) {
        if self.cursor_col > 0 {
            self.cursor_col -= 1;
        } else if self.cursor_row > 0 {
            self.cursor_row -= 1;
            self.cursor_col = self.current_line().len();
        }
    }

    fn move_cursor_right(&mut self) {
        let char_count = self.current_line().chars().count();
        if self.cursor_col < char_count {
            self.cursor_col += 1;
        } else if self.cursor_row < self.lines.len() - 1 {
            self.cursor_row += 1;
            self.cursor_col = 0;
        }
    }

    fn move_cursor_up(&mut self) {
        if self.cursor_row > 0 {
            self.cursor_row -= 1;
            let char_count = self.current_line().chars().count();
            self.cursor_col = self.cursor_col.min(char_count);
        }
    }

    fn move_cursor_down(&mut self) {
        if self.cursor_row < self.lines.len() - 1 {
            self.cursor_row += 1;
            let char_count = self.current_line().chars().count();
            self.cursor_col = self.cursor_col.min(char_count);
        }
    }

    fn current_line(&self) -> &String {
        &self.lines[self.cursor_row]
    }

    fn clear(&mut self) {
        self.lines = vec![String::new()];
        self.cursor_row = 0;
        self.cursor_col = 0;
        self.start_row = None;
    }
}

enum EditorAction {
    Submit,
    Cancel,
    Exit,
    Continue,
}
