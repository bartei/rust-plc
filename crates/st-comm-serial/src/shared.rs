//! Shared transport registry for link-device binding.
//!
//! SerialLink FBs register their transport handle here when they open a port.
//! Device FBs (Modbus RTU, etc.) look up the transport by port path.

use crate::transport::SerialTransport;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// Global registry of open serial transports, keyed by port path.
///
/// Shared between SerialLink and device FBs via `Arc<TransportMap>`.
/// SerialLink registers its transport after opening; device FBs look it up.
pub type TransportMap = Mutex<HashMap<String, Arc<Mutex<SerialTransport>>>>;

/// Create a new empty transport map.
pub fn new_transport_map() -> Arc<TransportMap> {
    Arc::new(Mutex::new(HashMap::new()))
}
