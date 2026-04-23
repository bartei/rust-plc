//! Modbus TCP frame builder and parser (MBAP framing).
//!
//! Modbus TCP uses the MBAP (Modbus Application Protocol) header instead
//! of the RTU slave_id + CRC framing:
//!
//! ```text
//! [Transaction ID: 2B][Protocol ID: 2B = 0x0000][Length: 2B][Unit ID: 1B][PDU...]
//! ```
//!
//! The PDU (Protocol Data Unit) is identical to RTU: function code + data.
//! No CRC is needed — TCP handles data integrity.

/// MBAP header length in bytes.
pub const MBAP_HEADER_LEN: usize = 7;

/// Modbus protocol identifier (always 0x0000 for Modbus).
const PROTOCOL_ID: u16 = 0x0000;

/// Modbus function codes (same as RTU).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FunctionCode {
    ReadCoils = 0x01,
    ReadDiscreteInputs = 0x02,
    ReadHoldingRegisters = 0x03,
    ReadInputRegisters = 0x04,
    WriteSingleCoil = 0x05,
    WriteSingleRegister = 0x06,
    WriteMultipleCoils = 0x0F,
    WriteMultipleRegisters = 0x10,
}

/// Build the MBAP header + PDU prefix for a frame.
///
/// `pdu_len` is the length of the PDU (function code + data).
/// The MBAP length field = pdu_len + 1 (for unit_id byte).
fn build_mbap_header(transaction_id: u16, unit_id: u8, pdu_len: usize) -> Vec<u8> {
    let length = (pdu_len + 1) as u16; // +1 for unit_id
    let mut header = Vec::with_capacity(MBAP_HEADER_LEN + pdu_len);
    header.push((transaction_id >> 8) as u8);
    header.push((transaction_id & 0xFF) as u8);
    header.push((PROTOCOL_ID >> 8) as u8);
    header.push((PROTOCOL_ID & 0xFF) as u8);
    header.push((length >> 8) as u8);
    header.push((length & 0xFF) as u8);
    header.push(unit_id);
    header
}

/// Build a Modbus TCP read request (FC01/02/03/04).
pub fn build_read_request(
    transaction_id: u16,
    unit_id: u8,
    fc: FunctionCode,
    start_addr: u16,
    count: u16,
) -> Vec<u8> {
    let pdu_len = 5; // FC(1) + addr(2) + count(2)
    let mut frame = build_mbap_header(transaction_id, unit_id, pdu_len);
    frame.push(fc as u8);
    frame.push((start_addr >> 8) as u8);
    frame.push((start_addr & 0xFF) as u8);
    frame.push((count >> 8) as u8);
    frame.push((count & 0xFF) as u8);
    frame
}

/// Build a FC05 Write Single Coil request.
pub fn build_write_single_coil(
    transaction_id: u16,
    unit_id: u8,
    addr: u16,
    value: bool,
) -> Vec<u8> {
    let val = if value { 0xFF00u16 } else { 0x0000 };
    let pdu_len = 5; // FC(1) + addr(2) + value(2)
    let mut frame = build_mbap_header(transaction_id, unit_id, pdu_len);
    frame.push(FunctionCode::WriteSingleCoil as u8);
    frame.push((addr >> 8) as u8);
    frame.push((addr & 0xFF) as u8);
    frame.push((val >> 8) as u8);
    frame.push((val & 0xFF) as u8);
    frame
}

/// Build a FC06 Write Single Register request.
pub fn build_write_single_register(
    transaction_id: u16,
    unit_id: u8,
    addr: u16,
    value: u16,
) -> Vec<u8> {
    let pdu_len = 5; // FC(1) + addr(2) + value(2)
    let mut frame = build_mbap_header(transaction_id, unit_id, pdu_len);
    frame.push(FunctionCode::WriteSingleRegister as u8);
    frame.push((addr >> 8) as u8);
    frame.push((addr & 0xFF) as u8);
    frame.push((value >> 8) as u8);
    frame.push((value & 0xFF) as u8);
    frame
}

