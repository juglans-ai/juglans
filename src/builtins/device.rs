// src/builtins/device.rs
//
// Device control builtins: keyboard/mouse simulation, event listening, screenshot.
//
// Keyboard: key_tap, key_combo, type_text
// Mouse:    mouse_move, mouse_click, mouse_scroll, mouse_position, mouse_drag
// Screen:   screen_size, screenshot
// Listen:   key_listen, mouse_listen (callback handler style)

use super::Tool;
use crate::core::context::WorkflowContext;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use enigo::{Axis, Button, Coordinate, Direction, Enigo, Keyboard, Mouse, Settings};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::info;

// ---------------------------------------------------------------------------
// Key name resolution
// ---------------------------------------------------------------------------

fn parse_enigo_key(name: &str) -> Result<enigo::Key> {
    let lower = name.to_lowercase();
    let key = match lower.as_str() {
        // Modifiers
        "shift" => enigo::Key::Shift,
        "ctrl" | "control" => enigo::Key::Control,
        "alt" | "option" => enigo::Key::Alt,
        "meta" | "cmd" | "command" | "win" | "super" => enigo::Key::Meta,
        // Special
        "enter" | "return" => enigo::Key::Return,
        "tab" => enigo::Key::Tab,
        "space" => enigo::Key::Space,
        "backspace" => enigo::Key::Backspace,
        "delete" | "del" => enigo::Key::Delete,
        "escape" | "esc" => enigo::Key::Escape,
        "capslock" => enigo::Key::CapsLock,
        // Navigation
        "up" => enigo::Key::UpArrow,
        "down" => enigo::Key::DownArrow,
        "left" => enigo::Key::LeftArrow,
        "right" => enigo::Key::RightArrow,
        "home" => enigo::Key::Home,
        "end" => enigo::Key::End,
        "pageup" => enigo::Key::PageUp,
        "pagedown" => enigo::Key::PageDown,
        // Function keys
        "f1" => enigo::Key::F1,
        "f2" => enigo::Key::F2,
        "f3" => enigo::Key::F3,
        "f4" => enigo::Key::F4,
        "f5" => enigo::Key::F5,
        "f6" => enigo::Key::F6,
        "f7" => enigo::Key::F7,
        "f8" => enigo::Key::F8,
        "f9" => enigo::Key::F9,
        "f10" => enigo::Key::F10,
        "f11" => enigo::Key::F11,
        "f12" => enigo::Key::F12,
        "f13" => enigo::Key::F13,
        "f14" => enigo::Key::F14,
        "f15" => enigo::Key::F15,
        "f16" => enigo::Key::F16,
        "f17" => enigo::Key::F17,
        "f18" => enigo::Key::F18,
        "f19" => enigo::Key::F19,
        "f20" => enigo::Key::F20,
        // Single character
        s if s.len() == 1 => enigo::Key::Unicode(s.chars().next().unwrap()),
        _ => return Err(anyhow!("Unknown key name: '{}'", name)),
    };
    Ok(key)
}

fn parse_button(name: &str) -> Button {
    match name.to_lowercase().as_str() {
        "right" => Button::Right,
        "middle" => Button::Middle,
        _ => Button::Left,
    }
}

/// Wrap enigo errors with macOS accessibility permission hint
fn wrap_enigo_error(e: impl std::fmt::Display) -> anyhow::Error {
    let msg = e.to_string();
    if msg.contains("ccessib") || msg.contains("ermission") || msg.contains("not trusted") {
        anyhow!(
            "{}\n\nmacOS 需要辅助功能权限：系统设置 > 隐私与安全性 > 辅助功能\n\
             将终端应用（Terminal/iTerm2/VS Code）添加到列表中。",
            msg
        )
    } else {
        anyhow!("Device error: {}", msg)
    }
}

fn new_enigo() -> Result<Enigo> {
    Enigo::new(&Settings::default()).map_err(wrap_enigo_error)
}

// ---------------------------------------------------------------------------
// key_tap(key="a")
// ---------------------------------------------------------------------------

pub struct KeyTap;
#[async_trait]
impl Tool for KeyTap {
    fn name(&self) -> &str {
        "key_tap"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let key_name = params
            .get("key")
            .ok_or_else(|| anyhow!("key_tap() requires 'key' parameter"))?;
        let key = parse_enigo_key(key_name)?;
        let mut enigo = new_enigo()?;
        enigo.key(key, Direction::Click).map_err(wrap_enigo_error)?;
        Ok(Some(json!({"status": "ok", "key": key_name})))
    }
}

