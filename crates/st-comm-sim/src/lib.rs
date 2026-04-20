//! Simulated communication device with web UI.
//!
//! Provides `SimulatedNativeFb` — a native function block backed by
//! in-memory register storage. Includes an HTTP/WebSocket UI for
//! manually toggling inputs and observing outputs.

pub mod device;
pub mod web;

pub use device::LayoutOnlyNativeFb;
pub use device::SimulatedDevice;
pub use device::SimulatedNativeFb;
