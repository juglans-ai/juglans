use anyhow::Result;
use crossterm::event::{self, Event, KeyEvent, MouseButton, MouseEventKind};
use std::time::Duration;
use tokio::sync::mpsc;

#[derive(Debug)]
pub enum AppEvent {
    Key(KeyEvent),
    MouseScrollUp { x: u16, y: u16 },
    MouseScrollDown { x: u16, y: u16 },
    MouseClick { x: u16, y: u16 },
    MouseDrag { x: u16, y: u16 },
    MouseUp { x: u16, y: u16 },
    Resize((), ()),
    Tick,
}

pub struct EventHandler {
    rx: mpsc::UnboundedReceiver<AppEvent>,
}

impl EventHandler {
    pub fn new(tick_rate: Duration) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();

        tokio::spawn(async move {
            loop {
                if event::poll(tick_rate).unwrap_or(false) {
                    match event::read() {
                        Ok(Event::Key(key)) => {
                            if tx.send(AppEvent::Key(key)).is_err() {
                                return;
                            }
                        }
                        Ok(Event::Mouse(mouse)) => match mouse.kind {
                            MouseEventKind::ScrollUp => {
                                if tx
                                    .send(AppEvent::MouseScrollUp {
                                        x: mouse.column,
                                        y: mouse.row,
                                    })
                                    .is_err()
                                {
                                    return;
                                }
                            }
                            MouseEventKind::ScrollDown => {
                                if tx
                                    .send(AppEvent::MouseScrollDown {
                                        x: mouse.column,
                                        y: mouse.row,
                                    })
                                    .is_err()
                                {
                                    return;
                                }
                            }
                            MouseEventKind::Down(MouseButton::Left) => {
                                if tx
                                    .send(AppEvent::MouseClick {
                                        x: mouse.column,
                                        y: mouse.row,
                                    })
                                    .is_err()
                                {
                                    return;
                                }
                            }
                            MouseEventKind::Drag(MouseButton::Left) => {
                                if tx
                                    .send(AppEvent::MouseDrag {
                                        x: mouse.column,
                                        y: mouse.row,
                                    })
                                    .is_err()
                                {
                                    return;
                                }
                            }
                            MouseEventKind::Up(MouseButton::Left) => {
                                if tx
                                    .send(AppEvent::MouseUp {
                                        x: mouse.column,
                                        y: mouse.row,
                                    })
                                    .is_err()
                                {
                                    return;
                                }
                            }
                            _ => {}
                        },
                        Ok(Event::Resize(_w, _h)) => {
                            if tx.send(AppEvent::Resize((), ())).is_err() {
                                return;
                            }
                        }
                        _ => {}
                    }
                } else if tx.send(AppEvent::Tick).is_err() {
                    return;
                }
            }
        });

        Self { rx }
    }

    pub async fn next(&mut self) -> Result<AppEvent> {
        self.rx
            .recv()
            .await
            .ok_or_else(|| anyhow::anyhow!("Event channel closed"))
    }
}