// ---------------------------------------------------------------------------
// key_combo(keys=["meta","c"])
// ---------------------------------------------------------------------------

pub struct KeyCombo;
#[async_trait]
impl Tool for KeyCombo {
    fn name(&self) -> &str {
        "key_combo"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let keys_str = params
            .get("keys")
            .ok_or_else(|| anyhow!("key_combo() requires 'keys' parameter (JSON array)"))?;
        let keys_arr: Vec<String> = serde_json::from_str(keys_str)
            .map_err(|_| anyhow!("key_combo(): 'keys' must be a JSON array of strings"))?;
        if keys_arr.is_empty() {
            return Err(anyhow!("key_combo(): 'keys' array is empty"));
        }

        let parsed: Vec<enigo::Key> = keys_arr
            .iter()
            .map(|k| parse_enigo_key(k))
            .collect::<Result<Vec<_>>>()?;

        let mut enigo = new_enigo()?;

        // Press all keys in order
        for key in &parsed {
            enigo
                .key(*key, Direction::Press)
                .map_err(wrap_enigo_error)?;
        }
        // Release in reverse order
        for key in parsed.iter().rev() {
            enigo
                .key(*key, Direction::Release)
                .map_err(wrap_enigo_error)?;
        }

        Ok(Some(json!({"status": "ok", "keys": keys_arr})))
    }
}

// ---------------------------------------------------------------------------
// type_text(text="hello", delay_ms=50)
// ---------------------------------------------------------------------------

pub struct TypeText;
#[async_trait]
impl Tool for TypeText {
    fn name(&self) -> &str {
        "type_text"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let text = params
            .get("text")
            .ok_or_else(|| anyhow!("type_text() requires 'text' parameter"))?;
        let delay_ms: u64 = params
            .get("delay_ms")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        let text = text.clone();
        let len = text.len();

        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut enigo = new_enigo()?;
            if delay_ms == 0 {
                enigo.text(&text).map_err(wrap_enigo_error)?;
            } else {
                for ch in text.chars() {
                    enigo
                        .key(enigo::Key::Unicode(ch), Direction::Click)
                        .map_err(wrap_enigo_error)?;
                    std::thread::sleep(std::time::Duration::from_millis(delay_ms));
                }
            }
            Ok(())
        })
        .await
        .map_err(|e| anyhow!("type_text spawn error: {}", e))??;

        Ok(Some(json!({"status": "ok", "length": len})))
    }
}

// ---------------------------------------------------------------------------
// mouse_move(x=100, y=200, relative=false)
// ---------------------------------------------------------------------------

pub struct MouseMove;
#[async_trait]
impl Tool for MouseMove {
    fn name(&self) -> &str {
        "mouse_move"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let x: i32 = params.get("x").and_then(|s| s.parse().ok()).unwrap_or(0);
        let y: i32 = params.get("y").and_then(|s| s.parse().ok()).unwrap_or(0);
        let relative = params.get("relative").map(|s| s == "true").unwrap_or(false);

        let mut enigo = new_enigo()?;
        let coord = if relative {
            Coordinate::Rel
        } else {
            Coordinate::Abs
        };
        enigo.move_mouse(x, y, coord).map_err(wrap_enigo_error)?;

        Ok(Some(json!({"status": "ok", "x": x, "y": y})))
    }
}

// ---------------------------------------------------------------------------
// mouse_click(button="left", count=1)
// ---------------------------------------------------------------------------

pub struct MouseClick;
#[async_trait]
impl Tool for MouseClick {
    fn name(&self) -> &str {
        "mouse_click"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let button = parse_button(params.get("button").map(|s| s.as_str()).unwrap_or("left"));
        let count: u32 = params
            .get("count")
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);

        let mut enigo = new_enigo()?;
        for _ in 0..count {
            enigo
                .button(button, Direction::Click)
                .map_err(wrap_enigo_error)?;
        }

        let btn_name = params.get("button").map(|s| s.as_str()).unwrap_or("left");
        Ok(Some(
            json!({"status": "ok", "button": btn_name, "count": count}),
        ))
    }
}

// ---------------------------------------------------------------------------
// mouse_scroll(x=0, y=-3)
// ---------------------------------------------------------------------------

