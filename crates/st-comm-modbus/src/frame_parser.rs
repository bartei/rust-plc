//! Streaming frame parser for Modbus RTU responses.
//!
//! Implements [`FrameParser`] so that [`SerialTransport::transaction_framed`]
//! can stop reading the moment a complete response has arrived, instead of
//! draining the OS read buffer until the configured inactivity timeout fires.
//! Eliminating that drain is the difference between ~5 ms and ~105 ms per
//! transaction at typical PLC timeouts.
//!
//! ## Frame shapes
//!
//! - Read responses (FC 01/02/03/04): `slave | fc | byte_count | data… | crc_lo | crc_hi`
//!   → total length = `5 + byte_count`.
//! - Single writes (FC 05/06) and multiple-element writes (FC 0F/10) all
//!   echo back the address and quantity → fixed 8 bytes total.
//! - Exception responses (`fc & 0x80`): `slave | (fc | 0x80) | exc | crc_lo | crc_hi`
//!   → fixed 5 bytes total.
//!
//! [`FrameParser`]: st_comm_serial::framing::FrameParser
//! [`SerialTransport::transaction_framed`]: st_comm_serial::transport::SerialTransport::transaction_framed

use st_comm_serial::framing::{FrameParser, FrameStatus};

/// Parses a single Modbus RTU response frame as bytes stream in.
///
/// One instance is created per transaction. The parser is stateless beyond
/// what it can derive from the buffer it has already seen, so it is also
/// safe to reuse an instance across transactions if a caller wishes to.
pub struct RtuFrameParser;

impl RtuFrameParser {
    pub fn new() -> Self {
        Self
    }
}

