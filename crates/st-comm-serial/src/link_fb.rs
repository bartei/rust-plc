//! SerialLink native function block.
//!
//! Exposes a serial port as an ST function block:
//! ```st
//! serial(port := '/dev/ttyUSB0', baud := 9600, parity := 'N', data_bits := 8, stop_bits := 1);
//! ```

use crate::shared::TransportMap;
use crate::transport::{ParityMode, SerialConfig, SerialTransport};
use st_comm_api::native_fb::*;
use st_comm_api::FieldDataType;
use st_ir::Value;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Field slot indices in the NativeFbLayout (must match layout order).
const SLOT_PORT: usize = 0;
const SLOT_BAUD: usize = 1;
const SLOT_PARITY: usize = 2;
const SLOT_DATA_BITS: usize = 3;
const SLOT_STOP_BITS: usize = 4;
const SLOT_CONNECTED: usize = 5;
const SLOT_ERROR_CODE: usize = 6;

/// A serial link native FB that manages an RS-485/RS-232 serial port.
///
/// On first call, opens the port with the given parameters. On subsequent
/// calls, maintains the connection and updates diagnostic fields.
/// Device FBs (Modbus RTU, etc.) access the shared transport via
/// `transport_handle()`.
pub struct SerialLinkNativeFb {
    layout: NativeFbLayout,
    transport: Arc<Mutex<SerialTransport>>,
    /// Shared transport map — when a port is opened, its transport is
    /// registered here so device FBs can find it by port path.
    transport_map: Arc<TransportMap>,
    /// Whether the port has been opened (latched on first call).
    initialized: Mutex<bool>,
    /// The port path used to register in the transport map.
    port_path: Mutex<String>,
}

impl Default for SerialLinkNativeFb {
    fn default() -> Self {
        Self::new()
    }
}

impl SerialLinkNativeFb {
    /// Create with a shared transport map for link-device binding.
    pub fn with_transport_map(transport_map: Arc<TransportMap>) -> Self {
        let layout = NativeFbLayout {
            type_name: "SerialLink".to_string(),
            fields: vec![
                // VAR_INPUT: configuration parameters
                NativeFbField {
                    name: "port".to_string(),
                    data_type: FieldDataType::String,
                    var_kind: NativeFbVarKind::VarInput,
                },
                NativeFbField {
                    name: "baud".to_string(),
                    data_type: FieldDataType::Int,
                    var_kind: NativeFbVarKind::VarInput,
                },
                NativeFbField {
                    name: "parity".to_string(),
                    data_type: FieldDataType::String,
                    var_kind: NativeFbVarKind::VarInput,
                },
                NativeFbField {
                    name: "data_bits".to_string(),
                    data_type: FieldDataType::Int,
                    var_kind: NativeFbVarKind::VarInput,
                },
                NativeFbField {
                    name: "stop_bits".to_string(),
                    data_type: FieldDataType::Int,
                    var_kind: NativeFbVarKind::VarInput,
                },
                // VAR: diagnostics
                NativeFbField {
                    name: "connected".to_string(),
                    data_type: FieldDataType::Bool,
                    var_kind: NativeFbVarKind::Var,
                },
                NativeFbField {
                    name: "error_code".to_string(),
                    data_type: FieldDataType::Int,
                    var_kind: NativeFbVarKind::Var,
                },
            ],
        };

        Self {
            layout,
            transport: Arc::new(Mutex::new(SerialTransport::new(SerialConfig::default()))),
            transport_map,
            initialized: Mutex::new(false),
            port_path: Mutex::new(String::new()),
        }
    }

    /// Create with a new empty transport map (for standalone testing).
    pub fn new() -> Self {
        Self::with_transport_map(crate::new_transport_map())
    }

    /// Get a shared handle to the underlying serial transport.
    /// Device FBs (e.g., ModbusRtuDevice) use this to send/receive on the bus.
    pub fn transport_handle(&self) -> Arc<Mutex<SerialTransport>> {
        Arc::clone(&self.transport)
    }
}

impl NativeFb for SerialLinkNativeFb {
    fn type_name(&self) -> &str {
        &self.layout.type_name
    }

    fn layout(&self) -> &NativeFbLayout {
        &self.layout
    }

