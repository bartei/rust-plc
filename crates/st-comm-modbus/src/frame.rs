//! Modbus RTU frame builder and parser.

use crate::crc::crc16;

/// Modbus function codes.
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

/// A Modbus exception response.
#[derive(Debug, Clone)]
pub struct ModbusException {
    pub slave_id: u8,
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

/// Build a Modbus RTU read request frame (FC01/02/03/04).
/// Returns the complete frame including CRC.
pub fn build_read_request(slave_id: u8, fc: FunctionCode, start_addr: u16, count: u16) -> Vec<u8> {
    let mut frame = Vec::with_capacity(8);
    frame.push(slave_id);
    frame.push(fc as u8);
    frame.push((start_addr >> 8) as u8);
    frame.push((start_addr & 0xFF) as u8);
    frame.push((count >> 8) as u8);
    frame.push((count & 0xFF) as u8);
    let (crc_lo, crc_hi) = crc16(&frame);
    frame.push(crc_lo);
    frame.push(crc_hi);
    frame
}

/// Build a FC05 Write Single Coil request.
/// `value`: true = 0xFF00, false = 0x0000.
pub fn build_write_single_coil(slave_id: u8, addr: u16, value: bool) -> Vec<u8> {
    let val = if value { 0xFF00u16 } else { 0x0000 };
    let mut frame = Vec::with_capacity(8);
    frame.push(slave_id);
    frame.push(FunctionCode::WriteSingleCoil as u8);
    frame.push((addr >> 8) as u8);
    frame.push((addr & 0xFF) as u8);
    frame.push((val >> 8) as u8);
    frame.push((val & 0xFF) as u8);
    let (crc_lo, crc_hi) = crc16(&frame);
    frame.push(crc_lo);
    frame.push(crc_hi);
    frame
}

/// Build a FC06 Write Single Register request.
pub fn build_write_single_register(slave_id: u8, addr: u16, value: u16) -> Vec<u8> {
    let mut frame = Vec::with_capacity(8);
    frame.push(slave_id);
    frame.push(FunctionCode::WriteSingleRegister as u8);
    frame.push((addr >> 8) as u8);
    frame.push((addr & 0xFF) as u8);
    frame.push((value >> 8) as u8);
    frame.push((value & 0xFF) as u8);
    let (crc_lo, crc_hi) = crc16(&frame);
    frame.push(crc_lo);
    frame.push(crc_hi);
    frame
}

/// Build a FC0F Write Multiple Coils request.
pub fn build_write_multiple_coils(slave_id: u8, start_addr: u16, coils: &[bool]) -> Vec<u8> {
    let count = coils.len() as u16;
    let byte_count = coils.len().div_ceil(8) as u8;
    let mut frame = Vec::with_capacity(9 + byte_count as usize);
    frame.push(slave_id);
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
    let (crc_lo, crc_hi) = crc16(&frame);
    frame.push(crc_lo);
    frame.push(crc_hi);
    frame
}

/// Build a FC10 Write Multiple Registers request.
pub fn build_write_multiple_registers(slave_id: u8, start_addr: u16, values: &[u16]) -> Vec<u8> {
    let count = values.len() as u16;
    let byte_count = (values.len() * 2) as u8;
    let mut frame = Vec::with_capacity(9 + byte_count as usize);
    frame.push(slave_id);
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
    let (crc_lo, crc_hi) = crc16(&frame);
    frame.push(crc_lo);
    frame.push(crc_hi);
    frame
}

/// Parse a read response (FC01/02/03/04).
/// Returns the data bytes (excluding slave_id, fc, byte_count, and CRC).
pub fn parse_read_response(frame: &[u8]) -> Result<&[u8], String> {
    if frame.len() < 5 {
        return Err("Response too short".into());
    }
    if !crate::crc::verify_crc(frame) {
        return Err("CRC mismatch".into());
    }
    // Check for exception response (FC has bit 7 set)
    if frame[1] & 0x80 != 0 {
        return Err(format!(
            "Modbus exception: {} (code {})",
            ModbusException {
                slave_id: frame[0],
                function_code: frame[1] & 0x7F,
                exception_code: frame[2],
            }
            .description(),
            frame[2],
        ));
    }
    let byte_count = frame[2] as usize;
    if frame.len() < 3 + byte_count + 2 {
        return Err(format!(
            "Response truncated: expected {} data bytes, got {}",
            byte_count,
            frame.len() - 5,
        ));
    }
    Ok(&frame[3..3 + byte_count])
}

/// Parse a write response (FC05/06/0F/10).
/// Returns Ok(()) if the response is valid, Err on exception or CRC.
pub fn parse_write_response(frame: &[u8]) -> Result<(), String> {
    if frame.len() < 5 {
        return Err("Response too short".into());
    }
    if !crate::crc::verify_crc(frame) {
        return Err("CRC mismatch".into());
    }
    if frame[1] & 0x80 != 0 {
        return Err(format!(
            "Modbus exception: {} (code {})",
            ModbusException {
                slave_id: frame[0],
                function_code: frame[1] & 0x7F,
                exception_code: frame[2],
            }
            .description(),
            frame[2],
        ));
    }
    Ok(())
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
    fn build_read_holding_registers() {
        let frame = build_read_request(1, FunctionCode::ReadHoldingRegisters, 0, 10);
        assert_eq!(frame.len(), 8);
        assert_eq!(frame[0], 1);  // slave
        assert_eq!(frame[1], 3);  // FC03
        assert_eq!(frame[2], 0);  // addr hi
        assert_eq!(frame[3], 0);  // addr lo
        assert_eq!(frame[4], 0);  // count hi
        assert_eq!(frame[5], 10); // count lo
        assert!(crate::crc::verify_crc(&frame));
    }

    #[test]
    fn build_read_coils() {
        let frame = build_read_request(1, FunctionCode::ReadCoils, 0, 8);
        assert_eq!(frame[1], 1); // FC01
        assert!(crate::crc::verify_crc(&frame));
    }

    #[test]
    fn build_and_verify_write_single_coil() {
        let frame = build_write_single_coil(1, 5, true);
        assert_eq!(frame[1], 5); // FC05
        assert_eq!(frame[4], 0xFF); // ON
        assert_eq!(frame[5], 0x00);
        assert!(crate::crc::verify_crc(&frame));

        let frame_off = build_write_single_coil(1, 5, false);
        assert_eq!(frame_off[4], 0x00); // OFF
        assert_eq!(frame_off[5], 0x00);
    }

    #[test]
    fn build_and_verify_write_single_register() {
        let frame = build_write_single_register(1, 10, 0x1234);
        assert_eq!(frame[1], 6); // FC06
        assert_eq!(frame[4], 0x12);
        assert_eq!(frame[5], 0x34);
        assert!(crate::crc::verify_crc(&frame));
    }

    #[test]
    fn build_write_multiple_coils_packed() {
        let coils = vec![true, false, true, true, false, false, false, true, true];
        let frame = build_write_multiple_coils(1, 0, &coils);
        assert_eq!(frame[1], 0x0F); // FC0F
        assert_eq!(frame[6], 2);    // byte count = ceil(9/8)
        assert_eq!(frame[7], 0b10001101); // bits 0-7: T F T T F F F T
        assert_eq!(frame[8] & 0x01, 1);   // bit 8: T
        assert!(crate::crc::verify_crc(&frame));
    }

    #[test]
    fn write_multiple_registers_frame() {
        let values = vec![0x0001u16, 0x0002, 0x0003];
        let frame = super::build_write_multiple_registers(1, 100, &values);
        assert_eq!(frame[1], 0x10); // FC10
        assert_eq!(frame[6], 6);    // byte count = 3*2
        assert_eq!(frame[7], 0x00);
        assert_eq!(frame[8], 0x01); // first register
        assert!(crate::crc::verify_crc(&frame));
    }

    #[test]
    fn parse_valid_read_response() {
        // Simulated FC03 response: slave=1, fc=3, byte_count=4, data=[0,1,0,2], CRC
        let mut frame = vec![0x01, 0x03, 0x04, 0x00, 0x01, 0x00, 0x02];
        let (crc_lo, crc_hi) = crate::crc::crc16(&frame);
        frame.push(crc_lo);
        frame.push(crc_hi);

        let data = parse_read_response(&frame).unwrap();
        assert_eq!(data, &[0x00, 0x01, 0x00, 0x02]);

        let regs = extract_registers(data);
        assert_eq!(regs, vec![1, 2]);
    }

    #[test]
    fn parse_exception_response() {
        // Exception: slave=1, fc=0x83 (FC03 + 0x80), exception_code=2
        let mut frame = vec![0x01, 0x83, 0x02];
        let (crc_lo, crc_hi) = crate::crc::crc16(&frame);
        frame.push(crc_lo);
        frame.push(crc_hi);

        let err = parse_read_response(&frame).unwrap_err();
        assert!(err.contains("Illegal data address"), "Got: {err}");
    }

    #[test]
    fn extract_coils_basic() {
        let data = [0b00001101]; // bits: 1,0,1,1,0,0,0,0
        let coils = extract_coils(&data, 4);
        assert_eq!(coils, vec![true, false, true, true]);
    }

    #[test]
    fn extract_coils_multi_byte() {
        let data = [0xFF, 0x01]; // 8 ON bits + 1 ON bit
        let coils = extract_coils(&data, 9);
        assert_eq!(coils.len(), 9);
        assert!(coils[0..8].iter().all(|&b| b));
        assert!(coils[8]);
    }
}