pub struct MouseScroll;
#[async_trait]
impl Tool for MouseScroll {
    fn name(&self) -> &str {
        "mouse_scroll"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let x: i32 = params.get("x").and_then(|s| s.parse().ok()).unwrap_or(0);
        let y: i32 = params.get("y").and_then(|s| s.parse().ok()).unwrap_or(0);

        let mut enigo = new_enigo()?;
        if y != 0 {
            enigo.scroll(y, Axis::Vertical).map_err(wrap_enigo_error)?;
        }
        if x != 0 {
            enigo
                .scroll(x, Axis::Horizontal)
                .map_err(wrap_enigo_error)?;
        }

        Ok(Some(json!({"status": "ok", "x": x, "y": y})))
    }
}

// ---------------------------------------------------------------------------
// mouse_position() → {x, y}
// ---------------------------------------------------------------------------

pub struct MousePosition;
#[async_trait]
impl Tool for MousePosition {
    fn name(&self) -> &str {
        "mouse_position"
    }
    async fn execute(
        &self,
        _params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let enigo = new_enigo()?;
        let (x, y) = enigo.location().map_err(wrap_enigo_error)?;
        Ok(Some(json!({"x": x, "y": y})))
    }
}

// ---------------------------------------------------------------------------
// mouse_drag(x1=0, y1=0, x2=100, y2=100, button="left", duration_ms=300)
// ---------------------------------------------------------------------------

pub struct MouseDrag;
#[async_trait]
impl Tool for MouseDrag {
    fn name(&self) -> &str {
        "mouse_drag"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let x1: i32 = params.get("x1").and_then(|s| s.parse().ok()).unwrap_or(0);
        let y1: i32 = params.get("y1").and_then(|s| s.parse().ok()).unwrap_or(0);
        let x2: i32 = params.get("x2").and_then(|s| s.parse().ok()).unwrap_or(0);
        let y2: i32 = params.get("y2").and_then(|s| s.parse().ok()).unwrap_or(0);
        let button = parse_button(params.get("button").map(|s| s.as_str()).unwrap_or("left"));
        let duration_ms: u64 = params
            .get("duration_ms")
            .and_then(|s| s.parse().ok())
            .unwrap_or(300);

        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut enigo = new_enigo()?;

            // Move to start position
            enigo
                .move_mouse(x1, y1, Coordinate::Abs)
                .map_err(wrap_enigo_error)?;
            std::thread::sleep(std::time::Duration::from_millis(50));

            // Press button
            enigo
                .button(button, Direction::Press)
                .map_err(wrap_enigo_error)?;

            // Interpolate movement over duration
            let steps = 20u32;
            let step_delay = duration_ms / steps as u64;
            for i in 1..=steps {
                let t = i as f64 / steps as f64;
                let cx = x1 + ((x2 - x1) as f64 * t) as i32;
                let cy = y1 + ((y2 - y1) as f64 * t) as i32;
                enigo
                    .move_mouse(cx, cy, Coordinate::Abs)
                    .map_err(wrap_enigo_error)?;
                std::thread::sleep(std::time::Duration::from_millis(step_delay));
            }

            // Release button
            enigo
                .button(button, Direction::Release)
                .map_err(wrap_enigo_error)?;
            Ok(())
        })
        .await
        .map_err(|e| anyhow!("mouse_drag spawn error: {}", e))??;

        Ok(Some(
            json!({"status": "ok", "from": {"x": x1, "y": y1}, "to": {"x": x2, "y": y2}}),
        ))
    }
}

// ---------------------------------------------------------------------------
// screen_size() → {width, height}
// ---------------------------------------------------------------------------

pub struct ScreenSize;
#[async_trait]
impl Tool for ScreenSize {
    fn name(&self) -> &str {
        "screen_size"
    }
    async fn execute(
        &self,
        _params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let enigo = new_enigo()?;
        let (w, h) = enigo.main_display().map_err(wrap_enigo_error)?;
        Ok(Some(json!({"width": w, "height": h})))
    }
}

// ---------------------------------------------------------------------------
// screenshot(path="out.png", x=0, y=0, w=0, h=0)
// ---------------------------------------------------------------------------