    fn execute(&self, fields: &mut [Value]) {
        let mut initialized = self.initialized.lock().unwrap();

        if !*initialized {
            // First call: extract config from VAR_INPUT fields and open the port
            let port = match &fields[SLOT_PORT] {
                Value::String(s) => s.clone(),
                _ => String::new(),
            };
            let baud = fields[SLOT_BAUD].as_int() as u32;
            let parity_str = match &fields[SLOT_PARITY] {
                Value::String(s) => s.clone(),
                _ => "N".to_string(),
            };
            let data_bits = fields[SLOT_DATA_BITS].as_int() as u8;
            let stop_bits = fields[SLOT_STOP_BITS].as_int() as u8;

            if port.is_empty() {
                fields[SLOT_CONNECTED] = Value::Bool(false);
                fields[SLOT_ERROR_CODE] = Value::Int(1); // No port configured
                return;
            }

            let config = SerialConfig {
                port: port.clone(),
                baud_rate: if baud > 0 { baud } else { 9600 },
                parity: ParityMode::parse(&parity_str),
                data_bits: if data_bits == 7 { 7 } else { 8 },
                stop_bits: if stop_bits == 2 { 2 } else { 1 },
                timeout: Duration::from_millis(100),
            };

            let mut transport = self.transport.lock().unwrap();
            *transport = SerialTransport::new(config);
            match transport.open() {
                Ok(()) => {
                    fields[SLOT_CONNECTED] = Value::Bool(true);
                    fields[SLOT_ERROR_CODE] = Value::Int(0);
                    *initialized = true;
                    // Register in the shared transport map so device FBs can find it
                    *self.port_path.lock().unwrap() = port.clone();
                    drop(transport); // release lock before map lock
                    if let Ok(mut map) = self.transport_map.lock() {
                        map.insert(port, Arc::clone(&self.transport));
                    }
                }
                Err(e) => {
                    tracing::warn!("SerialLink: failed to open port: {e}");
                    fields[SLOT_CONNECTED] = Value::Bool(false);
                    fields[SLOT_ERROR_CODE] = Value::Int(2); // Open failed
                }
            }
        } else {
            // Subsequent calls: verify port is still open, reconnect if needed
            let transport = self.transport.lock().unwrap();
            if transport.is_open() {
                fields[SLOT_CONNECTED] = Value::Bool(true);
                fields[SLOT_ERROR_CODE] = Value::Int(0);
            } else {
                // Port was lost — try to reopen
                drop(transport);
                let mut transport = self.transport.lock().unwrap();
                match transport.open() {
                    Ok(()) => {
                        fields[SLOT_CONNECTED] = Value::Bool(true);
                        fields[SLOT_ERROR_CODE] = Value::Int(0);
                    }
                    Err(_) => {
                        fields[SLOT_CONNECTED] = Value::Bool(false);
                        fields[SLOT_ERROR_CODE] = Value::Int(3); // Reconnect failed
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serial_link_layout() {
        let fb = SerialLinkNativeFb::new();
        let layout = fb.layout();
        assert_eq!(layout.type_name, "SerialLink");
        assert_eq!(layout.fields.len(), 7);
        assert_eq!(layout.fields[0].name, "port");
        assert_eq!(layout.fields[0].var_kind, NativeFbVarKind::VarInput);
        assert_eq!(layout.fields[5].name, "connected");
        assert_eq!(layout.fields[5].var_kind, NativeFbVarKind::Var);
    }

    #[test]
    fn serial_link_no_port_configured() {
        let fb = SerialLinkNativeFb::new();
        let mut fields = vec![
            Value::String(String::new()), // port (empty)
            Value::Int(9600),             // baud
            Value::String("N".into()),    // parity
            Value::Int(8),                // data_bits
            Value::Int(1),                // stop_bits
            Value::Bool(false),           // connected
            Value::Int(0),                // error_code
        ];
        fb.execute(&mut fields);
        assert_eq!(fields[SLOT_CONNECTED], Value::Bool(false));
        assert_eq!(fields[SLOT_ERROR_CODE], Value::Int(1)); // No port
    }

    #[test]
    fn serial_link_nonexistent_port() {
        let fb = SerialLinkNativeFb::new();
        let mut fields = vec![
            Value::String("/dev/ttyNONEXISTENT".into()),
            Value::Int(9600),
            Value::String("N".into()),
            Value::Int(8),
            Value::Int(1),
            Value::Bool(false),
            Value::Int(0),
        ];
        fb.execute(&mut fields);
        assert_eq!(fields[SLOT_CONNECTED], Value::Bool(false));
        assert_eq!(fields[SLOT_ERROR_CODE], Value::Int(2)); // Open failed
    }

    #[test]
    fn transport_handle_is_shared() {
        let fb = SerialLinkNativeFb::new();
        let h1 = fb.transport_handle();
        let h2 = fb.transport_handle();
        // Both point to the same transport
        assert!(Arc::ptr_eq(&h1, &h2));
    }
}
