//! UPP client — drives a single transaction over a shared
//! [`SerialTransport`].
//!
//! ## What a transaction looks like
//!
//! ```text
//!   ┌──── master ──────────────────────────────────────────────┐
//!   │  0. (transport flushes input buffer + waits preamble)    │
//!   │  1. send `AAcc[param]\r`                                 │
//!   │  2. read until CR with hard 5 ms deadline (skipped on    │
//!   │     broadcast 99 because the device is silent)           │
//!   │  3. sleep 1.5 ms cooldown (manual §7 "Additional         │
//!   │     instruction for the RS485 interface", item 4)        │
//!   │  4. strip address prefix + CR, hand payload to caller    │
//!   └──────────────────────────────────────────────────────────┘
//! ```
//!
//! Steps 0–2 are handled by
//! [`SerialTransport::transaction_framed`](st_comm_serial::SerialTransport::transaction_framed)
//! plus [`UppFrameParser`](crate::UppFrameParser). Steps 3–4 are this
//! crate's responsibility.
//!
//! ## Why broadcast `99` skips the read
//!
//! Manual §4.14: "98 = global address with response, 99 = global
//! address without response (settings only)". Issuing a `99…` request
//! and then waiting 5 ms for a response that the device is contractually
//! never going to send would cost a guaranteed timeout per write. The
//! client detects the address up front and returns
//! [`UppResponse::NoResponse`] immediately after the send completes.

use crate::address::Address;
use crate::command::{Command, CR};
use crate::error::UppError;
use crate::frame_parser::{UppFrameParser, MAX_RESPONSE_LEN};
use crate::parser::{DecodedValue, Decoder};
use st_comm_serial::transport::SerialTransport;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Default 5 ms response window — manual §7: "The pyrometer's
/// response will follow after 5 ms, at the latest". This is the
/// strict maximum the master should wait before declaring the
/// transaction failed.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_millis(5);

/// Default post-response cooldown — manual §7: "After receiving the
/// response, the master has to wait at least 1.5 ms before a new
/// command can be entered". We use 2 ms by default for a small safety
/// margin against scheduler jitter on busy hosts; users can dial it
/// down to the spec minimum via [`UppClient::with_timing`] when their
/// platform's timer resolution allows.
pub const DEFAULT_COOLDOWN: Duration = Duration::from_micros(2_000);

/// Default preamble (extra silence) before each transaction. The UPP
/// spec doesn't require any explicit preamble — the transport's
/// inherent inter-frame gap is enough — so this is zero by default.
pub const DEFAULT_PREAMBLE: Duration = Duration::ZERO;

/// What came back from the wire (after stripping the address prefix
/// and the `CR` terminator).
#[derive(Debug, Clone, PartialEq)]
pub enum UppResponse {
    /// Address that responded (echoed in every reply per manual §7),
    /// plus the payload bytes between the prefix and `CR`. The
    /// caller decides how to interpret the payload — usually by
    /// handing it to a [`Decoder`].
    Frame { address: Address, payload: Vec<u8> },
    /// Sent over a `99` broadcast — by spec the device does not
    /// transmit. Returned immediately after the master's send
    /// completes; no read attempted.
    NoResponse,
}

impl UppResponse {
    /// Borrow the payload bytes of a `Frame`. Returns
    /// [`UppError::BadResponse`] if this is a `NoResponse` (caller
    /// asked for content from a write-and-forget broadcast).
    pub fn payload(&self) -> Result<&[u8], UppError> {
        match self {
            UppResponse::Frame { payload, .. } => Ok(payload),
            UppResponse::NoResponse => Err(UppError::BadResponse(
                "broadcast-99 transactions return no payload".into(),
            )),
        }
    }

    /// Borrow the address prefix of a `Frame`.
    pub fn address(&self) -> Result<Address, UppError> {
        match self {
            UppResponse::Frame { address, .. } => Ok(*address),
            UppResponse::NoResponse => Err(UppError::BadResponse(
                "broadcast-99 transactions return no address".into(),
            )),
        }
    }
}

/// Round-trip diagnostics surfaced from each transaction. Wired into
/// the device FB's `last_response_ms` field by Phase 4.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransactionStats {
    /// Wall-clock duration from "send started" to "CR received"
    /// (or "send completed" for broadcast-99). Always > 0.
    pub round_trip: Duration,
}

