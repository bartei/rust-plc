//! Streaming frame parsing for length-aware reads on serial transports.
//!
//! Different serial protocols use different framing rules — fixed length,
//! length-prefixed, delimiter-terminated, etc. The [`FrameParser`] trait lets
//! a protocol crate express its framing logic; the transport layer then drives
//! the parser by streaming bytes through it as they arrive on the wire.
//!
//! This decoupling is the only way to receive a frame without an inactivity
//! timeout: the transport otherwise has no way to know that a response is
//! complete, and must wait for the timeout to fire — adding tens to hundreds
//! of milliseconds of dead time per transaction even when the actual response
//! arrived in microseconds.
//!
//! ## Contract
//!
//! [`SerialTransport::transaction_framed`](crate::transport::SerialTransport::transaction_framed)
//! calls [`FrameParser::parse`] with a slice covering all bytes received so
//! far (initially empty). The parser must return one of:
//!
//! - [`FrameStatus::Need`] — frame is incomplete; resume reading and call
//!   `parse` again once at least `min_total` bytes have been received. The
//!   parser must always return a `min_total` strictly greater than the buffer
//!   length it was just given (otherwise the transport would loop forever).
//! - [`FrameStatus::Complete`] — frame is complete and occupies `buf[..len]`.
//!   `len` must be `<= buf.len()` at the call site.
//! - [`FrameStatus::Invalid`] — frame is unrecoverable; the transaction
//!   aborts with the supplied message.

/// Outcome of a single [`FrameParser::parse`] call.
#[derive(Debug, Clone)]
pub enum FrameStatus {
    /// Frame is complete; it occupies `buf[..len]`.
    Complete(usize),
    /// Frame is still incomplete. The transport will read more bytes and
    /// call `parse` again once the buffer has at least `min_total` bytes.
    Need(usize),
    /// Frame is malformed; abort with the given error message.
    Invalid(String),
}

/// A protocol-specific streaming frame parser.
///
/// Implementors describe how to recognise the boundary of a single response
/// frame. The transport layer feeds bytes incrementally and stops reading
/// once the parser reports [`FrameStatus::Complete`].
pub trait FrameParser {
    /// Inspect the bytes received so far and decide how to proceed.
    fn parse(&mut self, buf: &[u8]) -> FrameStatus;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trivial parser used to exercise the framing API without depending
    /// on any specific protocol.
    struct FixedLength(usize);
    impl FrameParser for FixedLength {
        fn parse(&mut self, buf: &[u8]) -> FrameStatus {
            if buf.len() < self.0 {
                FrameStatus::Need(self.0)
            } else {
                FrameStatus::Complete(self.0)
            }
        }
    }

    #[test]
    fn fixed_length_progression() {
        let mut p = FixedLength(5);
        assert!(matches!(p.parse(&[]), FrameStatus::Need(5)));
        assert!(matches!(p.parse(&[1, 2]), FrameStatus::Need(5)));
        assert!(matches!(p.parse(&[1, 2, 3, 4, 5]), FrameStatus::Complete(5)));
        // Trailing bytes do not move the boundary
        assert!(matches!(p.parse(&[1, 2, 3, 4, 5, 6, 7]), FrameStatus::Complete(5)));
    }
}
