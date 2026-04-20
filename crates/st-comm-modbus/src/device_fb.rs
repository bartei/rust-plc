//! ModbusRtuDevice native function block.
//!
//! A generic Modbus RTU device parameterized by a YAML device profile.
//! The profile defines which registers to read/write; this FB handles
//! the protocol transactions.
//!
//! Follows the two-layer model: the device takes a `link` parameter
//! (serial port path from a SerialLink instance) instead of owning
//! serial config fields. Transport is managed by SerialLink; protocol
//! I/O runs on a shared background bus thread.

use crate::rtu_client::RtuClient;
use st_comm_api::native_fb::*;
use st_comm_api::profile::{DeviceProfile, FieldDirection, RegisterKind};
use st_comm_api::FieldDataType;
use st_comm_serial::transport::SerialTransport;
use st_comm_serial::bus::{BusDeviceIo, BusManager};
use st_ir::Value;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Fixed field slots before profile fields in the layout.
const SLOT_LINK: usize = 0;
const SLOT_SLAVE_ID: usize = 1;
const SLOT_REFRESH_RATE: usize = 2;
const SLOT_CONNECTED: usize = 3;
const SLOT_ERROR_CODE: usize = 4;
const SLOT_IO_CYCLES: usize = 5;
const SLOT_LAST_RESPONSE_MS: usize = 6;
const PROFILE_FIELD_OFFSET: usize = 7;

/// Shared state between the scan-cycle thread and the background I/O thread.
pub(crate) struct IoState {
    /// Latest values read from the device (indexed by profile field position).
    pub read_values: Vec<Value>,
    /// Values to write to the device (indexed by profile field position).
    pub write_values: Vec<Value>,
    /// Diagnostics
    pub connected: bool,
    pub error_code: i64,
    pub io_cycles: u64,
    pub last_response_ms: f64,
}

/// A Modbus RTU device native FB.
///
/// I/O is performed on a per-port background thread (managed by BusManager)
/// so it never blocks the PLC scan cycle. The `execute()` method copies
/// cached read values and queues write values.
pub struct ModbusRtuDeviceNativeFb {
    layout: NativeFbLayout,
    profile: DeviceProfile,
    bus_manager: Arc<BusManager>,
    io_state: Arc<Mutex<IoState>>,
    registered: Mutex<bool>,
}