pub struct Screenshot;
#[async_trait]
impl Tool for Screenshot {
    fn name(&self) -> &str {
        "screenshot"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        _context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let path = params
            .get("path")
            .ok_or_else(|| anyhow!("screenshot() requires 'path' parameter"))?;

        let monitors =
            xcap::Monitor::all().map_err(|e| anyhow!("Failed to enumerate monitors: {}", e))?;
        let monitor = monitors
            .first()
            .ok_or_else(|| anyhow!("No monitor found"))?;

        let image = monitor
            .capture_image()
            .map_err(|e| anyhow!("Screenshot capture failed: {}", e))?;

        // Optional region crop
        let crop_x: u32 = params.get("x").and_then(|s| s.parse().ok()).unwrap_or(0);
        let crop_y: u32 = params.get("y").and_then(|s| s.parse().ok()).unwrap_or(0);
        let crop_w: u32 = params.get("w").and_then(|s| s.parse().ok()).unwrap_or(0);
        let crop_h: u32 = params.get("h").and_then(|s| s.parse().ok()).unwrap_or(0);

        let final_image = if crop_w > 0 && crop_h > 0 {
            xcap::image::DynamicImage::ImageRgba8(image)
                .crop_imm(crop_x, crop_y, crop_w, crop_h)
                .to_rgba8()
        } else {
            image
        };

        // Ensure parent directory exists
        if let Some(parent) = std::path::Path::new(path).parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }

        final_image
            .save(path)
            .map_err(|e| anyhow!("Failed to save screenshot: {}", e))?;

        let w = final_image.width();
        let h = final_image.height();
        info!("📸 Screenshot saved: {} ({}x{})", path, w, h);

        Ok(Some(
            json!({"status": "ok", "path": path, "width": w, "height": h}),
        ))
    }
}

// ---------------------------------------------------------------------------
// key_listen(on_press=[handler], keys=[], timeout_ms=0)
// ---------------------------------------------------------------------------

pub struct KeyListen {
    builtin_registry: Option<std::sync::Weak<super::BuiltinRegistry>>,
}

impl Default for KeyListen {
    fn default() -> Self {
        Self::new()
    }
}

impl KeyListen {
    pub fn new() -> Self {
        Self {
            builtin_registry: None,
        }
    }

    pub fn set_registry(&mut self, registry: std::sync::Weak<super::BuiltinRegistry>) {
        self.builtin_registry = Some(registry);
    }
}

/// Convert rdev::Key to a human-readable name
fn rdev_key_name(key: &rdev::Key) -> String {
    match key {
        rdev::Key::Alt => "alt".into(),
        rdev::Key::AltGr => "altgr".into(),
        rdev::Key::Backspace => "backspace".into(),
        rdev::Key::CapsLock => "capslock".into(),
        rdev::Key::ControlLeft => "ctrl_left".into(),
        rdev::Key::ControlRight => "ctrl_right".into(),
        rdev::Key::Delete => "delete".into(),
        rdev::Key::DownArrow => "down".into(),
        rdev::Key::End => "end".into(),
        rdev::Key::Escape => "escape".into(),
        rdev::Key::F1 => "f1".into(),
        rdev::Key::F2 => "f2".into(),
        rdev::Key::F3 => "f3".into(),
        rdev::Key::F4 => "f4".into(),
        rdev::Key::F5 => "f5".into(),
        rdev::Key::F6 => "f6".into(),
        rdev::Key::F7 => "f7".into(),
        rdev::Key::F8 => "f8".into(),
        rdev::Key::F9 => "f9".into(),
        rdev::Key::F10 => "f10".into(),
        rdev::Key::F11 => "f11".into(),
        rdev::Key::F12 => "f12".into(),
        rdev::Key::Home => "home".into(),
        rdev::Key::LeftArrow => "left".into(),
        rdev::Key::MetaLeft => "meta_left".into(),
        rdev::Key::MetaRight => "meta_right".into(),
        rdev::Key::PageDown => "pagedown".into(),
        rdev::Key::PageUp => "pageup".into(),
        rdev::Key::Return => "enter".into(),
        rdev::Key::RightArrow => "right".into(),
        rdev::Key::ShiftLeft => "shift_left".into(),
        rdev::Key::ShiftRight => "shift_right".into(),
        rdev::Key::Space => "space".into(),
        rdev::Key::Tab => "tab".into(),
        rdev::Key::UpArrow => "up".into(),
        rdev::Key::KeyA => "a".into(),
        rdev::Key::KeyB => "b".into(),
        rdev::Key::KeyC => "c".into(),
        rdev::Key::KeyD => "d".into(),
        rdev::Key::KeyE => "e".into(),
        rdev::Key::KeyF => "f".into(),
        rdev::Key::KeyG => "g".into(),
        rdev::Key::KeyH => "h".into(),
        rdev::Key::KeyI => "i".into(),
        rdev::Key::KeyJ => "j".into(),
        rdev::Key::KeyK => "k".into(),
        rdev::Key::KeyL => "l".into(),
        rdev::Key::KeyM => "m".into(),
        rdev::Key::KeyN => "n".into(),
        rdev::Key::KeyO => "o".into(),
        rdev::Key::KeyP => "p".into(),
        rdev::Key::KeyQ => "q".into(),
        rdev::Key::KeyR => "r".into(),
        rdev::Key::KeyS => "s".into(),
        rdev::Key::KeyT => "t".into(),
        rdev::Key::KeyU => "u".into(),
        rdev::Key::KeyV => "v".into(),
        rdev::Key::KeyW => "w".into(),
        rdev::Key::KeyX => "x".into(),
        rdev::Key::KeyY => "y".into(),
        rdev::Key::KeyZ => "z".into(),
        rdev::Key::Num0 => "0".into(),
        rdev::Key::Num1 => "1".into(),
        rdev::Key::Num2 => "2".into(),
        rdev::Key::Num3 => "3".into(),
        rdev::Key::Num4 => "4".into(),
        rdev::Key::Num5 => "5".into(),
        rdev::Key::Num6 => "6".into(),
        rdev::Key::Num7 => "7".into(),
        rdev::Key::Num8 => "8".into(),
        rdev::Key::Num9 => "9".into(),
        _ => format!("{:?}", key).to_lowercase(),
    }
}

