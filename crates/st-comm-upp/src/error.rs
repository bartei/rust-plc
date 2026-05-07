//! Error type for the UPP protocol stack.
//!
//! Variants are coarse on purpose — UPP itself is a thin layer with
//! few failure modes, and the runtime's diagnostic field
//! (`error_code` on the device FB) is an [`i16`], so we keep the
//! variant set small enough to map 1:1 onto distinct integer codes.

use std::fmt;

/// All ways a UPP transaction can fail.
///
/// Every variant maps to a stable `i16` code via [`UppError::code`] so
/// runtime FB diagnostics ("error_code") can distinguish them without
/// exposing the rich enum to ST programs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UppError {
    /// No `CR` arrived within the configured response window
    /// (typically 5 ms — see manual §7 "Additional instruction for
    /// the RS485 interface", item 2). Per item 3 of the same section,
    /// this also covers parity / syntax errors at the UART level —
    /// from the master's point of view they look identical (no
    /// response).
    Timeout,

    /// A `CR`-terminated response arrived but its address prefix did
    /// not match the address that was queried. Indicates a bus
    /// arbitration race or a misconfigured device.
    AddressMismatch {
        expected: u8,
        got: String,
    },

    /// A `CR`-terminated response arrived but its body could not be
    /// decoded as the expected payload shape (wrong length,
    /// non-digit characters, value out of range, etc.).
    BadResponse(String),

    /// The caller passed a parameter outside the range advertised by
    /// the manual for the corresponding command (e.g. emissivity
    /// `> 1.000`). The device would reject this; we catch it
    /// client-side so the bus isn't wasted on a doomed write.
    OutOfRange(String),

    /// The caller invoked a command variant that this build does not
    /// implement yet. Used during incremental rollout — every
    /// shipped variant of [`Command`](crate::command::Command) MUST
    /// be reachable end-to-end before the next phase, so this
    /// variant should never appear on `master`.
    NotImplemented(&'static str),

    /// The underlying [`SerialTransport`](st_comm_serial::SerialTransport)
    /// reported an I/O failure (port closed, OS error, etc.).
    Transport(String),
}

impl UppError {
    /// Map to a stable diagnostic code suitable for the
    /// `error_code: INT` field on the runtime's device FB.
    /// Codes are 1-based; `0` is reserved for "no error" by the
    /// runtime layer.
    pub fn code(&self) -> i16 {
        match self {
            UppError::Timeout => 1,
            UppError::AddressMismatch { .. } => 2,
            UppError::BadResponse(_) => 3,
            UppError::OutOfRange(_) => 4,
            UppError::NotImplemented(_) => 5,
            UppError::Transport(_) => 6,
        }
    }
}

impl fmt::Display for UppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UppError::Timeout => write!(f, "UPP timeout (no CR within response window)"),
            UppError::AddressMismatch { expected, got } => {
                write!(f, "UPP address mismatch: expected {expected:02}, got {got:?}")
            }
            UppError::BadResponse(msg) => write!(f, "UPP bad response: {msg}"),
            UppError::OutOfRange(msg) => write!(f, "UPP out of range: {msg}"),
            UppError::NotImplemented(name) => write!(f, "UPP command not implemented: {name}"),
            UppError::Transport(msg) => write!(f, "UPP transport error: {msg}"),
        }
    }
}

impl std::error::Error for UppError {}

impl From<String> for UppError {
    /// Convenience: `SerialTransport` returns `Result<_, String>`, so
    /// `?` lifts those errors into [`UppError`] without boilerplate
    /// at every call site.
    ///
    /// We special-case the recognisable "Receive timeout" message
    /// produced by
    /// [`transaction_framed`](st_comm_serial::SerialTransport::transaction_framed)
    /// and surface it as [`UppError::Timeout`] — for UPP that's the
    /// most common and most actionable failure mode (manual §7
    /// item 3: "If there is no response, there is a parity or syntax
    /// error and the inquiry has to be repeated"). All other transport
    /// strings fall through as [`UppError::Transport`].
    fn from(msg: String) -> Self {
        if msg.contains("Receive timeout") {
            UppError::Timeout
        } else {
            UppError::Transport(msg)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codes_are_distinct() {
        let codes = [
            UppError::Timeout.code(),
            UppError::AddressMismatch { expected: 0, got: "xx".into() }.code(),
            UppError::BadResponse("x".into()).code(),
            UppError::OutOfRange("x".into()).code(),
            UppError::NotImplemented("x").code(),
            UppError::Transport("x".into()).code(),
        ];
        let mut sorted = codes.to_vec();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), codes.len(), "diagnostic codes must be unique");
        assert!(codes.iter().all(|c| *c > 0), "0 is reserved for no-error");
    }

    #[test]
    fn transport_lifting_is_implicit() {
        // Proves SerialTransport errors fold into UppError without
        // explicit conversion at call sites.
        fn inner() -> Result<(), UppError> {
            Err::<(), String>("port closed".into())?;
            Ok(())
        }
        let err = inner().unwrap_err();
        assert!(matches!(err, UppError::Transport(_)));
        assert_eq!(err.code(), 6);
    }

    #[test]
    fn lift_classifies_receive_timeout_as_timeout() {
        // The transport's transaction_framed emits exactly this
        // prefix on a deadline expiry. Pin that we surface it as
        // Timeout (code 1) rather than burying it in Transport
        // (code 6) — the runtime FB layer wants distinct codes for
        // "no response" vs "port disappeared".
        let err: UppError =
            String::from("Receive timeout: got 0 of expected 3+ bytes within 5ms").into();
        assert_eq!(err, UppError::Timeout);
        assert_eq!(err.code(), 1);
    }
}
