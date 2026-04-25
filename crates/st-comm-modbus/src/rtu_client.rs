//! Modbus RTU client — sends requests and parses responses via SerialTransport.

use crate::frame::{self, FunctionCode};
use crate::frame_parser::RtuFrameParser;
use st_comm_serial::transport::SerialTransport;
use std::sync::{Arc, Mutex};

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
}

impl RtuClient {
    pub fn new(transport: Arc<Mutex<SerialTransport>>) -> Self {
        Self { transport }
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
    fn transact(&self, request: &[u8]) -> Result<Vec<u8>, String> {
        let mut transport = self.transport.lock()
            .map_err(|e| format!("Transport lock poisoned: {e}"))?;

        // 256 bytes is the maximum legal Modbus RTU PDU including CRC.
        let mut response_buf = [0u8; 256];
        let mut parser = RtuFrameParser::new();
        let n = transport.transaction_framed(request, &mut response_buf, &mut parser)?;
        Ok(response_buf[..n].to_vec())
    }
}
