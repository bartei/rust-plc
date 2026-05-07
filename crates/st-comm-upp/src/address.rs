//! UPP device addresses — the 2-digit ASCII prefix on every frame.
//!
//! Per the IGAR 6 Smart manual §4.14:
//!
//! | Range  | Meaning                                                  |
//! |--------|----------------------------------------------------------|
//! | 00..97 | Individual device address (set on the device via `AAga`) |
//! | 98     | Global broadcast **with** response (one-device bus only) |
//! | 99     | Global broadcast **without** response (parameter push)   |
//!
//! The wire encoding is always two ASCII decimal digits with leading
//! zero, e.g. `b"00"`, `b"42"`, `b"99"`.

use crate::error::UppError;
use std::fmt;

/// A UPP address byte pair as it appears at the start of every frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Address {
    /// Individual device address `00..=97`.
    Individual(u8),
    /// Global address `98` — every device on the bus accepts the
    /// command and exactly one is expected to respond. Use only when
    /// a single device is wired to the master (e.g. during initial
    /// bring-up or to discover an unknown address).
    BroadcastWithResponse,
    /// Global address `99` — every device on the bus accepts the
    /// command and **no** device responds. Used to push parameter
    /// changes simultaneously to all pyrometers.
    BroadcastNoResponse,
}

impl Address {
    /// Maximum legal individual address per manual §4.14.
    pub const MAX_INDIVIDUAL: u8 = 97;

    /// Construct an individual address. Errors for `n > 97`.
    pub fn individual(n: u8) -> Result<Self, UppError> {
        if n > Self::MAX_INDIVIDUAL {
            return Err(UppError::OutOfRange(format!(
                "individual address must be 00..={}, got {n}",
                Self::MAX_INDIVIDUAL
            )));
        }
        Ok(Address::Individual(n))
    }

    /// Numeric value (00..99) used to encode the wire bytes.
    pub fn as_u8(&self) -> u8 {
        match self {
            Address::Individual(n) => *n,
            Address::BroadcastWithResponse => 98,
            Address::BroadcastNoResponse => 99,
        }
    }

    /// True if the device should not transmit a response. The client
    /// must skip the read phase for these to avoid a guaranteed 5 ms
    /// timeout per request.
    pub fn expects_response(&self) -> bool {
        !matches!(self, Address::BroadcastNoResponse)
    }

    /// Encode as the 2-byte ASCII prefix that opens every UPP frame.
    pub fn encode(&self) -> [u8; 2] {
        let n = self.as_u8();
        [b'0' + (n / 10), b'0' + (n % 10)]
    }

    /// Parse the 2-byte ASCII prefix from an inbound frame.
    pub fn parse(bytes: &[u8]) -> Result<Self, UppError> {
        if bytes.len() < 2 {
            return Err(UppError::BadResponse(format!(
                "address prefix needs 2 bytes, got {}",
                bytes.len()
            )));
        }
        let a = bytes[0];
        let b = bytes[1];
        if !a.is_ascii_digit() || !b.is_ascii_digit() {
            return Err(UppError::BadResponse(format!(
                "address prefix must be ASCII digits, got {:?}",
                String::from_utf8_lossy(&bytes[..2])
            )));
        }
        let n = (a - b'0') * 10 + (b - b'0');
        match n {
            0..=97 => Ok(Address::Individual(n)),
            98 => Ok(Address::BroadcastWithResponse),
            99 => Ok(Address::BroadcastNoResponse),
            // unreachable: two ASCII digits cap at 99
            _ => Err(UppError::BadResponse(format!("address byte 0x{n:02X}"))),
        }
    }
}

impl fmt::Display for Address {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:02}", self.as_u8())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_matches_manual_example_addr_zero() {
        // Manual §7 worked example: "Entry: '00em' + CR" — address
        // prefix is exactly b"00".
        assert_eq!(&Address::Individual(0).encode(), b"00");
    }

    #[test]
    fn encode_pads_single_digits() {
        assert_eq!(&Address::Individual(7).encode(), b"07");
        assert_eq!(&Address::Individual(42).encode(), b"42");
        assert_eq!(&Address::Individual(97).encode(), b"97");
    }

    #[test]
    fn broadcast_addresses_encode_correctly() {
        assert_eq!(&Address::BroadcastWithResponse.encode(), b"98");
        assert_eq!(&Address::BroadcastNoResponse.encode(), b"99");
    }

    #[test]
    fn individual_rejects_98_and_above() {
        assert!(Address::individual(98).is_err());
        assert!(Address::individual(99).is_err());
        assert!(Address::individual(255).is_err());
        assert!(Address::individual(97).is_ok());
    }

    #[test]
    fn expects_response_only_false_for_99() {
        assert!(Address::Individual(0).expects_response());
        assert!(Address::Individual(97).expects_response());
        assert!(Address::BroadcastWithResponse.expects_response());
        assert!(!Address::BroadcastNoResponse.expects_response());
    }

    #[test]
    fn parse_round_trip() {
        for n in 0..=99u8 {
            let mut buf = [0u8; 2];
            buf[0] = b'0' + n / 10;
            buf[1] = b'0' + n % 10;
            let a = Address::parse(&buf).expect("valid digits");
            assert_eq!(a.as_u8(), n);
            assert_eq!(a.encode(), buf);
        }
    }

    #[test]
    fn parse_rejects_non_digits() {
        assert!(matches!(
            Address::parse(b"0X"),
            Err(UppError::BadResponse(_))
        ));
        assert!(matches!(
            Address::parse(b"X0"),
            Err(UppError::BadResponse(_))
        ));
    }

    #[test]
    fn parse_rejects_short_input() {
        assert!(matches!(Address::parse(b""), Err(UppError::BadResponse(_))));
        assert!(matches!(Address::parse(b"0"), Err(UppError::BadResponse(_))));
    }

    #[test]
    fn display_pads_to_two_digits() {
        assert_eq!(Address::Individual(0).to_string(), "00");
        assert_eq!(Address::Individual(7).to_string(), "07");
        assert_eq!(Address::BroadcastWithResponse.to_string(), "98");
    }
}
