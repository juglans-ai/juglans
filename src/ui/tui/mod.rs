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

use anyhow::{Context, Result};
use app::{AgentState, App};
use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::execute;
use event::EventHandler;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;

use crate::core::context::WorkflowContext;
use crate::core::executor::WorkflowExecutor;
use crate::core::parser::GraphParser;
use crate::core::resolver;
use crate::services::config::JuglansConfig;
use crate::services::local_runtime::LocalRuntime;
use crate::services::prompt_loader::PromptRegistry;

pub async fn run(mut app: App) -> Result<()> {
    let mut terminal = ratatui::init();
    execute!(std::io::stdout(), EnableMouseCapture)?;
    execute!(std::io::stdout(), EnableBracketedPaste)?;
    execute!(std::io::stdout(), crossterm::terminal::SetTitle("Juglans"))?;

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

        // Handle pending agent load
        if let Some(path) = app.pending_agent_load.take() {
            match load_agent(&path).await {
                Ok((state, name)) => {
                    app.agent_name = Some(name);
                    app.agent_state = Some(state);
                    app.status_message = Some((
                        format!("Agent loaded: {}", app.agent_name.as_deref().unwrap_or("?")),
                        std::time::Instant::now(),
                    ));
                }
                Err(e) => {
                    app.status_message = Some((
                        format!("Failed to load agent: {}", e),
                        std::time::Instant::now(),
                    ));
                }
            }
        }

        // Handle pending agent message: execute agent
        if let Some(text) = app.pending_agent_message.take() {
            app.do_execute_agent(text).await;
        }

        // Handle pending claude message: spawn new subprocess or send to existing one
        if let Some(msg) = app.pending_send_message.take() {
            if app.claude_process.is_some() {
                app.send_user_message_to_subprocess(msg).await;
            } else {
                app.do_spawn_claude(msg).await;
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

        // Triple-multiplex: UI events + Claude events + Agent events
        let has_claude_rx = app.claude_rx.is_some();
        let has_agent_rx = app
            .agent_state
            .as_ref()
            .is_some_and(|s| s.event_rx.is_some());

        match (has_claude_rx, has_agent_rx) {
            (true, _) => {
                // Claude Code mode: select UI + Claude events
                let mut claude_rx = app.claude_rx.take().unwrap();
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
                // Drain remaining Claude events
                while let Ok(ev) = claude_rx.try_recv() {
                    app.handle_claude_event(ev);
                }
                app.claude_rx = Some(claude_rx);
            }
            (false, true) => {
                // Agent mode: select UI + Agent events
                let mut agent_rx = app.agent_state.as_mut().unwrap().event_rx.take().unwrap();
                tokio::select! {
                    ui_event = events.next() => {
                        match ui_event {
                            Ok(event) => app.update(event),
                            Err(_) => {
                                app.agent_state.as_mut().unwrap().event_rx = Some(agent_rx);
                                break;
                            }
                        }
                    }
                    agent_event = agent_rx.recv() => {
                        if let Some(ev) = agent_event {
                            app.handle_agent_event(ev);
                        }
                    }
                }
                // Drain remaining Agent events
                while let Ok(ev) = agent_rx.try_recv() {
                    app.handle_agent_event(ev);
                }
                if let Some(state) = app.agent_state.as_mut() {
                    state.event_rx = Some(agent_rx);
                }
            }
            _ => {
                // No active stream: just poll UI events
                let event = events.next().await?;
                app.update(event);
            }
        }
    }

    // Cleanup: kill any running subprocess
    if let Some(mut proc) = app.claude_process.take() {
        proc.kill();
    }
    // Cleanup: abort any running agent task
    if let Some(state) = &mut app.agent_state {
        if let Some(handle) = state.current_task.take() {
            handle.abort();
        }
    }

    if keyboard_enhanced {
        let _ = execute!(std::io::stdout(), PopKeyboardEnhancementFlags);
    }
    execute!(std::io::stdout(), DisableBracketedPaste)?;
    execute!(std::io::stdout(), DisableMouseCapture)?;
    let _ = execute!(std::io::stdout(), crossterm::terminal::SetTitle(""));
    ratatui::restore();
    Ok(())
}

/// Resolve import patterns: expand @ prefixes and make paths absolute
fn resolve_patterns(base_dir: &Path, patterns: &[String], at_base: Option<&Path>) -> Vec<String> {
    let expanded = resolver::expand_at_prefixes(patterns, at_base);
    expanded
        .iter()
        .map(|p| {
            if Path::new(p).is_absolute() {
                p.clone()
            } else {
                base_dir.join(p).to_string_lossy().to_string()
            }
        })
        .collect()
}

/// Load a .jg workflow file and create an AgentState for TUI use
async fn load_agent(path: &Path) -> Result<(AgentState, String)> {
    let source = std::fs::read_to_string(path)
        .with_context(|| format!("Cannot read workflow file: {:?}", path))?;

    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let config = JuglansConfig::load()?;

    let base_dir = path.parent().unwrap_or(Path::new("."));

    // Compute @ path alias base directory
    let at_base: Option<std::path::PathBuf> = config.paths.base.as_ref().map(|b| base_dir.join(b));

    let mut prompt_registry = PromptRegistry::new();

    let mut wf_parsed = GraphParser::parse(&source)?;
    let wf_dir = base_dir;

    // Resolve lib imports
    let wf_canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut import_stack = vec![wf_canonical.clone()];
    resolver::resolve_lib_imports(
        &mut wf_parsed,
        wf_dir,
        &mut import_stack,
        at_base.as_deref(),
    )?;

    // Resolve flow imports
    import_stack = vec![wf_canonical];
    resolver::resolve_flow_imports(
        &mut wf_parsed,
        wf_dir,
        &mut import_stack,
        at_base.as_deref(),
    )?;

    // Load prompt/tool patterns
    let p_paths = resolve_patterns(wf_dir, &wf_parsed.prompt_patterns, at_base.as_deref());

    prompt_registry.load_from_paths(&p_paths)?;

    // Update tool patterns
    wf_parsed.tool_patterns =
        resolve_patterns(wf_dir, &wf_parsed.tool_patterns, at_base.as_deref());

    let workflow = Some(Arc::new(wf_parsed));

    // Build executor
    let runtime: Arc<LocalRuntime> = Arc::new(LocalRuntime::new_with_config(&config.ai));
    let mut executor =
        WorkflowExecutor::new_with_debug(Arc::new(prompt_registry), runtime, config.debug.clone())
            .await;

    executor.apply_limits(&config.limits);

    if let Some(wf) = &workflow {
        executor.load_tools(wf).await;
        if let Err(e) = executor.init_python_runtime(wf, config.limits.python_workers) {
            tracing::warn!("Failed to initialize Python runtime: {}", e);
        }
    }

    let shared = Arc::new(executor);
    shared.get_registry().set_executor(Arc::downgrade(&shared));

    let (tx, rx) = mpsc::unbounded_channel();
    let context = WorkflowContext::with_sender(tx.clone());

    Ok((
        AgentState {
            executor: shared,
            context,
            model: String::new(),
            slug: name.clone(),
            workflow,
            event_rx: Some(rx),
            event_tx: tx,
            current_task: None,
        },
        name,
    ))
}
