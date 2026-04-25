//! Serial port transport layer.
//!
//! Manages the OS serial port resource, provides send/receive primitives,
//! and enforces RS-485 bus timing (3.5-character inter-frame gap).
//!
//! ## Receive primitives
//!
//! Three primitives are exposed, each with strict semantics so that
//! protocols can build on them without inheriting an inactivity-timeout
//! penalty:
//!
//! - [`SerialTransport::receive_some`] — single OS read; returns whatever
//!   bytes are immediately available (or `Ok(0)` if the OS read times out).
//!   Useful when a protocol wants to drive its own deadline loop.
//! - [`SerialTransport::receive_exact`] — read exactly `len` bytes within the
//!   configured per-transaction timeout, or fail.
//! - [`SerialTransport::transaction_framed`] — generic send-then-receive
//!   driven by a [`FrameParser`](crate::framing::FrameParser); returns as
//!   soon as the parser reports the frame complete, with no trailing
//!   timeout drain.

use crate::framing::{FrameParser, FrameStatus};
use serialport::{ClearBuffer, DataBits, FlowControl, Parity, SerialPort, StopBits};
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
    /// Cached current OS-level read timeout. Tracks `port.set_timeout` so
    /// per-transaction timeout adjustments are syscall-free when unchanged.
    current_read_timeout: Duration,
}

