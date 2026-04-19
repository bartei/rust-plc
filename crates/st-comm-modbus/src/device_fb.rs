//! ModbusRtuDevice native function block.
//!
//! A generic Modbus RTU device parameterized by a YAML device profile.
//! The profile defines which registers to read/write; this FB handles
//! the protocol transactions.

use crate::rtu_client::RtuClient;
use st_comm_api::native_fb::*;
use st_comm_api::profile::{DeviceProfile, FieldDirection, RegisterKind};
use st_comm_api::FieldDataType;
use st_comm_serial::transport::{ParityMode, SerialConfig, SerialTransport};
use st_comm_serial::TransportMap;
use st_ir::Value;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Fixed field slots before profile fields in the layout.
const SLOT_PORT: usize = 0;
const SLOT_BAUD: usize = 1;
const SLOT_PARITY: usize = 2;
const SLOT_DATA_BITS: usize = 3;
const SLOT_STOP_BITS: usize = 4;
const SLOT_SLAVE_ID: usize = 5;
const SLOT_REFRESH_RATE: usize = 6;
const SLOT_CONNECTED: usize = 7;
const SLOT_ERROR_CODE: usize = 8;
const SLOT_IO_CYCLES: usize = 9;
const SLOT_LAST_RESPONSE_MS: usize = 10;
const PROFILE_FIELD_OFFSET: usize = 11;

/// A Modbus RTU device native FB.
///
/// Uses the shared transport map to find (or create) a serial transport
/// for the configured port. Multiple devices on the same port share the
/// transport automatically.
pub struct ModbusRtuDeviceNativeFb {
    layout: NativeFbLayout,
    profile: DeviceProfile,
    transport_map: Arc<TransportMap>,
    last_io: Mutex<Option<Instant>>,
    cached_transport: Mutex<Option<Arc<Mutex<SerialTransport>>>>,
}

impl ModbusRtuDeviceNativeFb {
    /// Create from a device profile and a shared transport map.
    pub fn new(profile: DeviceProfile, transport_map: Arc<TransportMap>) -> Self {
        let layout = build_modbus_layout(&profile);
        Self {
            layout,
            profile,
            transport_map,
            last_io: Mutex::new(None),
            cached_transport: Mutex::new(None),
        }
    }
}

impl NativeFb for ModbusRtuDeviceNativeFb {
    fn type_name(&self) -> &str {
        &self.layout.type_name
    }

    fn layout(&self) -> &NativeFbLayout {
        &self.layout
    }

    fn execute(&self, fields: &mut [Value]) {
        let port = match &fields[SLOT_PORT] {
            Value::String(s) if !s.is_empty() => s.clone(),
            _ => {
                fields[SLOT_CONNECTED] = Value::Bool(false);
                fields[SLOT_ERROR_CODE] = Value::Int(1); // No port configured
                return;
            }
        };
        let slave_id = fields[SLOT_SLAVE_ID].as_int() as u8;
        let refresh_ms = fields[SLOT_REFRESH_RATE].as_int();

        if slave_id == 0 {
            fields[SLOT_CONNECTED] = Value::Bool(false);
            fields[SLOT_ERROR_CODE] = Value::Int(2); // No slave configured
            return;
        }

        // Check refresh rate timing
        if refresh_ms > 0 {
            let last_io = self.last_io.lock().unwrap();
            if let Some(last) = *last_io {
                if last.elapsed() < Duration::from_millis(refresh_ms as u64) {
                    return; // Not yet time for I/O
                }
            }
        }

        // Get or create the serial transport for this port
        let transport = self.get_or_create_transport(&port, fields);
        let Some(transport) = transport else {
            return; // Error already set in fields
        };

        let client = RtuClient::new(transport);
        let start = Instant::now();
        let mut had_error = false;

        // --- Read input-direction fields ---
        for (i, pf) in self.profile.fields.iter().enumerate() {
            if !matches!(pf.direction, FieldDirection::Input | FieldDirection::Inout) {
                continue;
            }
            let slot = PROFILE_FIELD_OFFSET + i;
            if slot >= fields.len() {
                break;
            }

            let result = match pf.register.kind {
                RegisterKind::Coil => {
                    client.read_coils(slave_id, pf.register.address as u16, 1)
                        .map(|v| Value::Bool(v.first().copied().unwrap_or(false)))
                }
                RegisterKind::DiscreteInput => {
                    client.read_discrete_inputs(slave_id, pf.register.address as u16, 1)
                        .map(|v| Value::Bool(v.first().copied().unwrap_or(false)))
                }
                RegisterKind::HoldingRegister => {
                    client.read_holding_registers(slave_id, pf.register.address as u16, 1)
                        .map(|v| register_to_value(v.first().copied().unwrap_or(0), pf.data_type, &pf.register))
                }
                RegisterKind::InputRegister => {
                    client.read_input_registers(slave_id, pf.register.address as u16, 1)
                        .map(|v| register_to_value(v.first().copied().unwrap_or(0), pf.data_type, &pf.register))
                }
                RegisterKind::Virtual => Ok(fields[slot].clone()),
            };

            match result {
                Ok(val) => fields[slot] = val,
                Err(e) => {
                    tracing::debug!("Modbus read {}.{}: {e}", self.profile.name, pf.name);
                    had_error = true;
                }
            }
        }

        // --- Write output-direction fields ---
        for (i, pf) in self.profile.fields.iter().enumerate() {
            if !matches!(pf.direction, FieldDirection::Output | FieldDirection::Inout) {
                continue;
            }
            let slot = PROFILE_FIELD_OFFSET + i;
            if slot >= fields.len() {
                break;
            }

            let result = match pf.register.kind {
                RegisterKind::Coil => {
                    let val = fields[slot].as_bool();
                    client.write_single_coil(slave_id, pf.register.address as u16, val)
                }
                RegisterKind::HoldingRegister => {
                    let raw = value_to_register(&fields[slot], pf.data_type, &pf.register);
                    client.write_single_register(slave_id, pf.register.address as u16, raw)
                }
                _ => Ok(()),
            };

            if let Err(e) = result {
                tracing::debug!("Modbus write {}.{}: {e}", self.profile.name, pf.name);
                had_error = true;
            }
        }

        // Update diagnostics
        let elapsed = start.elapsed();
        fields[SLOT_CONNECTED] = Value::Bool(!had_error);
        fields[SLOT_ERROR_CODE] = Value::Int(if had_error { 10 } else { 0 });
        let cycles = fields[SLOT_IO_CYCLES].as_int() as u64 + 1;
        fields[SLOT_IO_CYCLES] = Value::UInt(cycles);
        fields[SLOT_LAST_RESPONSE_MS] = Value::Real(elapsed.as_secs_f64() * 1000.0);

        *self.last_io.lock().unwrap() = Some(Instant::now());
    }
}

