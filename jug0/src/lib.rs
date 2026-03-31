// src/lib.rs
//
// jug0 as a library.
// - Default (no features): providers only (LLM, embedding, search)
// - "server" feature: also exposes entities and services

pub mod providers;

#[cfg(feature = "server")]
pub mod entities;
#[cfg(feature = "server")]
pub mod errors;
#[cfg(feature = "server")]
pub mod request;
#[cfg(feature = "server")]
pub mod response;
#[cfg(feature = "server")]
pub mod services;