/// A UPP client bound to one serial port. Transactions are
/// serialized through the shared `Arc<Mutex<SerialTransport>>`, so
/// multiple clients (e.g. one per device address on the same RS485
/// bus) can co-exist without stepping on each other's bytes.
pub struct UppClient {
    transport: Arc<Mutex<SerialTransport>>,
    timeout: Duration,
    cooldown: Duration,
    preamble: Duration,
}

impl UppClient {
    /// Build a client with the manual-default timing (5 ms timeout,
    /// 2 ms cooldown, no preamble).
    pub fn new(transport: Arc<Mutex<SerialTransport>>) -> Self {
        Self::with_timing(transport, DEFAULT_TIMEOUT, DEFAULT_COOLDOWN, DEFAULT_PREAMBLE)
    }

    /// Build a client with explicit timing. Use when a slow USB-RS485
    /// adapter or a longer cable run forces the response budget out
    /// past 5 ms (manual §3.1.2 warns about adapters that "are too
    /// slow for fast measuring equipment").
    pub fn with_timing(
        transport: Arc<Mutex<SerialTransport>>,
        timeout: Duration,
        cooldown: Duration,
        preamble: Duration,
    ) -> Self {
        Self { transport, timeout, cooldown, preamble }
    }

    /// Run one full UPP transaction and return the raw response.
    ///
    /// The address is encoded into the request, also matched against
    /// the response prefix, and the response payload (between prefix
    /// and CR) is returned untouched for the caller to decode.
    ///
    /// Errors classify per [`UppError`]:
    /// - [`Timeout`](UppError::Timeout) — no `CR` within `timeout`
    /// - [`AddressMismatch`](UppError::AddressMismatch) — frame
    ///   arrived but its prefix doesn't match the queried address
    ///   (cross-talk on a multi-drop bus, or a misconfigured device)
    /// - [`BadResponse`](UppError::BadResponse) — frame too short or
    ///   non-ASCII prefix
    /// - [`OutOfRange`](UppError::OutOfRange) — caller passed a
    ///   parameter the device would have rejected (caught client-side
    ///   to spare the bus a doomed write)
    /// - [`Transport`](UppError::Transport) — OS-level serial I/O
    ///   failure (port closed, USB unplugged, etc.)
    pub fn transaction(
        &self,
        addr: Address,
        cmd: &Command,
    ) -> Result<(UppResponse, TransactionStats), UppError> {
        let request = cmd.encode_request(addr)?;
        let started = Instant::now();

        let mut transport = self
            .transport
            .lock()
            .map_err(|e| UppError::Transport(format!("transport mutex poisoned: {e}")))?;

        if !addr.expects_response() {
            // Broadcast 99 — the device must not respond. Do a plain
            // send (still flushes input buffer for hygiene) and
            // bypass the read entirely.
            transport.clear_input_buffer()?;
            // The transport's send() enforces the protocol's mandatory
            // inter-frame gap, so we don't need to honour `preamble`
            // here separately for broadcast.
            transport.send(&request)?;
            drop(transport);
            // Same cooldown as a regular transaction so back-to-back
            // broadcast writes don't collide on the bus.
            sleep_at_least(self.cooldown);
            return Ok((
                UppResponse::NoResponse,
                TransactionStats { round_trip: started.elapsed() },
            ));
        }

        // Standard transaction: send, frame-parse the response, then
        // strip the address prefix.
        let mut response_buf = [0u8; MAX_RESPONSE_LEN + 1];
        let mut parser = UppFrameParser;
        let n = transport.transaction_framed(
            &request,
            &mut response_buf,
            &mut parser,
            self.timeout,
            self.preamble,
        )?;
        drop(transport);

        let frame = &response_buf[..n];
        let response = parse_response_frame(frame, addr)?;
        let stats = TransactionStats { round_trip: started.elapsed() };

        // Bus-cooldown — wait the spec minimum before the next
        // request can be sent. We do this AFTER the lock is released
        // so other threads can pipeline their own work, but BEFORE
        // returning to the caller so the caller does not need to
        // remember to space requests itself.
        sleep_at_least(self.cooldown);

        Ok((response, stats))
    }

