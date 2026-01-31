// src/ui/mod.rs
//! Terminal UI utilities for enhanced user experience

pub mod render;
pub mod input;

pub use render::render_markdown;
pub use input::MultilineInput;