#[async_trait]
impl Tool for KeyListen {
    fn name(&self) -> &str {
        "key_listen"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let on_press_raw = params
            .get("on_press")
            .ok_or_else(|| anyhow!("key_listen() requires 'on_press' parameter"))?;
        let handler_name = on_press_raw
            .trim()
            .trim_start_matches('[')
            .trim_end_matches(']')
            .to_string();

        let timeout_ms: u64 = params
            .get("timeout_ms")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        // Parse optional key filter
        let key_filter: Vec<String> = params
            .get("keys")
            .and_then(|s| serde_json::from_str(s).ok())
            .unwrap_or_default();

        let registry = self
            .builtin_registry
            .as_ref()
            .and_then(|w| w.upgrade())
            .ok_or_else(|| anyhow!("key_listen(): BuiltinRegistry not available"))?;
        let executor = registry
            .get_executor()
            .ok_or_else(|| anyhow!("key_listen(): WorkflowExecutor not available"))?;
        let workflow = context
            .get_root_workflow()
            .ok_or_else(|| anyhow!("key_listen(): no root workflow found"))?;

        info!(
            "🎹 key_listen: on_press=[{}], filter={:?}, timeout={}ms",
            handler_name,
            key_filter,
            if timeout_ms == 0 {
                "∞".to_string()
            } else {
                timeout_ms.to_string()
            }
        );

        let (tx, mut rx) = tokio::sync::mpsc::channel::<(String, Vec<String>)>(32);

        // Track modifier state for the filter
        let key_filter_clone = key_filter.clone();
        tokio::task::spawn_blocking(move || {
            let modifiers = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
            let mods = modifiers.clone();
            let tx = tx;
            let _ = rdev::listen(move |event: rdev::Event| {
                match event.event_type {
                    rdev::EventType::KeyPress(key) => {
                        let name = rdev_key_name(&key);
                        // Normalize for modifier tracking (strip _left/_right)
                        let base = name
                            .strip_suffix("_left")
                            .or_else(|| name.strip_suffix("_right"))
                            .unwrap_or(&name)
                            .to_string();
                        let is_modifier =
                            matches!(base.as_str(), "shift" | "ctrl" | "alt" | "altgr" | "meta");

                        if is_modifier {
                            let mut m = mods.lock().unwrap();
                            if !m.contains(&base) {
                                m.push(base.clone());
                            }
                        }

                        // Apply key filter if set
                        let pass = key_filter_clone.is_empty()
                            || key_filter_clone.iter().any(|f| f.to_lowercase() == name);

                        if pass {
                            let current_mods = mods.lock().unwrap().clone();
                            let _ = tx.blocking_send((name, current_mods));
                        }
                    }
                    rdev::EventType::KeyRelease(key) => {
                        let name = rdev_key_name(&key);
                        let base = name
                            .strip_suffix("_left")
                            .or_else(|| name.strip_suffix("_right"))
                            .unwrap_or(&name)
                            .to_string();
                        let mut m = mods.lock().unwrap();
                        m.retain(|k| k != &base);
                    }
                    _ => {}
                }
            });
        });

        // Receive events and dispatch to handler
        let recv_loop = async {
            while let Some((key, modifiers)) = rx.recv().await {
                let mut args: HashMap<String, Value> = HashMap::new();
                args.insert("key".to_string(), json!(key));
                args.insert("modifiers".to_string(), json!(modifiers));

                if workflow.functions.contains_key(&handler_name) {
                    let _ = executor
                        .clone()
                        .execute_function(handler_name.clone(), args, workflow.clone(), context)
                        .await;
                }
            }
            Ok::<_, anyhow::Error>(())
        };

        if timeout_ms > 0 {
            let _ = tokio::time::timeout(tokio::time::Duration::from_millis(timeout_ms), recv_loop)
                .await;
        } else {
            recv_loop.await?;
        }

        Ok(Some(json!({"status": "stopped"})))
    }
}