impl ModbusRtuDeviceNativeFb {
    /// Create from a device profile and a shared bus manager.
    pub fn new(profile: DeviceProfile, bus_manager: Arc<BusManager>) -> Self {
        let layout = build_modbus_layout(&profile);
        let default_values: Vec<Value> = profile
            .fields
            .iter()
            .map(|pf| Value::default_for_type(st_comm_api::field_data_type_to_var_type(pf.data_type)))
            .collect();
        Self {
            layout,
            bus_manager,
            io_state: Arc::new(Mutex::new(IoState {
                read_values: default_values.clone(),
                write_values: default_values,
                connected: false,
                error_code: 0,
                io_cycles: 0,
                last_response_ms: 0.0,
            })),
            registered: Mutex::new(false),
            profile,
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
        let link = match &fields[SLOT_LINK] {
            Value::String(s) if !s.is_empty() => s.clone(),
            _ => {
                fields[SLOT_CONNECTED] = Value::Bool(false);
                fields[SLOT_ERROR_CODE] = Value::Int(1); // No link configured
                return;
            }
        };
        let slave_id = fields[SLOT_SLAVE_ID].as_int() as u8;
        if slave_id == 0 {
            fields[SLOT_CONNECTED] = Value::Bool(false);
            fields[SLOT_ERROR_CODE] = Value::Int(2); // No slave configured
            return;
        }

        // Register this device on the shared bus for its link's port
        {
            let mut registered = self.registered.lock().unwrap();
            if !*registered {
                let refresh_ms = fields[SLOT_REFRESH_RATE].as_int();
                let interval = Duration::from_millis(if refresh_ms > 0 { refresh_ms as u64 } else { 50 });

                let io = ModbusDeviceIo::new(
                    slave_id,
                    self.profile.clone(),
                    Arc::clone(&self.io_state),
                );
                self.bus_manager.register(&link, interval, Box::new(io));
                *registered = true;
            }
        }

        // Queue output values for the background thread to write
        {
            let mut state = self.io_state.lock().unwrap();
            for (i, pf) in self.profile.fields.iter().enumerate() {
                if matches!(pf.direction, FieldDirection::Output | FieldDirection::Inout) {
                    let slot = PROFILE_FIELD_OFFSET + i;
                    if slot < fields.len() {
                        state.write_values[i] = fields[slot].clone();
                    }
                }
            }
        }

        // Copy cached read values and diagnostics into the field slice
        {
            let state = self.io_state.lock().unwrap();
            for (i, pf) in self.profile.fields.iter().enumerate() {
                if matches!(pf.direction, FieldDirection::Input | FieldDirection::Inout) {
                    let slot = PROFILE_FIELD_OFFSET + i;
                    if slot < fields.len() {
                        fields[slot] = state.read_values[i].clone();
                    }
                }
            }
            fields[SLOT_CONNECTED] = Value::Bool(state.connected);
            fields[SLOT_ERROR_CODE] = Value::Int(state.error_code);
            fields[SLOT_IO_CYCLES] = Value::UInt(state.io_cycles);
            fields[SLOT_LAST_RESPONSE_MS] = Value::Real(state.last_response_ms);
        }
    }
}

// ── BusDeviceIo implementation for Modbus RTU ─────────────────────────

/// Protocol-specific I/O for a Modbus RTU device on a serial bus.
struct ModbusDeviceIo {
    slave_id: u8,
    profile: DeviceProfile,
    io_state: Arc<Mutex<IoState>>,
}

impl ModbusDeviceIo {
    fn new(slave_id: u8, profile: DeviceProfile, io_state: Arc<Mutex<IoState>>) -> Self {
        Self { slave_id, profile, io_state }
    }
}

impl BusDeviceIo for ModbusDeviceIo {
    fn poll(&mut self, transport: &Arc<Mutex<SerialTransport>>) -> bool {
        let client = RtuClient::new(Arc::clone(transport));
        let start = std::time::Instant::now();
        let mut had_error = false;

        // Snapshot write values
        let write_snapshot: Vec<Value> = {
            self.io_state.lock().unwrap().write_values.clone()
        };

        // Read inputs (batched)
        had_error |= read_batched_io(&client, self.slave_id, &self.profile, &self.io_state);

        // Write outputs (batched)
        had_error |= write_batched_io(&client, self.slave_id, &self.profile, &write_snapshot);

        // Update diagnostics
        {
            let elapsed = start.elapsed();
            let mut state = self.io_state.lock().unwrap();
            state.connected = !had_error;
            state.error_code = if had_error { 10 } else { 0 };
            state.io_cycles += 1;
            state.last_response_ms = elapsed.as_secs_f64() * 1000.0;
        }

        !had_error
    }
}

// ── Layout builder ────────────────────────────────────────────────────

/// Build the NativeFbLayout for a Modbus RTU device from a profile.
/// Delegates to `DeviceProfile::to_modbus_rtu_device_layout()` so the layout
/// definition is shared with the LSP (which doesn't depend on this crate).
fn build_modbus_layout(profile: &DeviceProfile) -> NativeFbLayout {
    profile.to_modbus_rtu_device_layout()
}

// ── Batched I/O helpers ───────────────────────────────────────────────

/// Read input-direction fields in batches, storing results into `io_state`.
fn read_batched_io(
    client: &RtuClient,
    slave_id: u8,
    profile: &DeviceProfile,
    io_state: &Arc<Mutex<IoState>>,
) -> bool {
    let mut had_error = false;

    let input_fields: Vec<(usize, &st_comm_api::profile::ProfileField)> = profile
        .fields
        .iter()
        .enumerate()
        .filter(|(_, pf)| matches!(pf.direction, FieldDirection::Input | FieldDirection::Inout))
        .collect();

    let mut i = 0;
    while i < input_fields.len() {
        let (_, first_pf) = input_fields[i];

        if first_pf.register.kind == RegisterKind::Virtual {
            i += 1;
            continue;
        }

        // Find consecutive registers of the same kind
        let start_addr = first_pf.register.address as u16;
        let kind = first_pf.register.kind;
        let mut end = i + 1;
        while end < input_fields.len() {
            let (_, next_pf) = input_fields[end];
            let expected_addr = start_addr + (end - i) as u16;
            if next_pf.register.kind != kind || next_pf.register.address as u16 != expected_addr {
                break;
            }
            end += 1;
        }
        let count = (end - i) as u16;

        let result = match kind {
            RegisterKind::Coil => {
                client.read_coils(slave_id, start_addr, count).map(|bools| {
                    let mut state = io_state.lock().unwrap();
                    for (j, &val) in bools.iter().enumerate() {
                        let (idx, _) = input_fields[i + j];
                        state.read_values[idx] = Value::Bool(val);
                    }
                })
            }
            RegisterKind::DiscreteInput => {
                client.read_discrete_inputs(slave_id, start_addr, count).map(|bools| {
                    let mut state = io_state.lock().unwrap();
                    for (j, &val) in bools.iter().enumerate() {
                        let (idx, _) = input_fields[i + j];
                        state.read_values[idx] = Value::Bool(val);
                    }
                })
            }
            RegisterKind::HoldingRegister => {
                client.read_holding_registers(slave_id, start_addr, count).map(|regs| {
                    let mut state = io_state.lock().unwrap();
                    for (j, &raw) in regs.iter().enumerate() {
                        let (idx, pf) = input_fields[i + j];
                        state.read_values[idx] = register_to_value(raw, pf.data_type, &pf.register);
                    }
                })
            }
            RegisterKind::InputRegister => {
                client.read_input_registers(slave_id, start_addr, count).map(|regs| {
                    let mut state = io_state.lock().unwrap();
                    for (j, &raw) in regs.iter().enumerate() {
                        let (idx, pf) = input_fields[i + j];
                        state.read_values[idx] = register_to_value(raw, pf.data_type, &pf.register);
                    }
                })
            }
            RegisterKind::Virtual => Ok(()),
        };

        if let Err(e) = result {
            tracing::debug!("Modbus batch read {}.{}: {e}", profile.name, first_pf.name);
            had_error = true;
        }

        i = end;
    }
    had_error
}

/// Write output-direction fields in batches from a snapshot of write values.
fn write_batched_io(
    client: &RtuClient,
    slave_id: u8,
    profile: &DeviceProfile,
    write_values: &[Value],
) -> bool {
    let mut had_error = false;

    let output_fields: Vec<(usize, &st_comm_api::profile::ProfileField)> = profile
        .fields
        .iter()
        .enumerate()
        .filter(|(_, pf)| matches!(pf.direction, FieldDirection::Output | FieldDirection::Inout))
        .collect();

    let mut i = 0;
    while i < output_fields.len() {
        let (first_idx, first_pf) = output_fields[i];

        match first_pf.register.kind {
            RegisterKind::Coil => {
                let val = write_values[first_idx].as_bool();
                if let Err(e) = client.write_single_coil(slave_id, first_pf.register.address as u16, val) {
                    tracing::debug!("Modbus write {}.{}: {e}", profile.name, first_pf.name);
                    had_error = true;
                }
                i += 1;
            }
            RegisterKind::HoldingRegister => {
                let start_addr = first_pf.register.address as u16;
                let mut end = i + 1;
                while end < output_fields.len() {
                    let (_, next_pf) = output_fields[end];
                    let expected_addr = start_addr + (end - i) as u16;
                    if next_pf.register.kind != RegisterKind::HoldingRegister
                        || next_pf.register.address as u16 != expected_addr
                    {
                        break;
                    }
                    end += 1;
                }
                let count = end - i;
                if count == 1 {
                    let raw = value_to_register(&write_values[first_idx], first_pf.data_type, &first_pf.register);
                    if let Err(e) = client.write_single_register(slave_id, start_addr, raw) {
                        tracing::debug!("Modbus write {}.{}: {e}", profile.name, first_pf.name);
                        had_error = true;
                    }
                } else {
                    let values: Vec<u16> = (i..end)
                        .map(|j| {
                            let (idx, pf) = output_fields[j];
                            value_to_register(&write_values[idx], pf.data_type, &pf.register)
                        })
                        .collect();
                    if let Err(e) = client.write_multiple_registers(slave_id, start_addr, &values) {
                        tracing::debug!("Modbus batch write {}.{}: {e}", profile.name, first_pf.name);
                        had_error = true;
                    }
                }
                i = end;
            }
            _ => {
                i += 1;
            }
        }
    }
    had_error
}

// ── Value conversion helpers ──────────────────────────────────────────

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

// ── Tests ─────────────────────────────────────────────────────────────

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
        // 7 fixed + 2 profile fields
        assert_eq!(layout.fields.len(), 9);
        assert_eq!(layout.fields[SLOT_LINK].name, "link");
        assert_eq!(layout.fields[SLOT_SLAVE_ID].name, "slave_id");
        assert_eq!(layout.fields[PROFILE_FIELD_OFFSET].name, "DI_0");
        assert_eq!(layout.fields[PROFILE_FIELD_OFFSET + 1].name, "AO_0");
    }

    /// Verify that every SLOT_* constant matches the corresponding field name
    /// in the layout.
    #[test]
    fn slot_constants_match_layout_fields() {
        let profile = DeviceProfile::from_yaml(r#"
name: SlotCheck
protocol: modbus-rtu
fields:
  - { name: F0, type: INT, direction: input, register: { address: 0, kind: input_register } }
"#).unwrap();

        let layout = build_modbus_layout(&profile);

        assert_eq!(layout.fields[SLOT_LINK].name, "link");
        assert_eq!(layout.fields[SLOT_SLAVE_ID].name, "slave_id");
        assert_eq!(layout.fields[SLOT_REFRESH_RATE].name, "refresh_rate");
        assert_eq!(layout.fields[SLOT_CONNECTED].name, "connected");
        assert_eq!(layout.fields[SLOT_ERROR_CODE].name, "error_code");
        assert_eq!(layout.fields[SLOT_IO_CYCLES].name, "io_cycles");
        assert_eq!(layout.fields[SLOT_LAST_RESPONSE_MS].name, "last_response_ms");
        assert_eq!(PROFILE_FIELD_OFFSET, 7);
        assert_eq!(layout.fields[PROFILE_FIELD_OFFSET].name, "F0");
    }

    /// Verify that `to_modbus_rtu_device_layout()` (shared, used by LSP/DAP) produces
    /// the same layout as `build_modbus_layout()` (used by the runtime).
    #[test]
    fn shared_layout_matches_runtime_layout() {
        let profile = DeviceProfile::from_yaml(r#"
name: LayoutMatch
protocol: modbus-rtu
fields:
  - { name: AI_0, type: INT, direction: input, register: { address: 0, kind: input_register } }
  - { name: AI_1, type: INT, direction: input, register: { address: 1, kind: input_register } }
  - { name: AO_0, type: INT, direction: output, register: { address: 0, kind: holding_register } }
"#).unwrap();

        let runtime_layout = build_modbus_layout(&profile);
        let shared_layout = profile.to_modbus_rtu_device_layout();

        assert_eq!(runtime_layout.fields.len(), shared_layout.fields.len(),
            "Runtime and shared layouts must have same number of fields");

        for (i, (rt, sh)) in runtime_layout.fields.iter().zip(shared_layout.fields.iter()).enumerate() {
            assert_eq!(rt.name, sh.name,
                "Field name mismatch at slot {i}: runtime='{}', shared='{}'", rt.name, sh.name);
            assert_eq!(rt.data_type, sh.data_type,
                "Field type mismatch at slot {i} ('{}'): runtime={:?}, shared={:?}", rt.name, rt.data_type, sh.data_type);
            assert_eq!(rt.var_kind, sh.var_kind,
                "Field var_kind mismatch at slot {i} ('{}'): runtime={:?}, shared={:?}", rt.name, rt.var_kind, sh.var_kind);
        }
    }
}
