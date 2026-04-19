//! Modbus CRC16 calculation.
//!
//! Uses the standard Modbus polynomial (0xA001, reflected).

/// Compute the Modbus CRC16 for the given data.
/// Returns the CRC as (low_byte, high_byte) — Modbus sends CRC low byte first.
pub fn crc16(data: &[u8]) -> (u8, u8) {
    let mut crc: u16 = 0xFFFF;
    for &byte in data {
        crc ^= byte as u16;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xA001;
            } else {
                crc >>= 1;
            }
        }
    }
    ((crc & 0xFF) as u8, (crc >> 8) as u8)
}

/// Verify the CRC of a Modbus RTU frame (data + 2 CRC bytes).
/// Returns true if the CRC is valid.
pub fn verify_crc(frame: &[u8]) -> bool {
    if frame.len() < 3 {
        return false;
    }
    let data = &frame[..frame.len() - 2];
    let (crc_lo, crc_hi) = crc16(data);
    frame[frame.len() - 2] == crc_lo && frame[frame.len() - 1] == crc_hi
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crc16_known_vector() {
        // Known test vector: slave=1, FC=3, addr=0, count=10
        // Frame: 01 03 00 00 00 0A → CRC should be C5 CD
        let data = [0x01, 0x03, 0x00, 0x00, 0x00, 0x0A];
        let (lo, hi) = crc16(&data);
        assert_eq!((lo, hi), (0xC5, 0xCD), "CRC mismatch for known vector");
    }

    #[test]
    fn crc16_single_byte() {
        let (lo, hi) = crc16(&[0x01]);
        // Just verify it's deterministic
        let (lo2, hi2) = crc16(&[0x01]);
        assert_eq!((lo, hi), (lo2, hi2));
    }

    #[test]
    fn verify_valid_frame() {
        let data = [0x01, 0x03, 0x00, 0x00, 0x00, 0x0A];
        let (crc_lo, crc_hi) = crc16(&data);
        let mut frame = data.to_vec();
        frame.push(crc_lo);
        frame.push(crc_hi);
        assert!(verify_crc(&frame));
    }

    #[test]
    fn verify_corrupted_frame() {
        let mut frame = vec![0x01, 0x03, 0x00, 0x00, 0x00, 0x0A, 0xC5, 0xCD];
        assert!(verify_crc(&frame));
        frame[3] = 0xFF; // corrupt data
        assert!(!verify_crc(&frame));
    }

    #[test]
    fn verify_too_short() {
        assert!(!verify_crc(&[0x01, 0x02]));
        assert!(!verify_crc(&[]));
    }
}
