//! RS-485/RS-232 serial link for PLC communication.
//!
//! Provides `SerialTransport` for low-level serial I/O and `SerialLinkNativeFb`
//! as a native function block that users call from ST code:
//!
//! ```st
//! VAR
//!     serial : SerialLink;
//! END_VAR
//!     serial(port := '/dev/ttyUSB0', baud := 9600, parity := 'N', data_bits := 8, stop_bits := 1);
//! ```

pub mod transport;
pub mod shared;
mod link_fb;

pub use transport::SerialTransport;
pub use shared::{TransportMap, new_transport_map};
pub use link_fb::SerialLinkNativeFb;