    /// Convenience: run a transaction and decode the payload with
    /// the given [`Decoder`]. Most callers want this; the lower-level
    /// [`transaction`](Self::transaction) is for code paths that need
    /// access to the raw bytes (e.g. the diagnostic-frame logger).
    pub fn transact(
        &self,
        addr: Address,
        cmd: &Command,
        decoder: Decoder,
    ) -> Result<(DecodedValue, TransactionStats), UppError> {
        let (resp, stats) = self.transaction(addr, cmd)?;
        let payload = resp.payload()?;
        let stripped = strip_command_echo(payload, cmd);
        let decoded = decoder.decode(stripped)?;
        Ok((decoded, stats))
    }
}


/// Strip the address prefix from a complete UPP frame and validate
/// it matches what the request asked for. Returns the bytes between
/// the prefix and the trailing `CR` (excluding both).
fn parse_response_frame(frame: &[u8], expected: Address) -> Result<UppResponse, UppError> {
    // Frame must be at least "AA\r" — 2 prefix bytes + CR.
    if frame.len() < 3 {
        return Err(UppError::BadResponse(format!(
            "response frame {} byte(s), need at least 3 (AA + CR)",
            frame.len()
        )));
    }
    if *frame.last().unwrap() != CR {
        return Err(UppError::BadResponse(
            "response frame missing CR terminator".into(),
        ));
    }
    let address = Address::parse(&frame[0..2])?;

    // Manual §4.14: a global-98 request elicits a response from the
    // single device on the bus, but the response carries that
    // device's INDIVIDUAL address — not 98. So when we sent 98 we
    // accept any address-prefix as valid; for individual addresses
    // we require an exact match.
    if !addresses_match(expected, address) {
        return Err(UppError::AddressMismatch {
            expected: expected.as_u8(),
            got: format!("{address}"),
        });
    }

    let payload = frame[2..frame.len() - 1].to_vec();
    Ok(UppResponse::Frame { address, payload })
}

/// True if a response from `got` is a legitimate reply to a request
/// addressed to `expected`. Individual addresses must match exactly;
/// broadcast 98 accepts any individual address back; broadcast 99
/// produces no response and never reaches this check.
fn addresses_match(expected: Address, got: Address) -> bool {
    match expected {
        Address::Individual(_) => expected == got,
        Address::BroadcastWithResponse => matches!(got, Address::Individual(_)),
        Address::BroadcastNoResponse => false, // unreachable
    }
}

/// Some commands echo their own letters in the response (the manual
/// shows write replies as e.g. `00em0853`, with the `em` repeated
/// after the address). When that's the case, strip the 2-letter
/// echo so the payload handed to the [`Decoder`] is the same shape
/// the read counterpart would produce.
fn strip_command_echo<'a>(payload: &'a [u8], cmd: &Command) -> &'a [u8] {
    use crate::command::{Command::*, LimitsTarget};
    let mnem: &[u8; 2] = match cmd {
        // Writes that echo their own mnemonic per manual §7's
        // "Write Command" worked example.
        WriteEmissivity { .. } => b"em",
        WriteTransmittance { .. } => b"et",
        WriteEmissivityRatio { .. } => b"ev",
        WriteDirtyWindow { .. } => b"dw",
        WriteSwitchOff { .. } => b"aw",
        WriteResponseTime { .. } => b"ez",
        WriteClearPeak { .. } => b"lz",
        WriteFahrenheit { .. } => b"fh",
        WriteOpMode { .. } => b"ka",
        WriteLaser { .. } => b"la",
        WriteAnalogOutput { .. } => b"as",
        WriteDeviceAddress { .. } => b"ga",
        WriteSubRangeStep1 { .. } => b"m1",
        WriteBaudRate { .. } => b"br",
        // Limits queries echo the queried-parameter mnemonic on
        // their answer (e.g. `00em00501000`). Strip it so the
        // decoder sees just the 8-hex-digit pair.
        ReadLimits(t) => match t {
            LimitsTarget::Emissivity => b"em",
            LimitsTarget::Transmittance => b"et",
            LimitsTarget::EmissivityRatio => b"ev",
            LimitsTarget::DirtyWindow => b"dw",
            LimitsTarget::SwitchOff => b"aw",
            LimitsTarget::ResponseTime => b"ez",
            LimitsTarget::ClearPeak => b"lz",
            LimitsTarget::OpMode => b"ka",
            LimitsTarget::DeviceAddress => b"ga",
            LimitsTarget::BaudRate => b"br",
        },
        // Pure-write acks (`SimulateClearPeak`, `ConfirmSubRange`)
        // return `ok` / `no` and do NOT echo their mnemonic.
        // Reads return their value directly.
        _ => return payload,
    };
    if payload.len() >= 2 && &payload[0..2] == mnem {
        &payload[2..]
    } else {
        payload
    }
}

