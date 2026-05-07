//! UPP — Universal Pyrometer Protocol over RS485.
//!
//! Wire format used by Impac / LumaSense / Advanced Energy pyrometers
//! (e.g. **IGAR 6 Smart**). The protocol is ASCII over half-duplex
//! RS485 at 8E1, terminated by `CR` (0x0D), with no checksum. Error
//! detection relies on UART parity plus a hard 5 ms response window.
//!
//! ## Where this crate fits
//!
//! - **Transport** ([`st_comm_serial`]) opens the serial port,
//!   manages baud / parity / stop bits, and provides
//!   [`SerialTransport::transaction_framed`](st_comm_serial::SerialTransport::transaction_framed)
//!   so a [`FrameParser`](st_comm_serial::FrameParser) can decide when
//!   a response is complete.
//! - **Protocol** (this crate) encodes [`Command`](command::Command)
//!   requests, decodes responses via [`Decoder`](parser::Decoder), and
//!   uses [`UppFrameParser`](frame_parser::UppFrameParser) to detect
//!   the trailing `CR` so reads terminate the moment the wire-frame
//!   ends.
//! - **Native FB** (planned, Phase 4) bridges this protocol into the
//!   PLC scan cycle the same way `st-comm-modbus` bridges Modbus RTU.
//!
//! ## Spec source
//!
//! All wire-format details come from the *Impac IGAR 6 Smart Pyrometer
//! User Manual* (Advanced Energy 57010259-00A, November 2021),
//! §7 "Data format UPP (Universal Pyrometer Protocol)". Examples in
//! the source-tree tests reproduce the manual's worked examples
//! verbatim so the encoder/decoder is exercised against the
//! specification, not against itself.

pub mod address;
pub mod client;
pub mod command;
pub mod device_fb;
pub mod error;
pub mod frame_parser;
pub mod parser;
pub mod profile_binding;

pub use address::Address;
pub use client::{TransactionStats, UppClient, UppResponse};
pub use command::Command;
pub use device_fb::UppDeviceNativeFb;
pub use error::UppError;
pub use frame_parser::UppFrameParser;
pub use parser::{DecodedValue, Decoder};
