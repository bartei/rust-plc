//! TCP transport layer for Modbus TCP.
//!
//! Manages a TCP socket connection to a remote Modbus device. Provides
//! send/receive primitives with automatic reconnection on failure.

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::time::Duration;

/// TCP connection configuration.
#[derive(Debug, Clone)]
pub struct TcpConfig {
    pub host: String,
    pub port: u16,
    /// Read/write timeout (default 500ms).
    pub timeout: Duration,
    /// TCP connect timeout (default 2s).
    pub connect_timeout: Duration,
}

impl Default for TcpConfig {
    fn default() -> Self {
        Self {
            host: String::new(),
            port: 502,
            timeout: Duration::from_millis(500),
            connect_timeout: Duration::from_secs(2),
        }
    }
}

/// Low-level TCP transport for Modbus TCP communication.
///
/// Unlike serial transport (shared among bus devices), each TCP transport
/// is dedicated to a single remote endpoint. No bus coordination needed.
pub struct TcpTransport {
    stream: Option<TcpStream>,
    config: TcpConfig,
}

impl TcpTransport {
    /// Create a new transport (not yet connected).
    pub fn new(config: TcpConfig) -> Self {
        Self {
            stream: None,
            config,
        }
    }

    /// Connect to the remote Modbus device.
    pub fn connect(&mut self) -> Result<(), String> {
        if self.stream.is_some() {
            return Ok(());
        }

        let addr_str = format!("{}:{}", self.config.host, self.config.port);
        let addr = addr_str
            .to_socket_addrs()
            .map_err(|e| format!("Cannot resolve {addr_str}: {e}"))?
            .next()
            .ok_or_else(|| format!("No address found for {addr_str}"))?;

        tracing::info!("Connecting to Modbus TCP device at {addr_str}");

        let stream = TcpStream::connect_timeout(&addr, self.config.connect_timeout)
            .map_err(|e| format!("Cannot connect to {addr_str}: {e}"))?;

        stream
            .set_read_timeout(Some(self.config.timeout))
            .map_err(|e| format!("Set read timeout: {e}"))?;
        stream
            .set_write_timeout(Some(self.config.timeout))
            .map_err(|e| format!("Set write timeout: {e}"))?;
        stream
            .set_nodelay(true)
            .map_err(|e| format!("Set TCP_NODELAY: {e}"))?;

        tracing::info!("Connected to Modbus TCP device at {addr_str}");
        self.stream = Some(stream);
        Ok(())
    }

    /// Disconnect from the remote device.
    pub fn disconnect(&mut self) {
        if self.stream.is_some() {
            tracing::info!(
                "Disconnecting from {}:{}",
                self.config.host,
                self.config.port
            );
            self.stream = None;
        }
    }

    /// Check if connected.
    pub fn is_connected(&self) -> bool {
        self.stream.is_some()
    }

    /// Disconnect and reconnect.
    pub fn reconnect(&mut self) -> Result<(), String> {
        self.disconnect();
        self.connect()
    }

    /// Send bytes to the remote device.
    pub fn send(&mut self, data: &[u8]) -> Result<(), String> {
        let stream = self
            .stream
            .as_mut()
            .ok_or_else(|| "Not connected".to_string())?;
        stream
            .write_all(data)
            .map_err(|e| format!("TCP write error: {e}"))?;
        stream
            .flush()
            .map_err(|e| format!("TCP flush error: {e}"))?;
        Ok(())
    }

    /// Receive exactly `len` bytes from the remote device.
    ///
    /// Modbus TCP uses MBAP headers with a length field, so we always
    /// know exactly how many bytes to expect.
    pub fn receive_exact(&mut self, buf: &mut [u8], len: usize) -> Result<(), String> {
        if len > buf.len() {
            return Err(format!(
                "Buffer too small: need {len}, have {}",
                buf.len()
            ));
        }
        let stream = self
            .stream
            .as_mut()
            .ok_or_else(|| "Not connected".to_string())?;
        stream
            .read_exact(&mut buf[..len])
            .map_err(|e| format!("TCP read error: {e}"))
    }

    /// Send a request and receive a complete MBAP response.
    ///
    /// Reads the 7-byte MBAP header first to determine response length,
    /// then reads the remaining bytes. Returns the total response length.
    /// On connection failure, attempts one reconnect before returning error.
    pub fn transaction(
        &mut self,
        request: &[u8],
        response_buf: &mut [u8],
    ) -> Result<usize, String> {
        match self.transaction_inner(request, response_buf) {
            Ok(n) => Ok(n),
            Err(e) => {
                tracing::debug!("Modbus TCP transaction failed, reconnecting: {e}");
                self.reconnect()?;
                self.transaction_inner(request, response_buf)
            }
        }
    }

    fn transaction_inner(
        &mut self,
        request: &[u8],
        response_buf: &mut [u8],
    ) -> Result<usize, String> {
        self.send(request)?;

        // Read 7-byte MBAP header
        if response_buf.len() < 7 {
            return Err("Response buffer too small for MBAP header".into());
        }
        self.receive_exact(response_buf, 7)?;

        // Extract length from MBAP header bytes [4..6] (big-endian)
        let length = ((response_buf[4] as usize) << 8) | (response_buf[5] as usize);
        if length < 1 {
            return Err("Invalid MBAP length field".into());
        }

        // length includes unit_id (1 byte) which is already in header byte [6]
        // Remaining data after the 7-byte header = length - 1
        let remaining = length - 1;
        let total = 7 + remaining;
        if total > response_buf.len() {
            return Err(format!(
                "Response too large: {total} bytes, buffer is {}",
                response_buf.len()
            ));
        }

        if remaining > 0 {
            self.receive_exact(&mut response_buf[7..], remaining)?;
        }

        Ok(total)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_defaults() {
        let cfg = TcpConfig::default();
        assert_eq!(cfg.port, 502);
        assert_eq!(cfg.timeout, Duration::from_millis(500));
        assert_eq!(cfg.connect_timeout, Duration::from_secs(2));
    }

    #[test]
    fn transport_starts_disconnected() {
        let t = TcpTransport::new(TcpConfig::default());
        assert!(!t.is_connected());
    }

    #[test]
    fn send_fails_when_disconnected() {
        let mut t = TcpTransport::new(TcpConfig::default());
        let result = t.send(&[0x01, 0x02]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Not connected"));
    }

    #[test]
    fn receive_fails_when_disconnected() {
        let mut t = TcpTransport::new(TcpConfig::default());
        let mut buf = [0u8; 64];
        let result = t.receive_exact(&mut buf, 7);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Not connected"));
    }

    #[test]
    fn connect_fails_with_invalid_host() {
        let mut t = TcpTransport::new(TcpConfig {
            host: "192.0.2.1".into(), // TEST-NET, should timeout
            port: 502,
            timeout: Duration::from_millis(100),
            connect_timeout: Duration::from_millis(100),
        });
        let result = t.connect();
        assert!(result.is_err());
        assert!(!t.is_connected());
    }
}
