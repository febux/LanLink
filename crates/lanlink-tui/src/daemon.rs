//! Daemon control/status client (Unix domain socket or TCP loopback,
//! newline-delimited JSON).
//!
//! The TUI delegates all socket I/O to the shared `lanlink-client` crate.

pub use lanlink_client::{discover_socket, fetch_snapshot, send_command};