/// Build a FC0F Write Multiple Coils request.
pub fn build_write_multiple_coils(
    transaction_id: u16,
    unit_id: u8,
    start_addr: u16,
    coils: &[bool],
) -> Vec<u8> {
    let count = coils.len() as u16;
    let byte_count = coils.len().div_ceil(8) as u8;
    let pdu_len = 6 + byte_count as usize; // FC(1) + addr(2) + count(2) + byte_count(1) + data
    let mut frame = build_mbap_header(transaction_id, unit_id, pdu_len);
    frame.push(FunctionCode::WriteMultipleCoils as u8);
    frame.push((start_addr >> 8) as u8);
    frame.push((start_addr & 0xFF) as u8);
    frame.push((count >> 8) as u8);
    frame.push((count & 0xFF) as u8);
    frame.push(byte_count);
    // Pack coils into bytes (LSB first)
    for chunk_idx in 0..byte_count as usize {
        let mut byte = 0u8;
        for bit in 0..8 {
            let idx = chunk_idx * 8 + bit;
            if idx < coils.len() && coils[idx] {
                byte |= 1 << bit;
            }
        }
        frame.push(byte);
    }
    frame
}

/// Build a FC10 Write Multiple Registers request.
pub fn build_write_multiple_registers(
    transaction_id: u16,
    unit_id: u8,
    start_addr: u16,
    values: &[u16],
) -> Vec<u8> {
    let count = values.len() as u16;
    let byte_count = (values.len() * 2) as u8;
    let pdu_len = 6 + byte_count as usize; // FC(1) + addr(2) + count(2) + byte_count(1) + data
    let mut frame = build_mbap_header(transaction_id, unit_id, pdu_len);
    frame.push(FunctionCode::WriteMultipleRegisters as u8);
    frame.push((start_addr >> 8) as u8);
    frame.push((start_addr & 0xFF) as u8);
    frame.push((count >> 8) as u8);
    frame.push((count & 0xFF) as u8);
    frame.push(byte_count);
    for &val in values {
        frame.push((val >> 8) as u8);
        frame.push((val & 0xFF) as u8);
    }
    frame
}

/// A Modbus exception response.
#[derive(Debug, Clone)]
pub struct ModbusException {
    pub function_code: u8,
    pub exception_code: u8,
}

impl ModbusException {
    pub fn description(&self) -> &'static str {
        match self.exception_code {
            0x01 => "Illegal function",
            0x02 => "Illegal data address",
            0x03 => "Illegal data value",
            0x04 => "Slave device failure",
            0x05 => "Acknowledge",
            0x06 => "Slave device busy",
            _ => "Unknown exception",
        }
    }
}

/// Parse a complete Modbus TCP response frame (MBAP header + PDU).
///
/// Returns `(transaction_id, data_bytes)` where `data_bytes` is the response
/// data section (excluding MBAP header, unit_id, FC, and byte_count).
///
/// For read responses (FC01-FC04): returns the register/coil data.
/// For write responses (FC05/06/0F/10): returns empty (success) or error.
pub fn parse_response(frame: &[u8]) -> Result<(u16, Vec<u8>), String> {
    if frame.len() < MBAP_HEADER_LEN + 1 {
        return Err("Response too short for MBAP header".into());
    }

    let transaction_id = ((frame[0] as u16) << 8) | frame[1] as u16;
    let protocol_id = ((frame[2] as u16) << 8) | frame[3] as u16;
    if protocol_id != PROTOCOL_ID {
        return Err(format!("Invalid protocol ID: {protocol_id:#06x}"));
    }

    // PDU starts at byte 7 (after MBAP header)
    let fc = frame[7];

    // Check for exception response (FC has bit 7 set)
    if fc & 0x80 != 0 {
        if frame.len() < MBAP_HEADER_LEN + 2 {
            return Err("Exception response too short".into());
        }
        let exc = ModbusException {
            function_code: fc & 0x7F,
            exception_code: frame[8],
        };
        return Err(format!(
            "Modbus exception: {} (code {})",
            exc.description(),
            exc.exception_code,
        ));
    }

    // Determine response type by function code
    match fc {
        // Read responses: FC byte + byte_count + data
        0x01..=0x04 => {
            if frame.len() < MBAP_HEADER_LEN + 2 {
                return Err("Read response too short".into());
            }
            let byte_count = frame[8] as usize;
            let data_start = MBAP_HEADER_LEN + 2; // after unit_id + FC + byte_count
            let data_end = data_start + byte_count;
            if frame.len() < data_end {
                return Err(format!(
                    "Read response truncated: expected {byte_count} data bytes",
                ));
            }
            Ok((transaction_id, frame[data_start..data_end].to_vec()))
        }
        // Write responses: FC byte + echo of request params (no data to extract)
        0x05 | 0x06 | 0x0F | 0x10 => Ok((transaction_id, Vec::new())),
        _ => Err(format!("Unknown function code: {fc:#04x}")),
    }
}

