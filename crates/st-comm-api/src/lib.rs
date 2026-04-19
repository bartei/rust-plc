//! Communication framework API for PLC I/O.
//!
//! Provides the NativeFb trait system, device profile YAML parsing,
//! and configuration types for the communication layer.
//!
//! # Architecture
//!
//! - **Native FBs**: Rust-backed function blocks for device communication
//! - **Profiles**: YAML files defining register maps for specific hardware
//! - **Registry**: Central collection of available native FB types

pub mod error;
pub mod profile;
pub mod config;
pub mod native_fb;

pub use error::*;
pub use profile::*;
pub use config::*;
pub use native_fb::*;
