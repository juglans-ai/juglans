// src/ui/input.rs
//! 简单的文本输入组件

use anyhow::Result;
use std::io::{self, Write};

pub struct MultilineInput;

impl MultilineInput {
    pub fn new() -> Self {
        Self
    }

    pub fn prompt(&mut self, agent_name: &str, chat_id: Option<&str>) -> Result<Option<String>> {
        // 构建提示符
        let mut prompt = format!("USER@{}", agent_name);
        if let Some(id) = chat_id {
            prompt.push_str(&format!("[{}]", &id[..8.min(id.len())]));
        }
        prompt.push_str("> ");

        // 显示提示符
        print!("{}", prompt);
        io::stdout().flush()?;

        // 读取一行输入
        let mut input = String::new();
        match io::stdin().read_line(&mut input) {
            Ok(0) => {
                // EOF (Ctrl+D)
                Ok(None)
            }
            Ok(_) => {
                let trimmed = input.trim();
                if trimmed.is_empty() {
                    Ok(Some(String::new()))
                } else {
                    Ok(Some(trimmed.to_string()))
                }
            }
            Err(e) => Err(e.into()),
        }
    }
}