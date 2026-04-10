//! Error types for the communication framework.

use std::collections::HashMap;

/// Errors that can occur during communication operations.
#[derive(Debug, thiserror::Error)]
pub enum CommError {
    #[error("connection failed: {0}")]
    ConnectionFailed(String),

    #[error("connection lost: {0}")]
    ConnectionLost(String),

    #[error("timeout after {0}ms")]
    Timeout(u64),

    #[error("device not responding: unit {unit_id}")]
    DeviceNotResponding { unit_id: u16 },

    #[error("protocol error: {0}")]
    ProtocolError(String),

    #[error("invalid configuration: {0}")]
    InvalidConfig(String),

    #[error("profile error: {0}")]
    ProfileError(String),

    #[error("I/O error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("{0}")]
    Other(String),
}

/// Diagnostics for a communication link.
#[derive(Debug, Clone, Default)]
pub struct LinkDiagnostics {
    pub is_open: bool,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub errors: u64,
    pub last_error: Option<String>,
}

/// Diagnostics for a communication device.
#[derive(Debug, Clone, Default)]
pub struct DeviceDiagnostics {
    pub connected: bool,
    pub error: bool,
    pub error_count: u64,
    pub successful_cycles: u64,
    pub last_response_ms: u32,
    pub last_error: Option<String>,
}

/// Value type for I/O data exchange between comm devices and the VM.
#[derive(Debug, Clone, PartialEq)]
pub enum IoValue {
    Bool(bool),
    Int(i64),
    UInt(u64),
    Real(f64),
    String(String),
}

impl IoValue {
    pub fn as_bool(&self) -> bool {
        match self {
            IoValue::Bool(b) => *b,
            IoValue::Int(i) => *i != 0,
            IoValue::UInt(u) => *u != 0,
            IoValue::Real(r) => *r != 0.0,
            _ => false,
        }
    }

    pub fn as_int(&self) -> i64 {
        match self {
            IoValue::Int(i) => *i,
            IoValue::UInt(u) => *u as i64,
            IoValue::Bool(b) => *b as i64,
            IoValue::Real(r) => *r as i64,
            _ => 0,
        }
    }

    pub fn as_real(&self) -> f64 {
        match self {
            IoValue::Real(r) => *r,
            IoValue::Int(i) => *i as f64,
            IoValue::UInt(u) => *u as f64,
            _ => 0.0,
        }
    }
}

/// A set of named I/O values (field_name → value).
pub type IoValues = HashMap<String, IoValue>;

/// An acyclic (on-demand) request to a device.
#[derive(Debug, Clone)]
pub struct AcyclicRequest {
    pub operation: AcyclicOp,
    pub address: u32,
    pub count: u16,
    pub data: Option<Vec<u8>>,
}

/// Acyclic operation types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcyclicOp {
    Read,
    Write,
}

/// Response to an acyclic request.
#[derive(Debug, Clone)]
pub struct AcyclicResponse {
    pub success: bool,
    pub data: Vec<u8>,
    pub error: Option<String>,
}
