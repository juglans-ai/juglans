pub mod app;
pub mod claude_code;
pub mod dialog;
pub mod editor;
pub mod event;
pub mod markdown;
pub mod messages;
pub mod sidebar;
pub mod status_bar;
pub mod theme;
pub mod ui;
pub mod welcome;

use anyhow::Result;
use app::App;
use crossterm::event::{
    DisableMouseCapture, EnableMouseCapture, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags,
    PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use event::EventHandler;
use std::time::Duration;

pub async fn run(mut app: App) -> Result<()> {
    let mut terminal = ratatui::init();
    execute!(std::io::stdout(), EnableMouseCapture)?;

    // Enable enhanced keyboard protocol (Kitty) so Shift+Enter is reported correctly
    let keyboard_enhanced = crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false);
    if keyboard_enhanced {
        execute!(
            std::io::stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )?;
    }

    let mut events = EventHandler::new(Duration::from_millis(50));

    while !app.should_quit {
        terminal.draw(|f| ui::draw(f, &app))?;

        // Handle pending message: spawn new subprocess or send to existing one
        if let Some(text) = app.pending_send_message.take() {
            if app.claude_process.is_some() {
                app.send_user_message_to_subprocess(text).await;
            } else {
                app.do_spawn_claude(text).await;
            }
        }

        // Send any pending permission response to the subprocess
        if let Some(json) = app.pending_permission_response.take() {
            if let Some(ref mut proc) = app.claude_process {
                if let Err(e) = proc.send_response(&json).await {
                    tracing::debug!("Failed to send permission response: {}", e);
                }
            }
        }

        // Multiplex UI events and Claude Code subprocess events
        if let Some(mut claude_rx) = app.claude_rx.take() {
            tokio::select! {
                ui_event = events.next() => {
                    match ui_event {
                        Ok(event) => app.update(event),
                        Err(_) => {
                            app.claude_rx = Some(claude_rx);
                            break;
                        }
                    }
                }
                claude_event = claude_rx.recv() => {
                    match claude_event {
                        Some(ev) => app.handle_claude_event(ev),
                        None => {
                            // Channel closed without ProcessExited
                            app.handle_claude_event(
                                claude_code::ClaudeEvent::ProcessExited {
                                    _success: true,
                                    error: None,
                                }
                            );
                        }
                    }
                }
            }
            // Drain all remaining Claude events before next draw.
            // This ensures streaming text appears in bulk per frame
            // rather than one token per frame.
            while let Ok(ev) = claude_rx.try_recv() {
                app.handle_claude_event(ev);
            }
            app.claude_rx = Some(claude_rx);
        } else {
            let event = events.next().await?;
            app.update(event);
        }
    }

    // Cleanup: kill any running subprocess
    if let Some(mut proc) = app.claude_process.take() {
        proc.kill();
    }

    if keyboard_enhanced {
        let _ = execute!(std::io::stdout(), PopKeyboardEnhancementFlags);
    }
    execute!(std::io::stdout(), DisableMouseCapture)?;
    ratatui::restore();
    Ok(())
}