impl ModbusRtuDeviceNativeFb {
    /// Get the serial transport for the given port path.
    /// First checks the cache, then the shared map, then creates a new one.
    fn get_or_create_transport(
        &self,
        port: &str,
        fields: &mut [Value],
    ) -> Option<Arc<Mutex<SerialTransport>>> {
        // Check cache
        {
            let cached = self.cached_transport.lock().unwrap();
            if let Some(ref t) = *cached {
                return Some(Arc::clone(t));
            }
        }

        // Check shared map (a SerialLink FB may have already opened this port)
        if let Ok(map) = self.transport_map.lock() {
            if let Some(t) = map.get(port) {
                *self.cached_transport.lock().unwrap() = Some(Arc::clone(t));
                return Some(Arc::clone(t));
            }
        }

        // No existing transport — create one from the device's port params
        let baud = fields[SLOT_BAUD].as_int() as u32;
        let parity_str = match &fields[SLOT_PARITY] {
            Value::String(s) => s.clone(),
            _ => "N".to_string(),
        };
        let data_bits = fields[SLOT_DATA_BITS].as_int() as u8;
        let stop_bits = fields[SLOT_STOP_BITS].as_int() as u8;

        let config = SerialConfig {
            port: port.to_string(),
            baud_rate: if baud > 0 { baud } else { 9600 },
            parity: ParityMode::parse(&parity_str),
            data_bits: if data_bits == 7 { 7 } else { 8 },
            stop_bits: if stop_bits == 2 { 2 } else { 1 },
            timeout: Duration::from_millis(100),
        };

        let mut transport = SerialTransport::new(config);
        match transport.open() {
            Ok(()) => {
                let arc = Arc::new(Mutex::new(transport));
                // Register in shared map for other devices on same port
                if let Ok(mut map) = self.transport_map.lock() {
                    map.insert(port.to_string(), Arc::clone(&arc));
                }
                *self.cached_transport.lock().unwrap() = Some(Arc::clone(&arc));
                Some(arc)
            }
            Err(e) => {
                tracing::warn!("ModbusRtuDevice: failed to open {port}: {e}");
                fields[SLOT_CONNECTED] = Value::Bool(false);
                fields[SLOT_ERROR_CODE] = Value::Int(3); // Transport open failed
                None
            }
        }
    }
}

/// Build the NativeFbLayout for a Modbus RTU device from a profile.
fn build_modbus_layout(profile: &DeviceProfile) -> NativeFbLayout {
    let mut fields = vec![
        // Serial port configuration (same as SerialLink for self-contained usage)
        NativeFbField { name: "port".into(), data_type: FieldDataType::String, var_kind: NativeFbVarKind::VarInput },
        NativeFbField { name: "baud".into(), data_type: FieldDataType::Int, var_kind: NativeFbVarKind::VarInput },
        NativeFbField { name: "parity".into(), data_type: FieldDataType::String, var_kind: NativeFbVarKind::VarInput },
        NativeFbField { name: "data_bits".into(), data_type: FieldDataType::Int, var_kind: NativeFbVarKind::VarInput },
        NativeFbField { name: "stop_bits".into(), data_type: FieldDataType::Int, var_kind: NativeFbVarKind::VarInput },
        // Modbus parameters
        NativeFbField { name: "slave_id".into(), data_type: FieldDataType::Int, var_kind: NativeFbVarKind::VarInput },
        NativeFbField { name: "refresh_rate".into(), data_type: FieldDataType::Time, var_kind: NativeFbVarKind::VarInput },
        // Diagnostics
        NativeFbField { name: "connected".into(), data_type: FieldDataType::Bool, var_kind: NativeFbVarKind::Var },
        NativeFbField { name: "error_code".into(), data_type: FieldDataType::Int, var_kind: NativeFbVarKind::Var },
        NativeFbField { name: "io_cycles".into(), data_type: FieldDataType::Udint, var_kind: NativeFbVarKind::Var },
        NativeFbField { name: "last_response_ms".into(), data_type: FieldDataType::Real, var_kind: NativeFbVarKind::Var },
    ];

    for pf in &profile.fields {
        fields.push(NativeFbField {
            name: pf.name.clone(),
            data_type: pf.data_type,
            var_kind: NativeFbVarKind::Var,
        });
    }

    NativeFbLayout {
        type_name: profile.name.clone(),
        fields,
    }
}

