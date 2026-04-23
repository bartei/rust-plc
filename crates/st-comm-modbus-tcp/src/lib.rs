//! Modbus TCP protocol implementation for PLC communication.
//!
//! Self-contained crate that handles both TCP transport and Modbus TCP/IP
//! protocol. Unlike Modbus RTU (which shares a serial bus among devices),
//! Modbus TCP is point-to-point — each device FB owns its own TCP connection
//! and background I/O thread.

pub mod transport;
pub mod frame;
pub mod client;
pub mod device_fb;
