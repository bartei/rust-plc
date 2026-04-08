//! Communication framework API for PLC I/O.
//!
//! Defines the traits, types, and device profile system used by all
//! communication extensions (simulated, Modbus, PROFINET, etc.).
//!
//! # Architecture
//!
//! - **Links** (Layer 1-2): physical transport channels (TCP, serial, simulated)
//! - **Devices** (Layer 7): addressable units on a link (Modbus slave, simulated I/O)
//! - **Profiles**: YAML files defining struct schemas + register maps for specific hardware
//!
//! # Usage
//!
//! 1. Define a device profile YAML (or use a bundled one)
//! 2. Configure links and devices in `plc-project.yaml`
//! 3. The framework auto-generates ST struct types and global instances
//! 4. ST code reads/writes device fields; the comm manager handles I/O

pub mod error;
pub mod link;
pub mod device;
pub mod profile;
pub mod codegen;
pub mod config;

pub use error::*;
pub use link::*;
pub use device::*;
pub use profile::*;
pub use codegen::*;
pub use config::*;
