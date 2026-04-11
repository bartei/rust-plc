//! PLC runtime agent for remote deployment, lifecycle management, and monitoring.
//!
//! The `st-target-agent` is a standalone daemon that runs on target devices
//! (Linux/Windows embedded PCs). It manages the PLC runtime lifecycle and
//! exposes an HTTP REST API for remote deployment, control, and monitoring.

pub mod api;
pub mod auth;
pub mod config;
pub mod dap_proxy;
pub mod error;
pub mod program_store;
pub mod runtime_manager;
pub mod server;
pub mod watchdog;
