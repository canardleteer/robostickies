//! Git-friendly repository notes with shared CLI and MCP operations.
//!
//! Markdown is authoritative. The local redb index can always be regenerated.
//! Private tags and notes live in a separate, ignored directory namespace.

#![forbid(unsafe_code)]

pub mod cli;
pub mod config;
mod document;
mod index;
pub mod mcp;
pub mod model;
pub mod store;
pub mod version;

pub use model::{Error, Result};
pub use store::Store;