impl Default for RtuFrameParser {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameParser for RtuFrameParser {
    fn parse(&mut self, buf: &[u8]) -> FrameStatus {
        // Need slave_id and function code before we can decide anything.
        if buf.len() < 2 {
            return FrameStatus::Need(2);
        }

        let fc = buf[1];

        // Exception response — high bit of FC is set. Fixed 5-byte frame.
        if fc & 0x80 != 0 {
            return if buf.len() < 5 {
                FrameStatus::Need(5)
            } else {
                FrameStatus::Complete(5)
            };
        }

        match fc {
            // Read coils / discrete inputs / holding registers / input registers.
            // Length is determined by the byte_count field at offset 2.
            0x01..=0x04 => {
                if buf.len() < 3 {
                    return FrameStatus::Need(3);
                }
                let byte_count = buf[2] as usize;
                let total = 3 + byte_count + 2; // header + data + CRC
                if buf.len() < total {
                    FrameStatus::Need(total)
                } else {
                    FrameStatus::Complete(total)
                }
            }
            // Write single coil (FC05), write single register (FC06),
            // write multiple coils (FC0F), write multiple registers (FC10).
            // All echo address+quantity → fixed 8 bytes.
            0x05 | 0x06 | 0x0F | 0x10 => {
                if buf.len() < 8 {
                    FrameStatus::Need(8)
                } else {
                    FrameStatus::Complete(8)
                }
            }
            other => FrameStatus::Invalid(format!(
                "Unrecognised Modbus function code 0x{other:02X} in response"
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[track_caller]
    fn assert_need(status: FrameStatus, expected: usize) {
        match status {
            FrameStatus::Need(n) => assert_eq!(n, expected, "Need value mismatch"),
            other => panic!("Expected Need({expected}), got {other:?}"),
        }
    }

    #[track_caller]
    fn assert_complete(status: FrameStatus, expected: usize) {
        match status {
            FrameStatus::Complete(n) => assert_eq!(n, expected, "Complete value mismatch"),
            other => panic!("Expected Complete({expected}), got {other:?}"),
        }
    }

    #[test]
    fn empty_buffer_needs_two_bytes() {
        let mut p = RtuFrameParser::new();
        assert_need(p.parse(&[]), 2);
    }

    #[test]
    fn read_holding_progression() {
        // FC03 with byte_count = 4 → total 9 bytes.
        let mut p = RtuFrameParser::new();
        assert_need(p.parse(&[0x01]), 2);
        assert_need(p.parse(&[0x01, 0x03]), 3);
        assert_need(p.parse(&[0x01, 0x03, 0x04]), 9);
        assert_need(p.parse(&[0x01, 0x03, 0x04, 0x00, 0x01, 0x00]), 9);
        assert_complete(
            p.parse(&[0x01, 0x03, 0x04, 0x00, 0x01, 0x00, 0x02, 0xCC, 0xDD]),
            9,
        );
    }

    #[test]
    fn read_coils_progression() {
        // FC01 with byte_count = 1 (8 coils packed) → total 6 bytes.
        let mut p = RtuFrameParser::new();
        assert_need(p.parse(&[0x05, 0x01]), 3);
        assert_need(p.parse(&[0x05, 0x01, 0x01]), 6);
        assert_complete(p.parse(&[0x05, 0x01, 0x01, 0xFF, 0xCC, 0xDD]), 6);
    }

    #[test]
    fn read_discrete_inputs_zero_byte_count_edge() {
        // Pathological but valid frame: byte_count = 0 → total 5 bytes.
        let mut p = RtuFrameParser::new();
        assert_need(p.parse(&[0x01, 0x02, 0x00]), 5);
        assert_complete(p.parse(&[0x01, 0x02, 0x00, 0xCC, 0xDD]), 5);
    }

    #[test]
    fn write_single_coil_fixed_eight_bytes() {
        let mut p = RtuFrameParser::new();
        assert_need(p.parse(&[0x01, 0x05]), 8);
        assert_need(p.parse(&[0x01, 0x05, 0x00, 0x05, 0xFF, 0x00, 0x00]), 8);
        assert_complete(
            p.parse(&[0x01, 0x05, 0x00, 0x05, 0xFF, 0x00, 0xCC, 0xDD]),
            8,
        );
    }

    #[test]
    fn write_single_register_fixed_eight_bytes() {
        let mut p = RtuFrameParser::new();
        assert_need(p.parse(&[0x01, 0x06]), 8);
        assert_complete(
            p.parse(&[0x01, 0x06, 0x00, 0x0A, 0x12, 0x34, 0xCC, 0xDD]),
            8,
        );
    }

    #[test]
    fn write_multiple_coils_fixed_eight_bytes() {
        let mut p = RtuFrameParser::new();
        assert_need(p.parse(&[0x01, 0x0F]), 8);
        assert_complete(
            p.parse(&[0x01, 0x0F, 0x00, 0x00, 0x00, 0x08, 0xCC, 0xDD]),
            8,
        );
    }

    #[test]
    fn write_multiple_registers_fixed_eight_bytes() {
        let mut p = RtuFrameParser::new();
        assert_need(p.parse(&[0x01, 0x10]), 8);
        assert_complete(
            p.parse(&[0x01, 0x10, 0x00, 0x00, 0x00, 0x03, 0xCC, 0xDD]),
            8,
        );
    }

    #[test]
    fn exception_response_fixed_five_bytes() {
        // FC03 + 0x80 = 0x83 → exception
        let mut p = RtuFrameParser::new();
        assert_need(p.parse(&[0x01, 0x83]), 5);
        assert_need(p.parse(&[0x01, 0x83, 0x02]), 5);
        assert_complete(p.parse(&[0x01, 0x83, 0x02, 0xCC, 0xDD]), 5);
    }

    #[test]
    fn exception_response_for_write_function() {
        // FC10 + 0x80 = 0x90 → exception still 5 bytes
        let mut p = RtuFrameParser::new();
        assert_complete(p.parse(&[0x01, 0x90, 0x04, 0xCC, 0xDD]), 5);
    }

    #[test]
    fn unknown_function_code_is_invalid() {
        let mut p = RtuFrameParser::new();
        match p.parse(&[0x01, 0x42]) {
            FrameStatus::Invalid(msg) => {
                assert!(msg.contains("0x42"), "Error should mention the FC: {msg}");
            }
            other => panic!("Expected Invalid for FC 0x42, got {other:?}"),
        }
    }

    #[test]
    fn large_read_response_total_within_modbus_limits() {
        // FC03 with byte_count = 250 (max practical for a 256-byte buffer)
        // → total = 255 bytes.
        let mut p = RtuFrameParser::new();
        let header = [0x01, 0x03, 250];
        assert_need(p.parse(&header), 255);

        let mut full = vec![0u8; 255];
        full[..3].copy_from_slice(&header);
        assert_complete(p.parse(&full), 255);
    }

    #[test]
    fn complete_does_not_consume_trailing_bytes() {
        // Even if more bytes are present in the buffer, Complete returns the
        // exact frame length so the transport doesn't run past it.
        let mut p = RtuFrameParser::new();
        let frame = [0x01, 0x03, 0x02, 0x00, 0x05, 0xCC, 0xDD, /* trailing */ 0xAA, 0xBB];
        assert_complete(p.parse(&frame), 7);
    }
}