/// Sleep at least `d`. Spin-yield in the last 100 µs so a sub-ms
/// cooldown is honoured even on schedulers whose `sleep` resolution
/// is coarser than the requested interval.
fn sleep_at_least(d: Duration) {
    if d.is_zero() {
        return;
    }
    let deadline = Instant::now() + d;
    // OS-level sleep gets us close…
    if let Some(coarse) = d.checked_sub(Duration::from_micros(100)) {
        if !coarse.is_zero() {
            std::thread::sleep(coarse);
        }
    }
    // …then spin to the exact deadline so cooldowns of 1.5–2 ms hold
    // on platforms with 1 ms timer granularity.
    while Instant::now() < deadline {
        std::hint::spin_loop();
    }
}

// ── Tests that don't need real I/O ─────────────────────────────────
//
// Round-trip tests — encode a request, then synthesize the response
// shape the manual specifies and verify the parser pipeline (frame
// strip, address check, command-echo strip, decoder) hands back the
// expected typed value. The full real-transport coverage is in
// Phase 8 (socat + simulator) per the implementation plan.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::command::LimitsTarget;
    use crate::parser::Decoder;

    fn ind(n: u8) -> Address {
        Address::individual(n).unwrap()
    }

    #[test]
    fn parse_response_frame_strips_prefix_and_cr() {
        // Manual §7 example "Answer: '0970' + CR" for a read of `em`.
        // Our transport hands us the full frame including the CR.
        let frame = b"000970\r"; // address "00" + payload "0970" + CR
        let resp = parse_response_frame(frame, ind(0)).unwrap();
        match resp {
            UppResponse::Frame { address, payload } => {
                assert_eq!(address.as_u8(), 0);
                assert_eq!(payload, b"0970");
            }
            other => panic!("expected Frame, got {other:?}"),
        }
    }

    #[test]
    fn parse_response_frame_rejects_address_mismatch() {
        let frame = b"010970\r"; // says "from address 01"
        let err = parse_response_frame(frame, ind(0)).unwrap_err();
        match err {
            UppError::AddressMismatch { expected, got } => {
                assert_eq!(expected, 0);
                assert_eq!(got, "01");
            }
            other => panic!("expected AddressMismatch, got {other:?}"),
        }
    }

    #[test]
    fn parse_response_frame_rejects_missing_cr() {
        let err = parse_response_frame(b"000970", ind(0)).unwrap_err();
        assert!(matches!(err, UppError::BadResponse(_)));
    }

    #[test]
    fn parse_response_frame_rejects_too_short() {
        // "00" + CR alone is technically a valid frame for a
        // zero-payload write ack… but UPP write acks are at least
        // "ok" or "no" (2 bytes). Even so, parse_response_frame
        // accepts the empty payload and lets the decoder reject it.
        let resp = parse_response_frame(b"00\r", ind(0)).unwrap();
        assert_eq!(resp.payload().unwrap(), b"");
        // Anything shorter than 3 bytes IS a structural failure.
        assert!(matches!(
            parse_response_frame(b"00", ind(0)),
            Err(UppError::BadResponse(_))
        ));
        assert!(matches!(
            parse_response_frame(b"", ind(0)),
            Err(UppError::BadResponse(_))
        ));
    }

    #[test]
    fn broadcast_98_accepts_any_individual_address_back() {
        // Manual §4.14: 98 = "global address with response". The
        // single device on the bus replies with its own individual
        // address.
        let frame = b"420970\r";
        let resp =
            parse_response_frame(frame, Address::BroadcastWithResponse).unwrap();
        assert_eq!(resp.address().unwrap().as_u8(), 42);
    }

    #[test]
    fn broadcast_98_rejects_broadcast_address_back() {
        // The device should never echo back a broadcast address.
        let frame = b"980970\r";
        let err =
            parse_response_frame(frame, Address::BroadcastWithResponse).unwrap_err();
        assert!(matches!(err, UppError::AddressMismatch { .. }));
    }

    #[test]
    fn strip_command_echo_for_write_emissivity() {
        // Manual §7: "Answer: '00em0853' + CR" — our wire frame is
        // "00em0853\r"; after parse_response_frame the payload is
        // "em0853". Strip the 2-letter echo so the decoder sees
        // "0853".
        let stripped = strip_command_echo(b"em0853", &Command::WriteEmissivity { value: 853 });
        assert_eq!(stripped, b"0853");
    }

    #[test]
    fn strip_command_echo_for_limits_query() {
        // §7 "00em? answer could be 00501000 + CR" — the manual is
        // ambiguous on whether the echo is included. We treat any
        // leading "em" as an echo to be stripped; the bare hex pair
        // also still parses correctly.
        let stripped = strip_command_echo(
            b"em00501000",
            &Command::ReadLimits(LimitsTarget::Emissivity),
        );
        assert_eq!(stripped, b"00501000");
        // Bare-hex form (no echo): also passes through unchanged
        // because the leading two bytes aren't the mnemonic.
        let bare = strip_command_echo(
            b"00501000",
            &Command::ReadLimits(LimitsTarget::Emissivity),
        );
        assert_eq!(bare, b"00501000");
    }

    #[test]
    fn strip_command_echo_passthrough_for_reads() {
        // ReadEmissivity returns just the payload (e.g. "0970"), no
        // echo. The strip function must NOT touch it.
        let payload = b"0970";
        assert_eq!(
            strip_command_echo(payload, &Command::ReadEmissivity),
            payload,
        );
    }

    #[test]
    fn strip_command_echo_passthrough_for_acks() {
        // Pure-write commands return "ok" / "no" with no echo.
        assert_eq!(
            strip_command_echo(b"ok", &Command::SimulateClearPeak),
            b"ok",
        );
        assert_eq!(
            strip_command_echo(b"no", &Command::ConfirmSubRange),
            b"no",
        );
    }

    #[test]
    fn full_decode_pipeline_read_emissivity() {
        // End-to-end on the parsing side: synthesize the wire
        // response the manual gives for `00em` → `0970\r`, run it
        // through parse_response_frame + strip_echo + Decoder, and
        // check we get 0.970.
        let wire = b"000970\r";
        let resp = parse_response_frame(wire, ind(0)).unwrap();
        let payload = resp.payload().unwrap();
        let stripped = strip_command_echo(payload, &Command::ReadEmissivity);
        let v = Decoder::U16DecMilli.decode(stripped).unwrap();
        assert_eq!(v, DecodedValue::Per1000(0.970));
    }

    #[test]
    fn full_decode_pipeline_write_emissivity_echo() {
        // §7 worked write-example: master sends `00em0853\r`, device
        // replies `00em0853\r`. Our pipeline must yield 0.853.
        let wire = b"00em0853\r";
        let resp = parse_response_frame(wire, ind(0)).unwrap();
        let payload = resp.payload().unwrap();
        let stripped =
            strip_command_echo(payload, &Command::WriteEmissivity { value: 853 });
        let v = Decoder::U16DecMilli.decode(stripped).unwrap();
        assert_eq!(v, DecodedValue::Per1000(0.853));
    }

    #[test]
    fn sleep_at_least_does_not_undersleep() {
        // The cooldown enforcement must hit at least the requested
        // duration; on a busy CI runner it can OVER-sleep, never
        // under. Use a long-enough interval that scheduler jitter
        // can't flake the test.
        let want = Duration::from_millis(2);
        let t0 = Instant::now();
        sleep_at_least(want);
        assert!(t0.elapsed() >= want);
    }

    #[test]
    fn sleep_at_least_zero_is_noop() {
        let t0 = Instant::now();
        sleep_at_least(Duration::ZERO);
        // Should return essentially immediately. Allow generous
        // headroom for instrumentation.
        assert!(t0.elapsed() < Duration::from_millis(1));
    }
}
