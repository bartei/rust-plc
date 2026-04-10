//! Simulated link — in-memory, no network I/O.

use st_comm_api::{CommError, CommLink, LinkDiagnostics};

/// A simulated communication link. No physical transport — everything is in-memory.
/// Exists to satisfy the CommLink trait requirement. Multiple simulated devices
/// can share a single SimulatedLink.
pub struct SimulatedLink {
    name: String,
    is_open: bool,
}

impl SimulatedLink {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            is_open: false,
        }
    }
}

impl CommLink for SimulatedLink {
    fn name(&self) -> &str {
        &self.name
    }

    fn link_type(&self) -> &str {
        "simulated"
    }

    fn open(&mut self) -> Result<(), CommError> {
        self.is_open = true;
        Ok(())
    }

    fn close(&mut self) -> Result<(), CommError> {
        self.is_open = false;
        Ok(())
    }

    fn is_open(&self) -> bool {
        self.is_open
    }

    fn send(&mut self, _data: &[u8]) -> Result<(), CommError> {
        // No-op for simulated link
        Ok(())
    }

    fn receive(&mut self, _buffer: &mut [u8], _timeout_ms: u32) -> Result<usize, CommError> {
        // No-op for simulated link
        Ok(0)
    }

    fn diagnostics(&self) -> LinkDiagnostics {
        LinkDiagnostics {
            is_open: self.is_open,
            ..Default::default()
        }
    }
}
