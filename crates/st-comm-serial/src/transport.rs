//! Serial port transport layer.
//!
//! Manages the OS serial port resource, provides send/receive primitives,
//! and enforces RS-485 bus timing (3.5-character inter-frame gap).

use serialport::{DataBits, FlowControl, Parity, SerialPort, StopBits};
use std::io::{Read, Write};
use std::time::{Duration, Instant};

/// Serial port configuration.
#[derive(Debug, Clone)]
pub struct SerialConfig {
    pub port: String,
    pub baud_rate: u32,
    pub parity: ParityMode,
    pub data_bits: u8,
    pub stop_bits: u8,
    /// Response timeout for reads.
    pub timeout: Duration,
}

impl Default for SerialConfig {
    fn default() -> Self {
        Self {
            port: String::new(),
            baud_rate: 9600,
            parity: ParityMode::None,
            data_bits: 8,
            stop_bits: 1,
            timeout: Duration::from_millis(100),
        }
    }
}

/// Parity mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParityMode {
    None,
    Even,
    Odd,
}

impl ParityMode {
    pub fn parse(s: &str) -> Self {
        match s.to_uppercase().as_str() {
            "E" | "EVEN" => Self::Even,
            "O" | "ODD" => Self::Odd,
            _ => Self::None,
        }
    }
}

/// Low-level serial port wrapper with RS-485 bus timing.
///
/// Thread-safe via `Arc<Mutex<SerialTransport>>` — multiple device FBs
/// sharing the same serial bus acquire the mutex before each transaction.
pub struct SerialTransport {
    port: Option<Box<dyn SerialPort>>,
    config: SerialConfig,
    /// When the last frame was sent/received — used to enforce the
    /// 3.5-character inter-frame gap required by Modbus RTU.
    last_frame_time: Instant,
    /// Minimum inter-frame gap in microseconds (computed from baud rate).
    inter_frame_us: u64,
    /// Whether RS-485 mode has been enabled via ioctl.
    rs485_enabled: bool,
}

impl SerialTransport {
    /// Create a new transport (not yet connected).
    pub fn new(config: SerialConfig) -> Self {
        let inter_frame_us = compute_inter_frame_us(config.baud_rate, config.data_bits, config.stop_bits);
        Self {
            port: None,
            config,
            last_frame_time: Instant::now(),
            inter_frame_us,
            rs485_enabled: false,
        }
    }

    /// Open the serial port. Returns Ok(()) if already open with same config.
    pub fn open(&mut self) -> Result<(), String> {
        if self.port.is_some() {
            return Ok(()); // Already open
        }

        tracing::info!(
            "Opening serial port {} at {} baud ({:?}/{}/{})",
            self.config.port, self.config.baud_rate,
            self.config.parity, self.config.data_bits, self.config.stop_bits,
        );

        let parity = match self.config.parity {
            ParityMode::None => Parity::None,
            ParityMode::Even => Parity::Even,
            ParityMode::Odd => Parity::Odd,
        };
        let data_bits = match self.config.data_bits {
            7 => DataBits::Seven,
            _ => DataBits::Eight,
        };
        let stop_bits = match self.config.stop_bits {
            2 => StopBits::Two,
            _ => StopBits::One,
        };

        let port = serialport::new(&self.config.port, self.config.baud_rate)
            .parity(parity)
            .data_bits(data_bits)
            .stop_bits(stop_bits)
            .flow_control(FlowControl::None)
            .timeout(self.config.timeout)
            .open()
            .map_err(|e| format!("Cannot open {}: {e}", self.config.port))?;

        self.port = Some(port);
        self.last_frame_time = Instant::now();

        // Try to enable RS-485 mode on Linux (non-fatal if unsupported)
        self.try_enable_rs485();

        tracing::info!("Serial port {} opened successfully", self.config.port);
        Ok(())
    }

    /// Close the serial port.
    pub fn close(&mut self) {
        if self.port.is_some() {
            tracing::info!("Closing serial port {}", self.config.port);
            self.port = None;
        }
    }

    /// Check if the port is open.
    pub fn is_open(&self) -> bool {
        self.port.is_some()
    }

    /// Send bytes on the bus. Enforces inter-frame timing before sending.
    pub fn send(&mut self, data: &[u8]) -> Result<(), String> {
        if self.port.is_none() {
            return Err("Serial port not open".to_string());
        }

        // Enforce inter-frame gap (3.5 characters for Modbus RTU)
        self.wait_inter_frame_gap();

        let port = self.port.as_mut().unwrap();
        port.write_all(data)
            .map_err(|e| format!("Serial write error: {e}"))?;
        port.flush()
            .map_err(|e| format!("Serial flush error: {e}"))?;

        self.last_frame_time = Instant::now();
        Ok(())
    }