/// Extract register values from a read response data section.
/// Each register is 2 bytes, big-endian.
pub fn extract_registers(data: &[u8]) -> Vec<u16> {
    data.chunks(2)
        .filter(|c| c.len() == 2)
        .map(|c| (c[0] as u16) << 8 | c[1] as u16)
        .collect()
}

/// Extract coil/discrete input values from a read response data section.
/// Returns a vec of bools, one per bit, LSB first per byte.
pub fn extract_coils(data: &[u8], count: usize) -> Vec<bool> {
    let mut result = Vec::with_capacity(count);
    for &byte in data {
        for bit in 0..8 {
            if result.len() >= count {
                break;
            }
            result.push(byte & (1 << bit) != 0);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_read_holding_registers_frame() {
        let frame = build_read_request(1, 0xFF, FunctionCode::ReadHoldingRegisters, 0, 10);
        // MBAP header: 7 bytes + PDU: 5 bytes = 12
        assert_eq!(frame.len(), 12);
        // Transaction ID
        assert_eq!(frame[0], 0x00);
        assert_eq!(frame[1], 0x01);
        // Protocol ID
        assert_eq!(frame[2], 0x00);
        assert_eq!(frame[3], 0x00);
        // Length: unit_id(1) + FC(1) + addr(2) + count(2) = 6
        assert_eq!(frame[4], 0x00);
        assert_eq!(frame[5], 0x06);
        // Unit ID
        assert_eq!(frame[6], 0xFF);
        // FC03
        assert_eq!(frame[7], 0x03);
        // Start addr = 0
        assert_eq!(frame[8], 0x00);
        assert_eq!(frame[9], 0x00);
        // Count = 10
        assert_eq!(frame[10], 0x00);
        assert_eq!(frame[11], 0x0A);
    }

    #[test]
    fn build_read_coils_frame() {
        let frame = build_read_request(42, 1, FunctionCode::ReadCoils, 0, 8);
        assert_eq!(frame[7], 0x01); // FC01
        // Transaction ID = 42
        assert_eq!(frame[0], 0x00);
        assert_eq!(frame[1], 42);
    }

    #[test]
    fn build_write_single_coil_on() {
        let frame = build_write_single_coil(1, 1, 5, true);
        assert_eq!(frame[7], 0x05); // FC05
        assert_eq!(frame[10], 0xFF); // ON
        assert_eq!(frame[11], 0x00);
    }

    #[test]
    fn build_write_single_coil_off() {
        let frame = build_write_single_coil(1, 1, 5, false);
        assert_eq!(frame[10], 0x00); // OFF
        assert_eq!(frame[11], 0x00);
    }

    #[test]
    fn build_write_single_register_frame() {
        let frame = build_write_single_register(1, 1, 10, 0x1234);
        assert_eq!(frame[7], 0x06); // FC06
        assert_eq!(frame[10], 0x12);
        assert_eq!(frame[11], 0x34);
    }

    #[test]
    fn build_write_multiple_coils_packed() {
        let coils = vec![true, false, true, true, false, false, false, true, true];
        let frame = build_write_multiple_coils(1, 1, 0, &coils);
        assert_eq!(frame[7], 0x0F); // FC0F
        // MBAP(7) + FC(1) + addr(2) + count(2) = 12, then byte_count at 12
        assert_eq!(frame[12], 2); // ceil(9/8) = 2 bytes
        assert_eq!(frame[13], 0b10001101); // bits 0-7: T F T T F F F T
        assert_eq!(frame[14] & 0x01, 1); // bit 8: T
    }

    #[test]
    fn build_write_multiple_registers_frame() {
        let values = vec![0x0001u16, 0x0002, 0x0003];
        let frame = build_write_multiple_registers(1, 1, 100, &values);
        assert_eq!(frame[7], 0x10); // FC10
        // MBAP(7) + FC(1) + addr(2) + count(2) = 12, then byte_count at 12
        assert_eq!(frame[12], 6); // byte_count = 3*2
        assert_eq!(frame[13], 0x00);
        assert_eq!(frame[14], 0x01); // first register value
    }

    #[test]
    fn parse_read_holding_registers_response() {
        // Simulate FC03 response: txn_id=1, unit_id=0xFF, fc=3, byte_count=4, data=[0,1,0,2]
        let frame = vec![
            0x00, 0x01, // transaction id
            0x00, 0x00, // protocol id
            0x00, 0x07, // length: unit_id(1) + fc(1) + byte_count(1) + data(4) = 7
            0xFF, // unit id
            0x03, // FC03
            0x04, // byte count
            0x00, 0x01, 0x00, 0x02, // data: register 0=1, register 1=2
        ];
        let (txn_id, data) = parse_response(&frame).unwrap();
        assert_eq!(txn_id, 1);
        assert_eq!(data, &[0x00, 0x01, 0x00, 0x02]);

        let regs = extract_registers(&data);
        assert_eq!(regs, vec![1, 2]);
    }

    #[test]
    fn parse_read_coils_response() {
        // FC01 response: 1 byte of coil data (8 coils)
        let frame = vec![
            0x00, 0x02, // transaction id = 2
            0x00, 0x00, // protocol id
            0x00, 0x04, // length: unit_id(1) + fc(1) + byte_count(1) + data(1) = 4
            0x01, // unit id
            0x01, // FC01
            0x01, // byte count
            0b00001101, // coils: T F T T F F F F
        ];
        let (txn_id, data) = parse_response(&frame).unwrap();
        assert_eq!(txn_id, 2);
        let coils = extract_coils(&data, 4);
        assert_eq!(coils, vec![true, false, true, true]);
    }

    #[test]
    fn parse_exception_response() {
        // Exception: FC03 + 0x80 = 0x83, exception code 2 (illegal data address)
        let frame = vec![
            0x00, 0x01, // transaction id
            0x00, 0x00, // protocol id
            0x00, 0x03, // length: unit_id(1) + fc(1) + exception_code(1) = 3
            0x01, // unit id
            0x83, // FC03 + 0x80
            0x02, // exception code: illegal data address
        ];
        let err = parse_response(&frame).unwrap_err();
        assert!(err.contains("Illegal data address"), "Got: {err}");
    }

    #[test]
    fn parse_write_single_coil_response() {
        // FC05 echo response
        let frame = vec![
            0x00, 0x05, // transaction id
            0x00, 0x00, // protocol id
            0x00, 0x06, // length
            0x01, // unit id
            0x05, // FC05
            0x00, 0x05, // addr
            0xFF, 0x00, // value
        ];
        let (txn_id, data) = parse_response(&frame).unwrap();
        assert_eq!(txn_id, 5);
        assert!(data.is_empty()); // write responses have no data payload
    }

    #[test]
    fn extract_coils_multi_byte() {
        let data = [0xFF, 0x01]; // 8 ON bits + 1 ON bit
        let coils = extract_coils(&data, 9);
        assert_eq!(coils.len(), 9);
        assert!(coils[0..8].iter().all(|&b| b));
        assert!(coils[8]);
    }

    #[test]
    fn mbap_length_field_correct() {
        // FC03 read 10 holding registers: PDU = FC(1) + addr(2) + count(2) = 5
        // MBAP length = unit_id(1) + PDU(5) = 6
        let frame = build_read_request(1, 1, FunctionCode::ReadHoldingRegisters, 0, 10);
        let length = ((frame[4] as u16) << 8) | frame[5] as u16;
        assert_eq!(length, 6);

        // FC10 write 3 registers: PDU = FC(1) + addr(2) + count(2) + byte_count(1) + data(6) = 12
        // MBAP length = unit_id(1) + PDU(12) = 13
        let frame = build_write_multiple_registers(1, 1, 0, &[1, 2, 3]);
        let length = ((frame[4] as u16) << 8) | frame[5] as u16;
        assert_eq!(length, 13);
    }
}
