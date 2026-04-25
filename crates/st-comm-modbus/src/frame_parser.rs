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
/// One instance is created per transaction. The parser validates that the
/// response slave_id and function code match the request — leftover bytes
/// from a previous transaction (e.g. a slave that responded after the
/// master timed out) are rejected immediately rather than being mistaken
/// for the current response.
pub struct RtuFrameParser {
    /// Expected slave_id from the matching request. `None` disables the
    /// check (used by tests that exercise frame-shape parsing only).
    expected_slave_id: Option<u8>,
    /// Expected function code (low 7 bits) from the matching request.
    /// `None` disables the check.
    expected_fc: Option<u8>,
}

impl RtuFrameParser {
    /// Build a parser bound to the slave_id and function code of a request.
    ///
    /// Use this in production: any response whose first two bytes don't
    /// match the request will be reported as [`FrameStatus::Invalid`]
    /// before its CRC is even checked.
    pub fn for_request(slave_id: u8, fc: u8) -> Self {
        Self {
            expected_slave_id: Some(slave_id),
            expected_fc: Some(fc & 0x7F),
        }
    }

    /// Build a permissive parser that accepts any slave_id and function
    /// code. Intended for unit tests of frame-length logic.
    pub fn new() -> Self {
        Self {
            expected_slave_id: None,
            expected_fc: None,
        }
    }
}

impl Default for RtuFrameParser {
    fn default() -> Self {
        Self::new()
    }
}

impl FrameParser for RtuFrameParser {
    fn parse(&mut self, buf: &[u8]) -> FrameStatus {
        // Validate slave_id as soon as it's available.
        if let (Some(expected), Some(&got)) = (self.expected_slave_id, buf.first()) {
            if got != expected {
                return FrameStatus::Invalid(format!(
                    "Slave ID mismatch: expected 0x{expected:02X}, got 0x{got:02X}"
                ));
            }
        }

        // Need slave_id and function code before we can decide anything.
        if buf.len() < 2 {
            return FrameStatus::Need(2);
        }

        let fc = buf[1];

        // Validate function code (allowing the exception bit) against the
        // request once we have it.
        if let Some(expected_fc) = self.expected_fc {
            if (fc & 0x7F) != expected_fc {
                return FrameStatus::Invalid(format!(
                    "Function code mismatch: expected 0x{expected_fc:02X} (or exception), \
                     got 0x{fc:02X}"
                ));
            }
        }

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
    fn slave_id_mismatch_is_invalid() {
        // Sent to slave 20, response arrived from slave 21 (e.g. a previous
        // device's late frame still in the OS buffer).
        let mut p = RtuFrameParser::for_request(20, 0x02);
        match p.parse(&[21, 0x02, 0x01, 0x00, 0xCC, 0xDD]) {
            FrameStatus::Invalid(msg) => {
                assert!(msg.contains("0x14"), "expected error to mention 0x14: {msg}");
                assert!(msg.contains("0x15"), "expected error to mention 0x15: {msg}");
            }
            other => panic!("Expected Invalid for slave mismatch, got {other:?}"),
        }
    }

    #[test]
    fn function_code_mismatch_is_invalid() {
        // Sent FC02, response FC03 — wrong frame.
        let mut p = RtuFrameParser::for_request(1, 0x02);
        match p.parse(&[0x01, 0x03, 0x02, 0x00, 0x05, 0xCC, 0xDD]) {
            FrameStatus::Invalid(msg) => {
                assert!(msg.contains("0x02"), "should mention expected: {msg}");
                assert!(msg.contains("0x03"), "should mention got: {msg}");
            }
            other => panic!("Expected Invalid for FC mismatch, got {other:?}"),
        }
    }

    #[test]
    fn exception_response_passes_fc_validation() {
        // Sent FC03, slave returned exception 0x83 — the parser should
        // accept it as the matching exception variant, not reject it.
        let mut p = RtuFrameParser::for_request(1, 0x03);
        assert_complete(p.parse(&[0x01, 0x83, 0x02, 0xCC, 0xDD]), 5);
    }

    #[test]
    fn slave_id_validated_before_buffer_has_two_bytes() {
        // Single byte received — already enough to reject a wrong slave.
        let mut p = RtuFrameParser::for_request(20, 0x02);
        match p.parse(&[21]) {
            FrameStatus::Invalid(_) => {}
            other => panic!("expected Invalid for slave mismatch on 1 byte, got {other:?}"),
        }
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