/// Convert a raw register value to an `st_ir::Value`, applying scaling.
fn register_to_value(
    raw: u16,
    data_type: FieldDataType,
    reg: &st_comm_api::profile::RegisterMapping,
) -> Value {
    let scaled = if let Some(scale) = reg.scale {
        raw as f64 * scale + reg.offset.unwrap_or(0.0)
    } else {
        raw as f64 + reg.offset.unwrap_or(0.0)
    };

    match data_type {
        FieldDataType::Bool => Value::Bool(raw != 0),
        FieldDataType::Real | FieldDataType::Lreal => Value::Real(scaled),
        FieldDataType::Usint | FieldDataType::Uint | FieldDataType::Udint | FieldDataType::Ulint
        | FieldDataType::Byte | FieldDataType::Word | FieldDataType::Dword | FieldDataType::Lword => {
            Value::UInt(scaled as u64)
        }
        _ => Value::Int(scaled as i64),
    }
}

/// Convert an `st_ir::Value` to a raw register value, applying inverse scaling.
fn value_to_register(
    value: &Value,
    _data_type: FieldDataType,
    reg: &st_comm_api::profile::RegisterMapping,
) -> u16 {
    let raw_f64 = match value {
        Value::Bool(b) => if *b { 1.0 } else { 0.0 },
        Value::Int(i) => *i as f64,
        Value::UInt(u) => *u as f64,
        Value::Real(r) => *r,
        _ => 0.0,
    };

    let unscaled = if let Some(scale) = reg.scale {
        if scale != 0.0 {
            (raw_f64 - reg.offset.unwrap_or(0.0)) / scale
        } else {
            raw_f64
        }
    } else {
        raw_f64 - reg.offset.unwrap_or(0.0)
    };

    unscaled.clamp(0.0, 65535.0) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_to_value_int() {
        let reg = st_comm_api::profile::RegisterMapping {
            address: 0, kind: RegisterKind::InputRegister, bit: None,
            scale: None, offset: None, unit: None,
            byte_order: st_comm_api::profile::ByteOrder::BigEndian, word_count: 1,
        };
        assert_eq!(register_to_value(42, FieldDataType::Int, &reg).as_int(), 42);
    }

    #[test]
    fn register_to_value_with_scaling() {
        let reg = st_comm_api::profile::RegisterMapping {
            address: 0, kind: RegisterKind::InputRegister, bit: None,
            scale: Some(0.1), offset: None, unit: None,
            byte_order: st_comm_api::profile::ByteOrder::BigEndian, word_count: 1,
        };
        assert!((register_to_value(450, FieldDataType::Real, &reg).as_real() - 45.0).abs() < 0.01);
    }

    #[test]
    fn value_to_register_inverse_scaling() {
        let reg = st_comm_api::profile::RegisterMapping {
            address: 0, kind: RegisterKind::HoldingRegister, bit: None,
            scale: Some(0.1), offset: None, unit: None,
            byte_order: st_comm_api::profile::ByteOrder::BigEndian, word_count: 1,
        };
        assert_eq!(value_to_register(&Value::Real(45.0), FieldDataType::Real, &reg), 450);
    }

    #[test]
    fn modbus_layout_from_profile() {
        let profile = DeviceProfile::from_yaml(r#"
name: TestModbus
protocol: modbus-rtu
fields:
  - { name: DI_0, type: BOOL, direction: input, register: { address: 0, kind: discrete_input } }
  - { name: AO_0, type: INT, direction: output, register: { address: 0, kind: holding_register } }
"#).unwrap();

        let layout = build_modbus_layout(&profile);
        assert_eq!(layout.type_name, "TestModbus");
        // 11 fixed + 2 profile fields
        assert_eq!(layout.fields.len(), 13);
        assert_eq!(layout.fields[SLOT_PORT].name, "port");
        assert_eq!(layout.fields[SLOT_SLAVE_ID].name, "slave_id");
        assert_eq!(layout.fields[PROFILE_FIELD_OFFSET].name, "DI_0");
        assert_eq!(layout.fields[PROFILE_FIELD_OFFSET + 1].name, "AO_0");
    }
}
