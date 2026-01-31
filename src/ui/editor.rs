// src/ui/editor.rs
//! 自定义多行编辑器，支持 Shift+Enter 换行

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
    start_row: Option<u16>,  // 记录输入框的起始行号
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

    /// 编辑多行文本
    /// Enter: 提交
    /// Shift+Enter: 换行
    /// Ctrl+C: 取消
    pub fn edit(&mut self, agent_name: &str, chat_id: Option<&str>) -> Result<Option<String>> {
        let mut stdout = stdout();

        // 获取当前光标位置
        let (_, term_height) = terminal::size()?;

        // 为输入框腾出空间：计算需要的行数
        let total_height = 5u16;  // 上边框 + 输入行 + 下边框 + 状态栏 + 帮助

        // 确保有足够空间
        for _ in 0..total_height {
            println!();
        }
        stdout.flush()?;

        // 记录输入框的起始行（从底部算起的固定位置）
        self.start_row = Some(term_height.saturating_sub(total_height));

        // 进入 raw mode（保持光标可见）
        terminal::enable_raw_mode()?;

        let result = self.edit_loop(agent_name, chat_id);

        // 退出 raw mode，清除输入区域
        if let Some(start) = self.start_row {
            for i in 0..(total_height + self.lines.len() as u16) {
                queue!(stdout, cursor::MoveTo(0, start + i), Clear(ClearType::CurrentLine))?;
            }
        }

        terminal::disable_raw_mode()?;

        // 重置状态
        self.start_row = None;

        result
    }

    fn edit_loop(&mut self, agent_name: &str, chat_id: Option<&str>) -> Result<Option<String>> {
        let mut stdout = stdout();

        // 获取终端尺寸
        let (term_width, _) = terminal::size()?;

        loop {
            // 重绘界面
            self.render(&mut stdout, term_width, agent_name, chat_id)?;

            // 读取键盘事件
            if let Event::Key(key) = event::read()? {
                match self.handle_key(key)? {
                    EditorAction::Submit => {
                        // 提交内容
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
                        // 继续编辑
                    }
                }
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> Result<EditorAction> {
        match (key.code, key.modifiers) {
            // Enter: 提交
            (KeyCode::Enter, KeyModifiers::NONE) => {
                Ok(EditorAction::Submit)
            }

            // Shift+Enter: 换行
            (KeyCode::Enter, KeyModifiers::SHIFT) => {
                self.insert_newline();
                Ok(EditorAction::Continue)
            }

            // Ctrl+C: 取消
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                Ok(EditorAction::Cancel)
            }

            // Ctrl+D: 退出
            (KeyCode::Char('d'), KeyModifiers::CONTROL) => {
                Ok(EditorAction::Exit)
            }

            // Backspace: 删除字符
            (KeyCode::Backspace, _) => {
                self.backspace();
                Ok(EditorAction::Continue)
            }

            // Delete: 删除右侧字符
            (KeyCode::Delete, _) => {
                self.delete();
                Ok(EditorAction::Continue)
            }

            // 左箭头
            (KeyCode::Left, _) => {
                self.move_cursor_left();
                Ok(EditorAction::Continue)
            }

            // 右箭头
            (KeyCode::Right, _) => {
                self.move_cursor_right();
                Ok(EditorAction::Continue)
            }

            // 上箭头
            (KeyCode::Up, _) => {
                self.move_cursor_up();
                Ok(EditorAction::Continue)
            }

            // 下箭头
            (KeyCode::Down, _) => {
                self.move_cursor_down();
                Ok(EditorAction::Continue)
            }

            // Home: 行首
            (KeyCode::Home, _) => {
                self.cursor_col = 0;
                Ok(EditorAction::Continue)
            }

            // End: 行尾
            (KeyCode::End, _) => {
                self.cursor_col = self.current_line().chars().count();
                Ok(EditorAction::Continue)
            }

            // 输入字符
            (KeyCode::Char(c), _) => {
                self.insert_char(c);
                Ok(EditorAction::Continue)
            }

            _ => Ok(EditorAction::Continue),
        }
    }

    fn render(&self, stdout: &mut impl Write, term_width: u16, agent_name: &str, chat_id: Option<&str>) -> Result<()> {
        let (_, term_height) = terminal::size()?;

        // 使用记录的起始行，如果没有则计算一个
        let start_row = self.start_row.unwrap_or_else(|| {
            let input_lines = self.lines.len() as u16;
            let total_height = input_lines + 4;
            term_height.saturating_sub(total_height)
        });

        let input_lines = self.lines.len() as u16;
        let total_height = input_lines + 4;  // 上边框 + 输入行 + 下边框 + 状态栏 + 帮助

        // 清除输入区域（包括可能增长的行数）
        for i in 0..(total_height + 5) {  // 额外清除几行，防止残留
            queue!(
                stdout,
                cursor::MoveTo(0, start_row + i),
                Clear(ClearType::CurrentLine)
            )?;
        }

        // 绘制上边框
        queue!(
            stdout,
            cursor::MoveTo(0, start_row),
            SetForegroundColor(Color::DarkGrey),
            Print("─".repeat(term_width as usize)),
            ResetColor,
        )?;

        // 绘制输入内容
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

        // 绘制下边框
        let bottom_border = start_row + 1 + input_lines;
        queue!(
            stdout,
            cursor::MoveTo(0, bottom_border),
            SetForegroundColor(Color::DarkGrey),
            Print("─".repeat(term_width as usize)),
            ResetColor,
        )?;

        // 状态栏位置
        let status_row = bottom_border + 1;

        // 绘制状态栏（agent 名称和 chat id）
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

        // 绘制帮助信息
        queue!(
            stdout,
            cursor::MoveTo(0, status_row + 1),
            SetForegroundColor(Color::DarkGrey),
            Print("Enter: submit │ Shift+Enter: new line │ Ctrl+C: cancel │ Ctrl+D: exit"),
            ResetColor,
        )?;

        // 移动光标到编辑位置
        let cursor_x = 2 + self.cursor_col;
        let cursor_y = start_row + 1 + self.cursor_row as u16;
        queue!(stdout, cursor::MoveTo(cursor_x as u16, cursor_y))?;

        stdout.flush()?;
        Ok(())
    }

    fn insert_char(&mut self, c: char) {
        let line = &mut self.lines[self.cursor_row];
        // 找到正确的字节索引
        let byte_pos = line.char_indices()
            .nth(self.cursor_col)
            .map(|(pos, _)| pos)
            .unwrap_or(line.len());
        line.insert(byte_pos, c);
        self.cursor_col += 1;
    }

    fn insert_newline(&mut self) {
        let current_line = self.lines[self.cursor_row].clone();

        // 找到正确的字节索引来分割
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
            // 找到要删除的字符的字节位置
            let byte_pos = line.char_indices()
                .nth(self.cursor_col - 1)
                .map(|(pos, _)| pos)
                .unwrap_or(0);
            line.remove(byte_pos);
            self.cursor_col -= 1;
        } else if self.cursor_row > 0 {
            // 合并到上一行
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
            // 找到要删除的字符的字节位置
            let byte_pos = line.char_indices()
                .nth(self.cursor_col)
                .map(|(pos, _)| pos)
                .unwrap_or(line.len());
            if byte_pos < line.len() {
                line.remove(byte_pos);
            }
        } else if self.cursor_row < self.lines.len() - 1 {
            // 合并下一行
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
