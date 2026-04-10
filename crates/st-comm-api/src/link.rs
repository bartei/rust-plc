//! Communication link trait — physical transport layer.
//!
//! A link represents a communication channel: TCP socket, serial port,
//! or simulated in-memory connection. Multiple devices can share a link
//! (e.g., multiple Modbus slaves on one RS-485 bus).

use crate::error::{CommError, LinkDiagnostics};

/// Physical transport layer for communication.
///
/// Implementations: TCP socket, serial port (RS-485/RS-232), simulated (in-memory).
/// A link is shared by multiple devices via coordinated access (mutex/queue).
pub trait CommLink: Send + Sync {
    /// Human-readable link name (from YAML config).
    fn name(&self) -> &str;

    /// Link type identifier: "tcp", "serial", "simulated", etc.
    fn link_type(&self) -> &str;

    /// Open the physical channel with the configured settings.
    fn open(&mut self) -> Result<(), CommError>;

    /// Close the physical channel.
    fn close(&mut self) -> Result<(), CommError>;

    /// Whether the link is currently open and operational.
    fn is_open(&self) -> bool;

    /// Send raw bytes over the link.
    fn send(&mut self, data: &[u8]) -> Result<(), CommError>;

    /// Receive raw bytes from the link.
    /// Returns the number of bytes actually received.
    fn receive(&mut self, buffer: &mut [u8], timeout_ms: u32) -> Result<usize, CommError>;

    /// Current diagnostics for this link.
    fn diagnostics(&self) -> LinkDiagnostics;
}
