//! Catsyphon Collector Client
//!
//! This module provides optional integration with Catsyphon servers,
//! enabling aiobscura to push events to a central analytics platform.
//!
//! ## Architecture
//!
//! The collector follows a "local-first" principle:
//! - Events are always stored in the local SQLite database first
//! - Publishing to Catsyphon happens asynchronously after successful DB insert
//! - Network failures never block local operation
//!
//! ## Usage
//!
//! Enable the collector in `~/.config/aiobscura/config.toml`:
//!
//! ```toml
//! [collector]
//! enabled = true
//! server_url = "https://catsyphon.example.com"
//! collector_id = "your-collector-id"
//! api_key = "cs_live_xxxxxxxxxxxx"
//! ```

mod client;
mod events;

pub use client::CollectorClient;
pub use events::CollectorEvent;
