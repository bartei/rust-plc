//! Simulated communication device with web UI.
//!
//! Implements `CommDevice` with in-memory register storage.
//! Provides a web UI (HTTP + WebSocket) for manually toggling inputs
//! and observing outputs — no physical hardware required.

pub mod device;
pub mod link;
pub mod web;

pub use device::SimulatedDevice;
pub use device::SimulatedNativeFb;
pub use link::SimulatedLink;
