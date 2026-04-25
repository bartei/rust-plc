//! Modbus RTU client — sends requests and parses responses via SerialTransport.

use crate::frame::{self, FunctionCode};
use crate::frame_parser::RtuFrameParser;
use st_comm_serial::transport::SerialTransport;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Default transaction timeout when no per-device value is configured.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_millis(100);

/// Default preamble (additional bus-silence) before each transaction.
///
/// 5 ms is the tested minimum for typical multi-drop RTU buses with cheap
/// RS-485 modules: their UART/firmware needs several ms after their
/// previous response before they can parse a fresh request reliably (we
/// observed 12–20 % silent request-drops with only the protocol-mandatory
/// 3.5-char gap, ~0 % with 5 ms; 3 ms was insufficient on the test bench).
///
/// Set `preamble := T#0ms` per device to opt out when talking to a strict
/// spec-compliant slave that doesn't need the cushion.
pub const DEFAULT_PREAMBLE: Duration = Duration::from_millis(5);

/// Result of a Modbus transaction.
pub struct TransactionResult {
    /// Response data bytes (excluding header and CRC).
    pub data: Vec<u8>,
    /// Round-trip time in microseconds.
    pub rtt_us: u64,
}

/// Modbus RTU client operating over a shared serial transport.
pub struct RtuClient {
    transport: Arc<Mutex<SerialTransport>>,
    timeout: Duration,
    preamble: Duration,
}

impl RtuClient {
    /// Build a client with the default 100 ms transaction timeout and no
    /// extra preamble.
    pub fn new(transport: Arc<Mutex<SerialTransport>>) -> Self {
        Self::with_timing(transport, DEFAULT_TIMEOUT, DEFAULT_PREAMBLE)
    }

    /// Build a client with an explicit per-device transaction timeout
    /// (preamble defaults to zero).
    ///
    /// Devices that respond quickly can lower this value to recover faster
    /// from a missed response without sacrificing reliability.
    pub fn with_timeout(transport: Arc<Mutex<SerialTransport>>, timeout: Duration) -> Self {
        Self::with_timing(transport, timeout, DEFAULT_PREAMBLE)
    }

    /// Build a client with explicit timeout + preamble.
    ///
    /// `preamble` is the minimum bus-silence (in addition to the protocol-
    /// mandatory inter-frame gap) the slave needs before it will parse the
    /// next request. Useful for cheap RS-485 modules that drop frames sent
    /// too soon after the previous transaction.
    pub fn with_timing(
        transport: Arc<Mutex<SerialTransport>>,
        timeout: Duration,
        preamble: Duration,
    ) -> Self {
        Self { transport, timeout, preamble }
    }

    /// Read coils (FC01).
    pub fn read_coils(&self, slave_id: u8, start: u16, count: u16) -> Result<Vec<bool>, String> {
        let request = frame::build_read_request(slave_id, FunctionCode::ReadCoils, start, count);
        let response = self.transact(&request)?;
        let data = frame::parse_read_response(&response)?;
        Ok(frame::extract_coils(data, count as usize))
    }

    /// Read discrete inputs (FC02).
    pub fn read_discrete_inputs(&self, slave_id: u8, start: u16, count: u16) -> Result<Vec<bool>, String> {
        let request = frame::build_read_request(slave_id, FunctionCode::ReadDiscreteInputs, start, count);
        let response = self.transact(&request)?;
        let data = frame::parse_read_response(&response)?;
        Ok(frame::extract_coils(data, count as usize))
    }

    /// Read holding registers (FC03).
    pub fn read_holding_registers(&self, slave_id: u8, start: u16, count: u16) -> Result<Vec<u16>, String> {
        let request = frame::build_read_request(slave_id, FunctionCode::ReadHoldingRegisters, start, count);
        let response = self.transact(&request)?;
        let data = frame::parse_read_response(&response)?;
        Ok(frame::extract_registers(data))
    }

    /// Read input registers (FC04).
    pub fn read_input_registers(&self, slave_id: u8, start: u16, count: u16) -> Result<Vec<u16>, String> {
        let request = frame::build_read_request(slave_id, FunctionCode::ReadInputRegisters, start, count);
        let response = self.transact(&request)?;
        let data = frame::parse_read_response(&response)?;
        Ok(frame::extract_registers(data))
    }

    /// Write single coil (FC05).
    pub fn write_single_coil(&self, slave_id: u8, addr: u16, value: bool) -> Result<(), String> {
        let request = frame::build_write_single_coil(slave_id, addr, value);
        let response = self.transact(&request)?;
        frame::parse_write_response(&response)
    }

    /// Write single register (FC06).
    pub fn write_single_register(&self, slave_id: u8, addr: u16, value: u16) -> Result<(), String> {
        let request = frame::build_write_single_register(slave_id, addr, value);
        let response = self.transact(&request)?;
        frame::parse_write_response(&response)
    }

    /// Write multiple coils (FC0F).
    pub fn write_multiple_coils(&self, slave_id: u8, start: u16, coils: &[bool]) -> Result<(), String> {
        let request = frame::build_write_multiple_coils(slave_id, start, coils);
        let response = self.transact(&request)?;
        frame::parse_write_response(&response)
    }

    /// Write multiple registers (FC10).
    pub fn write_multiple_registers(&self, slave_id: u8, start: u16, values: &[u16]) -> Result<(), String> {
        let request = frame::build_write_multiple_registers(slave_id, start, values);
        let response = self.transact(&request)?;
        frame::parse_write_response(&response)
    }

    /// Low-level: send request, receive response, return raw frame.
    ///
    /// Uses [`SerialTransport::transaction_framed`] with [`RtuFrameParser`]
    /// so the call returns as soon as the complete Modbus frame has arrived
    /// — no inactivity-timeout drain at the tail of every transaction.
    ///
    /// The parser is bound to the request's slave_id and FC so that bytes
    /// left over from an earlier transaction (e.g. another slave's late
    /// response) are rejected before being mistaken for our reply.
    fn transact(&self, request: &[u8]) -> Result<Vec<u8>, String> {
        if request.len() < 2 {
            return Err("Request too short to extract slave_id/FC".into());
        }
        let slave_id = request[0];
        let fc = request[1];

        let mut transport = self.transport.lock()
            .map_err(|e| format!("Transport lock poisoned: {e}"))?;

        // 256 bytes is the maximum legal Modbus RTU PDU including CRC.
        let mut response_buf = [0u8; 256];
        let mut parser = RtuFrameParser::for_request(slave_id, fc);
        let n = transport.transaction_framed(
            request,
            &mut response_buf,
            &mut parser,
            self.timeout,
            self.preamble,
        )?;
        Ok(response_buf[..n].to_vec())
    }
}
