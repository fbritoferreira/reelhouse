//! Client logic shared by every Reelhouse app: API client, session and playback state.

pub mod client;
#[cfg(not(target_arch = "wasm32"))]
pub mod discover;
pub mod timefmt;

pub use client::{Client, ClientError};