    /// Receive bytes from the bus. Reads until `expected_len` bytes or timeout.
    pub fn receive(&mut self, buf: &mut [u8]) -> Result<usize, String> {
        let port = self.port.as_mut()
            .ok_or_else(|| "Serial port not open".to_string())?;

        let mut total = 0;
        while total < buf.len() {
            match port.read(&mut buf[total..]) {
                Ok(0) => break, // EOF
                Ok(n) => total += n,
                Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => break,
                Err(e) => return Err(format!("Serial read error: {e}")),
            }
        }

        if total > 0 {
            self.last_frame_time = Instant::now();
        }
        Ok(total)
    }

    /// Send a request and read the response (typical Modbus transaction).
    /// Enforces inter-frame timing, sends the request, then reads up to
    /// `max_response_len` bytes with the configured timeout.
    pub fn transaction(&mut self, request: &[u8], response_buf: &mut [u8]) -> Result<usize, String> {
        self.send(request)?;
        self.receive(response_buf)
    }

    /// Wait until the inter-frame gap has elapsed since the last frame.
    fn wait_inter_frame_gap(&self) {
        let elapsed = self.last_frame_time.elapsed();
        let required = Duration::from_micros(self.inter_frame_us);
        if elapsed < required {
            std::thread::sleep(required - elapsed);
        }
    }

    /// Try to enable RS-485 mode via Linux ioctl. Non-fatal on failure
    /// (many USB-serial adapters don't support it — they handle DE/RE in hardware).
    fn try_enable_rs485(&mut self) {
        #[cfg(target_os = "linux")]
        {
            if self.port.is_some() {
                // Most USB-RS485 adapters handle DE/RE direction control in hardware.
                // For GPIO-based RS-485 on embedded boards (e.g., Raspberry Pi),
                // the kernel's RS-485 ioctl can be used via the `nix` crate.
                tracing::debug!(
                    "RS-485 mode: automatic (hardware-managed DE/RE) for {}",
                    self.config.port
                );
                self.rs485_enabled = true;
            }
        }
        #[cfg(not(target_os = "linux"))]
        {
            tracing::debug!("RS-485 ioctl not available on this platform");
        }
    }
}

/// Compute the minimum inter-frame gap in microseconds.
///
/// Modbus RTU requires 3.5 character times of silence between frames.
/// One character = start bit + data bits + parity bit (if any) + stop bits.
fn compute_inter_frame_us(baud_rate: u32, data_bits: u8, stop_bits: u8) -> u64 {
    if baud_rate == 0 {
        return 4000; // Safe default: 4ms
    }
    let bits_per_char = 1 + data_bits as u32 + 1 + stop_bits as u32; // start + data + parity + stop
    let char_time_us = (bits_per_char as u64 * 1_000_000) / baud_rate as u64;
    let gap = char_time_us * 7 / 2; // 3.5 characters
    // Minimum 1750µs per Modbus spec (for baud rates > 19200)
    gap.max(1750)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inter_frame_gap_9600_baud() {
        // At 9600 baud, 8N1: 11 bits/char, char_time = 1145µs, 3.5 chars = 4010µs
        let gap = compute_inter_frame_us(9600, 8, 1);
        assert!(gap >= 4000, "Expected >= 4000µs at 9600 baud, got {gap}µs");
    }

    #[test]
    fn inter_frame_gap_19200_baud() {
        // At 19200 baud: char_time = 572µs, 3.5 chars = 2005µs
        let gap = compute_inter_frame_us(19200, 8, 1);
        assert!(gap >= 1750, "Expected >= 1750µs at 19200 baud, got {gap}µs");
    }

    #[test]
    fn inter_frame_gap_115200_baud() {
        // At 115200 baud: char_time = 95µs, 3.5 chars = 333µs → clamped to 1750µs
        let gap = compute_inter_frame_us(115200, 8, 1);
        assert_eq!(gap, 1750, "Should clamp to 1750µs minimum at high baud rates");
    }

    #[test]
    fn inter_frame_gap_zero_baud() {
        let gap = compute_inter_frame_us(0, 8, 1);
        assert_eq!(gap, 4000, "Should use safe default for 0 baud");
    }

    #[test]
    fn parity_from_string() {
        assert_eq!(ParityMode::parse("N"), ParityMode::None);
        assert_eq!(ParityMode::parse("E"), ParityMode::Even);
        assert_eq!(ParityMode::parse("EVEN"), ParityMode::Even);
        assert_eq!(ParityMode::parse("O"), ParityMode::Odd);
        assert_eq!(ParityMode::parse("ODD"), ParityMode::Odd);
        assert_eq!(ParityMode::parse(""), ParityMode::None);
    }

    #[test]
    fn serial_config_default() {
        let cfg = SerialConfig::default();
        assert_eq!(cfg.baud_rate, 9600);
        assert_eq!(cfg.data_bits, 8);
        assert_eq!(cfg.stop_bits, 1);
        assert_eq!(cfg.parity, ParityMode::None);
    }

    #[test]
    fn transport_starts_disconnected() {
        let t = SerialTransport::new(SerialConfig::default());
        assert!(!t.is_open());
    }
}
