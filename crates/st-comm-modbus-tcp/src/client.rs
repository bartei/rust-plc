//! Modbus TCP client — sends requests and parses responses via TcpTransport.

use crate::frame::{self, FunctionCode};
use crate::transport::TcpTransport;

/// Modbus TCP client operating over a TCP transport.
///
/// Each client owns a mutable reference to its transport (no sharing needed
/// since TCP connections are point-to-point, unlike serial buses).
pub struct TcpModbusClient<'a> {
    transport: &'a mut TcpTransport,
    transaction_id: u16,
}

impl<'a> TcpModbusClient<'a> {
    pub fn new(transport: &'a mut TcpTransport) -> Self {
        Self {
            transport,
            transaction_id: 0,
        }
    }

    fn next_transaction_id(&mut self) -> u16 {
        self.transaction_id = self.transaction_id.wrapping_add(1);
        self.transaction_id
    }

    /// Read coils (FC01).
    pub fn read_coils(
        &mut self,
        unit_id: u8,
        start: u16,
        count: u16,
    ) -> Result<Vec<bool>, String> {
        let txn_id = self.next_transaction_id();
        let request =
            frame::build_read_request(txn_id, unit_id, FunctionCode::ReadCoils, start, count);
        let (_, data) = self.transact(&request)?;
        Ok(frame::extract_coils(&data, count as usize))
    }

    /// Read discrete inputs (FC02).
    pub fn read_discrete_inputs(
        &mut self,
        unit_id: u8,
        start: u16,
        count: u16,
    ) -> Result<Vec<bool>, String> {
        let txn_id = self.next_transaction_id();
        let request = frame::build_read_request(
            txn_id,
            unit_id,
            FunctionCode::ReadDiscreteInputs,
            start,
            count,
        );
        let (_, data) = self.transact(&request)?;
        Ok(frame::extract_coils(&data, count as usize))
    }

    /// Read holding registers (FC03).
    pub fn read_holding_registers(
        &mut self,
        unit_id: u8,
        start: u16,
        count: u16,
    ) -> Result<Vec<u16>, String> {
        let txn_id = self.next_transaction_id();
        let request = frame::build_read_request(
            txn_id,
            unit_id,
            FunctionCode::ReadHoldingRegisters,
            start,
            count,
        );
        let (_, data) = self.transact(&request)?;
        Ok(frame::extract_registers(&data))
    }

    /// Read input registers (FC04).
    pub fn read_input_registers(
        &mut self,
        unit_id: u8,
        start: u16,
        count: u16,
    ) -> Result<Vec<u16>, String> {
        let txn_id = self.next_transaction_id();
        let request = frame::build_read_request(
            txn_id,
            unit_id,
            FunctionCode::ReadInputRegisters,
            start,
            count,
        );
        let (_, data) = self.transact(&request)?;
        Ok(frame::extract_registers(&data))
    }

    /// Write single coil (FC05).
    pub fn write_single_coil(
        &mut self,
        unit_id: u8,
        addr: u16,
        value: bool,
    ) -> Result<(), String> {
        let txn_id = self.next_transaction_id();
        let request = frame::build_write_single_coil(txn_id, unit_id, addr, value);
        self.transact(&request)?;
        Ok(())
    }

    /// Write single register (FC06).
    pub fn write_single_register(
        &mut self,
        unit_id: u8,
        addr: u16,
        value: u16,
    ) -> Result<(), String> {
        let txn_id = self.next_transaction_id();
        let request = frame::build_write_single_register(txn_id, unit_id, addr, value);
        self.transact(&request)?;
        Ok(())
    }

    /// Write multiple coils (FC0F).
    pub fn write_multiple_coils(
        &mut self,
        unit_id: u8,
        start: u16,
        coils: &[bool],
    ) -> Result<(), String> {
        let txn_id = self.next_transaction_id();
        let request = frame::build_write_multiple_coils(txn_id, unit_id, start, coils);
        self.transact(&request)?;
        Ok(())
    }

    /// Write multiple registers (FC10).
    pub fn write_multiple_registers(
        &mut self,
        unit_id: u8,
        start: u16,
        values: &[u16],
    ) -> Result<(), String> {
        let txn_id = self.next_transaction_id();
        let request = frame::build_write_multiple_registers(txn_id, unit_id, start, values);
        self.transact(&request)?;
        Ok(())
    }

    /// Send a request and parse the response.
    fn transact(&mut self, request: &[u8]) -> Result<(u16, Vec<u8>), String> {
        let mut response_buf = [0u8; 512];
        let n = self.transport.transaction(request, &mut response_buf)?;
        if n == 0 {
            return Err("No response (timeout)".into());
        }
        frame::parse_response(&response_buf[..n])
    }
}
