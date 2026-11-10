//! Client logic shared by every Reelhouse app: API client, session and playback state.

pub mod client;
pub mod timefmt;

pub use client::{Client, ClientError};