// ---------------------------------------------------------------------------
// mouse_listen(on_click=[handler], on_move=[handler], timeout_ms=0)
// ---------------------------------------------------------------------------

pub struct MouseListen {
    builtin_registry: Option<std::sync::Weak<super::BuiltinRegistry>>,
}

impl Default for MouseListen {
    fn default() -> Self {
        Self::new()
    }
}

impl MouseListen {
    pub fn new() -> Self {
        Self {
            builtin_registry: None,
        }
    }

    pub fn set_registry(&mut self, registry: std::sync::Weak<super::BuiltinRegistry>) {
        self.builtin_registry = Some(registry);
    }
}

/// Mouse event info sent through channel
enum MouseEvent {
    Click { button: String, x: f64, y: f64 },
    Move { x: f64, y: f64 },
}

#[async_trait]
impl Tool for MouseListen {
    fn name(&self) -> &str {
        "mouse_listen"
    }
    async fn execute(
        &self,
        params: &HashMap<String, String>,
        context: &WorkflowContext,
    ) -> Result<Option<Value>> {
        let on_click_raw = params.get("on_click");
        let on_move_raw = params.get("on_move");

        if on_click_raw.is_none() && on_move_raw.is_none() {
            return Err(anyhow!(
                "mouse_listen() requires at least 'on_click' or 'on_move' parameter"
            ));
        }

        let click_handler = on_click_raw.map(|s| {
            s.trim()
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_string()
        });
        let move_handler = on_move_raw.filter(|s| !s.is_empty()).map(|s| {
            s.trim()
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_string()
        });

        let timeout_ms: u64 = params
            .get("timeout_ms")
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        let registry = self
            .builtin_registry
            .as_ref()
            .and_then(|w| w.upgrade())
            .ok_or_else(|| anyhow!("mouse_listen(): BuiltinRegistry not available"))?;
        let executor = registry
            .get_executor()
            .ok_or_else(|| anyhow!("mouse_listen(): WorkflowExecutor not available"))?;
        let workflow = context
            .get_root_workflow()
            .ok_or_else(|| anyhow!("mouse_listen(): no root workflow found"))?;

        info!(
            "🖱️ mouse_listen: on_click={:?}, on_move={:?}, timeout={}ms",
            click_handler,
            move_handler,
            if timeout_ms == 0 {
                "∞".to_string()
            } else {
                timeout_ms.to_string()
            }
        );

        let (tx, mut rx) = tokio::sync::mpsc::channel::<MouseEvent>(64);

        let has_click = click_handler.is_some();
        let has_move = move_handler.is_some();
        tokio::task::spawn_blocking(move || {
            let _ = rdev::listen(move |event: rdev::Event| match event.event_type {
                rdev::EventType::ButtonPress(btn) if has_click => {
                    let button = match btn {
                        rdev::Button::Left => "left",
                        rdev::Button::Right => "right",
                        rdev::Button::Middle => "middle",
                        _ => "unknown",
                    };
                    // rdev doesn't provide coordinates in ButtonPress on all platforms,
                    // so we track last known position separately if needed
                    let _ = tx.blocking_send(MouseEvent::Click {
                        button: button.to_string(),
                        x: 0.0,
                        y: 0.0,
                    });
                }
                rdev::EventType::MouseMove { x, y } if has_move => {
                    let _ = tx.blocking_send(MouseEvent::Move { x, y });
                }
                _ => {}
            });
        });

        let recv_loop = async {
            while let Some(event) = rx.recv().await {
                match event {
                    MouseEvent::Click { button, x, y } => {
                        if let Some(ref handler) = click_handler {
                            let mut args: HashMap<String, Value> = HashMap::new();
                            args.insert("button".to_string(), json!(button));
                            args.insert("x".to_string(), json!(x));
                            args.insert("y".to_string(), json!(y));
                            if workflow.functions.contains_key(handler) {
                                let _ = executor
                                    .clone()
                                    .execute_function(
                                        handler.clone(),
                                        args,
                                        workflow.clone(),
                                        context,
                                    )
                                    .await;
                            }
                        }
                    }
                    MouseEvent::Move { x, y } => {
                        if let Some(ref handler) = move_handler {
                            let mut args: HashMap<String, Value> = HashMap::new();
                            args.insert("x".to_string(), json!(x));
                            args.insert("y".to_string(), json!(y));
                            if workflow.functions.contains_key(handler) {
                                let _ = executor
                                    .clone()
                                    .execute_function(
                                        handler.clone(),
                                        args,
                                        workflow.clone(),
                                        context,
                                    )
                                    .await;
                            }
                        }
                    }
                }
            }
            Ok::<_, anyhow::Error>(())
        };

        if timeout_ms > 0 {
            let _ = tokio::time::timeout(tokio::time::Duration::from_millis(timeout_ms), recv_loop)
                .await;
        } else {
            recv_loop.await?;
        }

        Ok(Some(json!({"status": "stopped"})))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_enigo_key_letters() {
        assert!(parse_enigo_key("a").is_ok());
        assert!(parse_enigo_key("z").is_ok());
        assert!(parse_enigo_key("A").is_ok()); // case insensitive for special names
    }

    #[test]
    fn test_parse_enigo_key_specials() {
        assert!(parse_enigo_key("enter").is_ok());
        assert!(parse_enigo_key("return").is_ok());
        assert!(parse_enigo_key("tab").is_ok());
        assert!(parse_enigo_key("space").is_ok());
        assert!(parse_enigo_key("backspace").is_ok());
        assert!(parse_enigo_key("escape").is_ok());
        assert!(parse_enigo_key("esc").is_ok());
    }

    #[test]
    fn test_parse_enigo_key_modifiers() {
        assert!(parse_enigo_key("shift").is_ok());
        assert!(parse_enigo_key("ctrl").is_ok());
        assert!(parse_enigo_key("control").is_ok());
        assert!(parse_enigo_key("alt").is_ok());
        assert!(parse_enigo_key("option").is_ok());
        assert!(parse_enigo_key("meta").is_ok());
        assert!(parse_enigo_key("cmd").is_ok());
        assert!(parse_enigo_key("command").is_ok());
    }

    #[test]
    fn test_parse_enigo_key_function_keys() {
        for i in 1..=20 {
            assert!(parse_enigo_key(&format!("f{}", i)).is_ok());
        }
    }

    #[test]
    fn test_parse_enigo_key_unknown() {
        assert!(parse_enigo_key("nonexistent_key").is_err());
    }

    #[test]
    fn test_parse_button() {
        assert!(matches!(parse_button("left"), Button::Left));
        assert!(matches!(parse_button("right"), Button::Right));
        assert!(matches!(parse_button("middle"), Button::Middle));
        assert!(matches!(parse_button("unknown"), Button::Left)); // default
    }

    #[test]
    fn test_rdev_key_name() {
        assert_eq!(rdev_key_name(&rdev::Key::KeyA), "a");
        assert_eq!(rdev_key_name(&rdev::Key::Return), "enter");
        assert_eq!(rdev_key_name(&rdev::Key::MetaLeft), "meta_left");
        assert_eq!(rdev_key_name(&rdev::Key::MetaRight), "meta_right");
        assert_eq!(rdev_key_name(&rdev::Key::ShiftLeft), "shift_left");
        assert_eq!(rdev_key_name(&rdev::Key::ControlLeft), "ctrl_left");
    }
}
