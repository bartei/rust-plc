//! WebSocket-based online monitoring server for the PLC runtime.
//!
//! Streams live variable values to connected clients for real-time
//! dashboards and trend recording. Used by both st-target-agent (remote
//! targets) and st-dap (local debug sessions).

pub mod protocol;
pub mod server;

pub use protocol::*;
pub use server::{MonitorHandle, MonitorState, run_monitor_server};
