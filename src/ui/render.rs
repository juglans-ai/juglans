// src/ui/render.rs
//! Markdown rendering and status display utilities

use ::crossterm::style::Stylize;
use termimad::*;

/// 渲染 Markdown 文本（带颜色和格式）
pub fn render_markdown(content: &str) {
    let mut skin = MadSkin::default();

    // 自定义颜色方案（使用 termimad 的 Color）
    skin.bold.set_fg(crossterm::style::Color::Yellow);
    skin.italic.set_fg(crossterm::style::Color::Cyan);
    skin.headers[0].set_fg(crossterm::style::Color::Magenta);
    skin.headers[1].set_fg(crossterm::style::Color::Blue);
    skin.code_block.set_fg(crossterm::style::Color::Green);
    skin.inline_code.set_fg(crossterm::style::Color::Green);

    // 渲染
    println!("{}", skin.term_text(content));
}

/// 显示状态栏
pub fn show_status(chat_id: Option<&str>, status: &str, depth: usize) {
    println!();

    // Chat ID
    if let Some(id) = chat_id {
        println!("  {} {}", "●".dark_grey(), id.dark_grey());
    }

    // 状态
    let status_colored = match status {
        "processing" => status.yellow(),
        "completed" => status.green(),
        "error" => status.red(),
        _ => status.white(),
    };
    println!("  {} {}", "Status:".dark_grey(), status_colored);

    // 嵌套深度
    if depth > 0 {
        println!("  {} {}", "Depth:".dark_grey(), depth.to_string().cyan());
    }

    println!();
}

/// 显示欢迎信息
pub fn show_welcome(agent_name: &str, agent_slug: &str, has_workflow: bool) {
    eprintln!();
    eprintln!("● Agent: {}", agent_name);
    eprintln!("  ID: {}", agent_slug);

    if has_workflow {
        eprintln!("  ⚡ Workflow enabled");
    }

    eprintln!();
    eprintln!("  Commands: 'help' | 'exit' | 'quit'");
    eprintln!();
}

/// 显示帮助信息
pub fn show_shortcuts() {
    println!();
    println!("{}", "Keyboard Shortcuts:".bold());
    println!();
    println!("  {}         Submit message", "Enter".cyan());
    println!("  {}   New line", "Shift+Enter".cyan());
    println!("  {}        Cancel input", "Ctrl+C".cyan());
    println!("  {}      Exit program (or type 'exit'/'quit')", "Ctrl+D".cyan());
    println!();
}
