//! Modbus RTU protocol implementation for PLC communication.
//!
//! Provides frame building/parsing, CRC16, and `ModbusRtuDeviceNativeFb` —
//! a native function block that reads/writes Modbus registers according to
//! a YAML device profile.

pub mod crc;
pub mod frame;
pub mod rtu_client;
pub mod device_fb;
