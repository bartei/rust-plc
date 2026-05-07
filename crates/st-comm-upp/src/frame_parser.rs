//! Streaming frame parser for UPP responses.
//!
//! Per manual §7, every response ends in `CR` (0x0D). There is no
//! length field, so the only way to know "the response is complete"
//! is to scan for `CR`. This parser plugs into
//! [`SerialTransport::transaction_framed`](st_comm_serial::SerialTransport::transaction_framed)
//! and returns:
//!
//! - [`FrameStatus::Need(n+1)`](st_comm_serial::FrameStatus::Need) while
//!   `CR` has not yet appeared (asks the transport to read at least
//!   one more byte — the contract requires a value strictly greater
//!   than the current buffer length).
//! - [`FrameStatus::Complete(n+1)`](st_comm_serial::FrameStatus::Complete)
//!   the moment a `CR` is seen, where `n+1` includes the terminator.
//! - [`FrameStatus::Invalid`](st_comm_serial::FrameStatus::Invalid) if
//!   the response is so long that we'd overrun a sane bound — this
//!   is a defence-in-depth guard against an adversarial / faulty
//!   device sending an endless stream of bytes. The hard cap is
//!   chosen well above the longest legitimate response in the
//!   manual.
//!
//! The 5 ms response window itself is enforced by the transport's
//! `timeout` parameter, not by this parser. The parser only knows
//! about content shape.

use crate::command::CR;
use st_comm_serial::{FrameParser, FrameStatus};

/// Largest legitimate UPP response payload (excluding the CR
/// terminator). The longest answer in manual §7 is the 16-character
/// device-type string from `na`. We cap at 64 to leave headroom for
/// future commands and still detect a runaway response.
pub const MAX_RESPONSE_LEN: usize = 64;

/// Streaming UPP frame parser. Stateless.
#[derive(Debug, Default, Clone, Copy)]
pub struct UppFrameParser;

impl FrameParser for UppFrameParser {
    fn parse(&mut self, buf: &[u8]) -> FrameStatus {
        if let Some(idx) = buf.iter().position(|b| *b == CR) {
            // Frame ends at the CR. Total length includes the CR
            // itself so the transport can hand the full byte string
            // (including terminator) back to us if it wants — the
            // client strips the prefix and CR before decoding.
            return FrameStatus::Complete(idx + 1);
        }
        if buf.len() >= MAX_RESPONSE_LEN {
            return FrameStatus::Invalid(format!(
                "UPP response exceeded {MAX_RESPONSE_LEN} bytes without CR"
            ));
        }
        // Ask for one more byte than we currently hold. The
        // FrameParser contract requires `min_total > buf.len()`.
        FrameStatus::Need(buf.len() + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_buffer_asks_for_one_byte() {
        let mut p = UppFrameParser;
        match p.parse(&[]) {
            FrameStatus::Need(n) => assert_eq!(n, 1),
            other => panic!("expected Need(1), got {other:?}"),
        }
    }

    #[test]
    fn partial_response_asks_for_more() {
        let mut p = UppFrameParser;
        // Manual example "0970" (no CR yet) — typical mid-frame
        // state.
        match p.parse(b"0970") {
            FrameStatus::Need(n) => assert_eq!(n, 5, "must request strictly more than {}", 4),
            other => panic!("expected Need(5), got {other:?}"),
        }
    }

    #[test]
    fn cr_terminates_frame_immediately() {
        let mut p = UppFrameParser;
        // Manual example "0970" + CR — the parser should see the
        // CR at index 4 and return Complete(5).
        match p.parse(b"0970\r") {
            FrameStatus::Complete(n) => assert_eq!(n, 5),
            other => panic!("expected Complete(5), got {other:?}"),
        }
    }

    #[test]
    fn cr_at_position_zero() {
        // Bare CR — pathological but well-defined. A 0-byte payload
        // is invalid in UPP, but the framer only enforces shape;
        // payload-emptiness is the decoder's problem.
        let mut p = UppFrameParser;
        match p.parse(b"\r") {
            FrameStatus::Complete(n) => assert_eq!(n, 1),
            other => panic!("expected Complete(1), got {other:?}"),
        }
    }

    #[test]
    fn longest_legit_answer_completes() {
        // Manual: device type `na` returns 16 ASCII chars. With CR,
        // the frame is 17 bytes. Well under MAX_RESPONSE_LEN.
        let mut buf = b"IGAR 6 Smart  ".to_vec();
        buf.push(CR);
        let mut p = UppFrameParser;
        match p.parse(&buf) {
            FrameStatus::Complete(n) => assert_eq!(n, buf.len()),
            other => panic!("expected Complete, got {other:?}"),
        }
    }

    #[test]
    fn runaway_response_eventually_invalid() {
        // No CR for MAX_RESPONSE_LEN bytes — we must return Invalid
        // rather than asking forever. This protects the transport
        // from an infinite read loop on a malfunctioning device.
        let buf = vec![b'A'; MAX_RESPONSE_LEN];
        let mut p = UppFrameParser;
        match p.parse(&buf) {
            FrameStatus::Invalid(msg) => {
                assert!(msg.contains(&MAX_RESPONSE_LEN.to_string()))
            }
            other => panic!("expected Invalid, got {other:?}"),
        }
    }

    #[test]
    fn need_strictly_increases_each_call() {
        // The FrameParser contract says: `min_total` MUST be
        // strictly greater than the current buffer length, otherwise
        // the transport loops forever. Sweep a range of buffer
        // lengths to enforce the invariant.
        let mut p = UppFrameParser;
        for len in 0..MAX_RESPONSE_LEN - 1 {
            let buf = vec![b'A'; len];
            match p.parse(&buf) {
                FrameStatus::Need(n) => assert!(
                    n > len,
                    "Need({n}) must be strictly > current length {len}"
                ),
                other => panic!("expected Need at len={len}, got {other:?}"),
            }
        }
    }

    #[test]
    fn cr_inside_buffer_terminates_at_first_occurrence() {
        // If a buffer (e.g. one with stale data left over from a
        // previous transaction) somehow contains two CRs, the
        // parser must terminate at the first one — that's the end
        // of THIS response. The transport drains the input buffer
        // before send(), so this should never happen in practice,
        // but the contract is what it is.
        let mut p = UppFrameParser;
        match p.parse(b"ok\r99em\r") {
            FrameStatus::Complete(n) => assert_eq!(n, 3),
            other => panic!("expected Complete(3), got {other:?}"),
        }
    }
}