impl SerialTransport {
    /// Create a new transport (not yet connected).
    pub fn new(config: SerialConfig) -> Self {
        let inter_frame_us = compute_inter_frame_us(config.baud_rate, config.data_bits, config.stop_bits);
        let current_read_timeout = config.timeout;
        Self {
            port: None,
            config,
            last_frame_time: Instant::now(),
            inter_frame_us,
            rs485_enabled: false,
            current_read_timeout,
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
        self.current_read_timeout = self.config.timeout;
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

    /// Single-shot read from the OS serial buffer.
    ///
    /// Performs exactly one `read()` syscall. Returns the number of bytes
    /// read. Returns `Ok(0)` if the OS read timed out before any bytes were
    /// available — callers driving their own deadline can re-check it and
    /// loop, while callers that simply want "anything available right now"
    /// can treat 0 as "nothing to do".
    ///
    /// Does **not** loop until the buffer fills. Looping until full would
    /// always end with a timeout-fired read (since we have no protocol
    /// knowledge of where the frame boundary is) and add a full timeout's
    /// worth of dead time to every transaction.
    pub fn receive_some(&mut self, buf: &mut [u8]) -> Result<usize, String> {
        let port = self.port.as_mut()
            .ok_or_else(|| "Serial port not open".to_string())?;

        match port.read(buf) {
            Ok(n) => {
                if n > 0 {
                    self.last_frame_time = Instant::now();
                }
                Ok(n)
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => Ok(0),
            Err(e) => Err(format!("Serial read error: {e}")),
        }
    }

    /// Read exactly `len` bytes into the start of `buf` within the
    /// configured timeout.
    ///
    /// Useful for protocols (or sub-frames within a protocol) where the
    /// expected length is known ahead of time. Fails if the deadline
    /// elapses before `len` bytes have been received.
    pub fn receive_exact(&mut self, buf: &mut [u8], len: usize) -> Result<(), String> {
        if len > buf.len() {
            return Err(format!(
                "Buffer too small for receive_exact: need {len}, have {}",
                buf.len()
            ));
        }
        let deadline = Instant::now() + self.config.timeout;
        let mut total = 0;
        while total < len {
            if Instant::now() >= deadline {
                return Err(format!(
                    "Receive timeout: got {total}/{len} bytes within {:?}",
                    self.config.timeout
                ));
            }
            match self.receive_some(&mut buf[total..len])? {
                0 => continue, // OS read timed out with nothing available; check deadline
                n => total += n,
            }
        }
        Ok(())
    }

    /// Discard any bytes currently sitting in the OS input or output buffer.
    ///
    /// Called at the start of every framed transaction to prevent stale
    /// bytes from polluting the next transaction:
    ///
    /// - **input**: a previous slave's late response that arrived after the
    ///   master timed out.
    /// - **output**: bytes left over from a write that errored mid-flight
    ///   before reaching the wire — without flushing them, they would be
    ///   prepended to the next request.
    ///
    /// When the input buffer is non-empty, the bytes are read into a
    /// temporary buffer and hex-logged at debug level before the clear,
    /// so we can see exactly what we're throwing away. (This is the
    /// usual diagnostic for "the slave's response landed in the wrong
    /// transaction" or "an echoing RS-485 adapter is feeding us our own
    /// TX bytes" — both invisible without a log of the discarded bytes.)
    pub fn clear_input_buffer(&mut self) -> Result<(), String> {
        let port = self.port.as_mut()
            .ok_or_else(|| "Serial port not open".to_string())?;
        let pending = port.bytes_to_read().unwrap_or(0) as usize;
        if pending > 0 && tracing::enabled!(tracing::Level::DEBUG) {
            let mut peek = vec![0u8; pending];
            match port.read(&mut peek) {
                Ok(n) if n > 0 => tracing::debug!(
                    "clear_input_buffer discarding {n} byte(s): {}",
                    hex_dump(&peek[..n])
                ),
                _ => {}
            }
        }
        port.clear(ClearBuffer::All)
            .map_err(|e| format!("Serial clear-buffer error: {e}"))
    }

    /// Update the OS-level read timeout if it differs from the cached one.
    ///
    /// `port.set_timeout` is one syscall; the cache lets back-to-back
    /// transactions on the same device skip it.
    fn ensure_read_timeout(&mut self, timeout: Duration) -> Result<(), String> {
        if timeout == self.current_read_timeout {
            return Ok(());
        }
        let port = self.port.as_mut()
            .ok_or_else(|| "Serial port not open".to_string())?;
        port.set_timeout(timeout)
            .map_err(|e| format!("Serial set-timeout error: {e}"))?;
        self.current_read_timeout = timeout;
        Ok(())
    }

    /// Send a request and receive a response framed by `parser`.
    ///
    /// `timeout` is the maximum time allowed for the entire response to
    /// arrive after the request is on the wire. The OS-level read timeout
    /// is updated to match, so a short transaction timeout will not be
    /// stretched by a longer port-level timeout left over from earlier.
    ///
    /// `preamble` is the minimum bus-silence the caller wants since the
    /// last frame on the wire, before the request is transmitted. The
    /// protocol's mandatory 3.5-character inter-frame gap is always
    /// enforced; `preamble` lets a slow slave demand more (e.g. some
    /// cheap RS-485 modules need several ms of quiet before they will
    /// reliably parse the next request). Pass `Duration::ZERO` when
    /// the protocol minimum is enough.
    ///
    /// The parser is consulted after every successful read; once it reports
    /// [`FrameStatus::Complete`] the call returns immediately, without any
    /// inactivity-timeout drain. The parser is also consulted before the
    /// first read so that it can tell us how many bytes to wait for up
    /// front.
    ///
    /// The OS input buffer is cleared after the preamble wait and before
    /// the request is sent, so any late bytes left over from a previous
    /// transaction (or that arrived during the wait) cannot pollute this
    /// one.
    ///
    /// Returns the length of the parsed frame (`buf[..len]`).
    pub fn transaction_framed<P: FrameParser>(
        &mut self,
        request: &[u8],
        response_buf: &mut [u8],
        parser: &mut P,
        timeout: Duration,
        preamble: Duration,
    ) -> Result<usize, String> {
        self.ensure_read_timeout(timeout)?;

        // Honour the caller's minimum-silence requirement on top of the
        // mandatory inter-frame gap that `send()` already enforces.
        if !preamble.is_zero() {
            let elapsed = self.last_frame_time.elapsed();
            if elapsed < preamble {
                std::thread::sleep(preamble - elapsed);
            }
        }

        self.clear_input_buffer()?;
        let t_send_start = Instant::now();
        self.send(request)?;
        let t_send_done = Instant::now();
        let deadline = t_send_done + timeout;
        let mut total = 0;
        let mut empty_reads = 0u32;
        let mut data_reads = 0u32;

        loop {
            match parser.parse(&response_buf[..total]) {
                FrameStatus::Complete(n) => {
                    if n > total {
                        return Err(format!(
                            "FrameParser returned Complete({n}) but only {total} bytes were read"
                        ));
                    }
                    tracing::trace!(
                        "tx ok req={} resp={} send_us={} recv_us={} reads={}/{}",
                        hex_dump(request),
                        hex_dump(&response_buf[..n]),
                        (t_send_done - t_send_start).as_micros(),
                        t_send_done.elapsed().as_micros(),
                        data_reads,
                        empty_reads,
                    );
                    return Ok(n);
                }
                FrameStatus::Invalid(msg) => {
                    tracing::debug!(
                        "tx invalid req={} partial_resp={} reason={msg}",
                        hex_dump(request),
                        hex_dump(&response_buf[..total]),
                    );
                    return Err(msg);
                }
                FrameStatus::Need(min_total) => {
                    if min_total > response_buf.len() {
                        return Err(format!(
                            "Frame requires {min_total} bytes, response buffer is {} bytes",
                            response_buf.len()
                        ));
                    }
                    if min_total <= total {
                        return Err(format!(
                            "FrameParser asked for Need({min_total}) but already have {total} bytes \
                             — parser must request strictly more than current length"
                        ));
                    }
                    while total < min_total {
                        if Instant::now() >= deadline {
                            tracing::debug!(
                                "tx timeout req={} partial_resp={} need={min_total} got={total} \
                                 send_ms={:.2} recv_ms={:.2} reads={}/{} timeout={timeout:?}",
                                hex_dump(request),
                                hex_dump(&response_buf[..total]),
                                (t_send_done - t_send_start).as_secs_f64() * 1000.0,
                                t_send_done.elapsed().as_secs_f64() * 1000.0,
                                data_reads,
                                empty_reads,
                            );
                            return Err(format!(
                                "Receive timeout: got {total} of expected {min_total}+ bytes \
                                 within {timeout:?}"
                            ));
                        }
                        // Read into the rest of the buffer, not just up to min_total —
                        // this lets a single OS read pull more than the parser's current
                        // milestone when data is already queued, avoiding extra syscalls.
                        match self.receive_some(&mut response_buf[total..])? {
                            0 => empty_reads += 1,
                            n => {
                                total += n;
                                data_reads += 1;
                            }
                        }
                    }
                }
            }
        }
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

/// Format a byte slice as space-separated uppercase hex (`01 02 0A FF`).
/// Returns `<empty>` for an empty slice so log lines stay self-explanatory.
fn hex_dump(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return "<empty>".to_string();
    }
    let mut out = String::with_capacity(bytes.len() * 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(&format!("{b:02X}"));
    }
    out
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

    #[test]
    fn receive_some_fails_when_not_open() {
        let mut t = SerialTransport::new(SerialConfig::default());
        let mut buf = [0u8; 8];
        let err = t.receive_some(&mut buf).unwrap_err();
        assert!(err.contains("not open"));
    }

    #[test]
    fn receive_exact_rejects_oversized_len() {
        let mut t = SerialTransport::new(SerialConfig::default());
        let mut buf = [0u8; 4];
        let err = t.receive_exact(&mut buf, 8).unwrap_err();
        assert!(err.contains("Buffer too small"), "Got: {err}");
    }
}
