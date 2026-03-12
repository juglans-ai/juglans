// src/ui/input.rs
//! Simple text input component

use anyhow::Result;
use std::io::{self, Write};

pub struct MultilineInput;

impl Default for MultilineInput {
    fn default() -> Self {
        Self::new()
    }
}

impl MultilineInput {
    pub fn new() -> Self {
        Self
    }

    pub fn prompt(&mut self, agent_name: &str, chat_id: Option<&str>) -> Result<Option<String>> {
        // Build prompt
        let mut prompt = format!("USER@{}", agent_name);
        if let Some(id) = chat_id {
            prompt.push_str(&format!("[{}]", &id[..8.min(id.len())]));
        }
        prompt.push_str("> ");

        // Display prompt
        print!("{}", prompt);
        io::stdout().flush()?;

        // Read a line of input
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
