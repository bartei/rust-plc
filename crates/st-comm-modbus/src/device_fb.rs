//! ModbusRtuDevice native function block.
//!
//! A generic Modbus RTU device parameterized by a YAML device profile.
//! The profile defines which registers to read/write; this FB handles
//! the protocol transactions.

use crate::rtu_client::RtuClient;
use st_comm_api::native_fb::*;
use st_comm_api::profile::{DeviceProfile, FieldDirection, RegisterKind};
use st_comm_api::FieldDataType;
use st_comm_serial::transport::SerialTransport;
use st_ir::Value;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Fixed field slots before profile fields in the layout.
/// [0] link       : INT (VarInput — SerialLink handle, unused for now)
/// [1] slave_id   : INT (VarInput)
/// [2] refresh_rate : TIME (VarInput)
/// [3] connected  : BOOL (Var)
/// [4] error_code : INT (Var)
/// [5] io_cycles  : UDINT (Var)
/// [6] last_response_ms : REAL (Var)
/// [7..] profile fields
const _SLOT_LINK: usize = 0;
const SLOT_SLAVE_ID: usize = 1;
const SLOT_REFRESH_RATE: usize = 2;
const SLOT_CONNECTED: usize = 3;
const SLOT_ERROR_CODE: usize = 4;
const SLOT_IO_CYCLES: usize = 5;
const SLOT_LAST_RESPONSE_MS: usize = 6;
const PROFILE_FIELD_OFFSET: usize = 7;

/// A Modbus RTU device native FB.
///
/// Uses a shared `SerialTransport` (from a `SerialLink` FB) to communicate
/// with a Modbus slave. The device profile defines the register map.
pub struct ModbusRtuDeviceNativeFb {
    layout: NativeFbLayout,
    profile: DeviceProfile,
    transport: Arc<Mutex<SerialTransport>>,
    last_io: Mutex<Option<Instant>>,
}

impl ModbusRtuDeviceNativeFb {
    /// Create from a device profile and a shared serial transport.
    pub fn new(profile: DeviceProfile, transport: Arc<Mutex<SerialTransport>>) -> Self {
        let layout = build_modbus_layout(&profile);
        Self {
            layout,
            profile,
            transport,
            last_io: Mutex::new(None),
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
        let slave_id = fields[SLOT_SLAVE_ID].as_int() as u8;
        let refresh_ms = fields[SLOT_REFRESH_RATE].as_int();

        // Check refresh rate timing
        if refresh_ms > 0 {
            let last_io = self.last_io.lock().unwrap();
            if let Some(last) = *last_io {
                if last.elapsed() < Duration::from_millis(refresh_ms as u64) {
                    return; // Not yet time for I/O
                }
            }
        }

        if slave_id == 0 {
            fields[SLOT_CONNECTED] = Value::Bool(false);
            fields[SLOT_ERROR_CODE] = Value::Int(1); // No slave configured
            return;
        }

        let client = RtuClient::new(Arc::clone(&self.transport));
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
                RegisterKind::Virtual => Ok(fields[slot].clone()), // no-op for virtual
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
                _ => Ok(()), // Can't write to input registers or virtual
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

/// Build the NativeFbLayout for a Modbus RTU device from a profile.
fn build_modbus_layout(profile: &DeviceProfile) -> NativeFbLayout {
    let mut fields = vec![
        NativeFbField {
            name: "link".to_string(),
            data_type: FieldDataType::Int,
            var_kind: NativeFbVarKind::VarInput,
        },
        NativeFbField {
            name: "slave_id".to_string(),
            data_type: FieldDataType::Int,
            var_kind: NativeFbVarKind::VarInput,
        },
        NativeFbField {
            name: "refresh_rate".to_string(),
            data_type: FieldDataType::Time,
            var_kind: NativeFbVarKind::VarInput,
        },
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
        NativeFbField {
            name: "io_cycles".to_string(),
            data_type: FieldDataType::Udint,
            var_kind: NativeFbVarKind::Var,
        },
        NativeFbField {
            name: "last_response_ms".to_string(),
            data_type: FieldDataType::Real,
            var_kind: NativeFbVarKind::Var,
        },
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

    // Inverse scaling: raw = (value - offset) / scale
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
            address: 0,
            kind: RegisterKind::InputRegister,
            bit: None,
            scale: None,
            offset: None,
            unit: None,
            byte_order: st_comm_api::profile::ByteOrder::BigEndian,
            word_count: 1,
        };
        let val = register_to_value(42, FieldDataType::Int, &reg);
        assert_eq!(val.as_int(), 42);
    }

    #[test]
    fn register_to_value_with_scaling() {
        let reg = st_comm_api::profile::RegisterMapping {
            address: 0,
            kind: RegisterKind::InputRegister,
            bit: None,
            scale: Some(0.1),
            offset: None,
            unit: None,
            byte_order: st_comm_api::profile::ByteOrder::BigEndian,
            word_count: 1,
        };
        let val = register_to_value(450, FieldDataType::Real, &reg);
        assert!((val.as_real() - 45.0).abs() < 0.01);
    }

    #[test]
    fn value_to_register_inverse_scaling() {
        let reg = st_comm_api::profile::RegisterMapping {
            address: 0,
            kind: RegisterKind::HoldingRegister,
            bit: None,
            scale: Some(0.1),
            offset: None,
            unit: None,
            byte_order: st_comm_api::profile::ByteOrder::BigEndian,
            word_count: 1,
        };
        let raw = value_to_register(&Value::Real(45.0), FieldDataType::Real, &reg);
        assert_eq!(raw, 450);
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
        // 7 fixed + 2 profile fields
        assert_eq!(layout.fields.len(), 9);
        assert_eq!(layout.fields[SLOT_SLAVE_ID].name, "slave_id");
        assert_eq!(layout.fields[PROFILE_FIELD_OFFSET].name, "DI_0");
        assert_eq!(layout.fields[PROFILE_FIELD_OFFSET + 1].name, "AO_0");
    }
}
