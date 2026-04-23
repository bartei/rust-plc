//! ModbusTcpDevice native function block.
//!
//! A generic Modbus TCP device parameterized by a YAML device profile.
//! Unlike Modbus RTU (which shares a serial bus), each TCP device FB
//! owns its own TCP connection and background I/O thread.
//!
//! The device FB unifies the transport (TCP socket) and protocol (Modbus
//! TCP/IP) in one place — no separate TcpLink needed.

use crate::client::TcpModbusClient;
use crate::transport::{TcpConfig, TcpTransport};
use st_comm_api::native_fb::*;
use st_comm_api::profile::{DeviceProfile, FieldDirection, RegisterKind};
use st_comm_api::FieldDataType;
use st_ir::Value;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Fixed field slots before profile fields in the layout.
const SLOT_HOST: usize = 0;
const SLOT_PORT: usize = 1;
const SLOT_UNIT_ID: usize = 2;
const SLOT_REFRESH_RATE: usize = 3;
const SLOT_CONNECTED: usize = 4;
const SLOT_ERROR_CODE: usize = 5;
const SLOT_IO_CYCLES: usize = 6;
const SLOT_LAST_RESPONSE_MS: usize = 7;
const PROFILE_FIELD_OFFSET: usize = 8;

/// Shared state between the scan-cycle thread and the background I/O thread.
struct IoState {
    /// Latest values read from the device (indexed by profile field position).
    read_values: Vec<Value>,
    /// Values to write to the device (indexed by profile field position).
    write_values: Vec<Value>,
    /// Diagnostics
    connected: bool,
    error_code: i64,
    io_cycles: u64,
    last_response_ms: f64,
}

/// A Modbus TCP device native FB.
///
/// I/O is performed on a dedicated background thread (one per device)
/// so it never blocks the PLC scan cycle. The `execute()` method copies
/// cached read values and queues write values.
pub struct ModbusTcpDeviceNativeFb {
    layout: NativeFbLayout,
    profile: DeviceProfile,
    io_state: Arc<Mutex<IoState>>,
    started: Mutex<bool>,
}

impl ModbusTcpDeviceNativeFb {
    /// Create from a device profile.
    pub fn new(profile: DeviceProfile) -> Self {
        let layout = profile.to_modbus_tcp_device_layout();
        let default_values: Vec<Value> = profile
            .fields
            .iter()
            .map(|pf| {
                Value::default_for_type(st_comm_api::field_data_type_to_var_type(pf.data_type))
            })
            .collect();
        Self {
            layout,
            io_state: Arc::new(Mutex::new(IoState {
                read_values: default_values.clone(),
                write_values: default_values,
                connected: false,
                error_code: 0,
                io_cycles: 0,
                last_response_ms: 0.0,
            })),
            started: Mutex::new(false),
            profile,
        }
    }
}

impl NativeFb for ModbusTcpDeviceNativeFb {
    fn type_name(&self) -> &str {
        &self.layout.type_name
    }

    fn layout(&self) -> &NativeFbLayout {
        &self.layout
    }

    fn execute(&self, fields: &mut [Value]) {
        let host = match &fields[SLOT_HOST] {
            Value::String(s) if !s.is_empty() => s.clone(),
            _ => {
                fields[SLOT_CONNECTED] = Value::Bool(false);
                fields[SLOT_ERROR_CODE] = Value::Int(1); // No host configured
                return;
            }
        };
        let port = {
            let p = fields[SLOT_PORT].as_int();
            if p > 0 { p as u16 } else { 502 }
        };
        let unit_id = fields[SLOT_UNIT_ID].as_int() as u8;

        // Spawn the background I/O thread on first call
        {
            let mut started = self.started.lock().unwrap();
            if !*started {
                let refresh_ms = fields[SLOT_REFRESH_RATE].as_int();
                let interval =
                    Duration::from_millis(if refresh_ms > 0 { refresh_ms as u64 } else { 50 });

                let config = TcpConfig {
                    host: host.clone(),
                    port,
                    timeout: Duration::from_millis(500),
                    connect_timeout: Duration::from_secs(2),
                };
                let io_state = Arc::clone(&self.io_state);
                let profile = self.profile.clone();
                let thread_name = format!("modbus-tcp-{host}:{port}");

                std::thread::Builder::new()
                    .name(thread_name.clone())
                    .spawn(move || {
                        io_thread_loop(config, unit_id, interval, profile, io_state);
                    })
                    .expect("Failed to spawn Modbus TCP I/O thread");

                tracing::info!("Spawned Modbus TCP I/O thread: {thread_name}");
                *started = true;
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

// ── Background I/O thread ────────────────────────────────────────────

/// Main loop for a device I/O thread.
fn io_thread_loop(
    config: TcpConfig,
    unit_id: u8,
    interval: Duration,
    profile: DeviceProfile,
    io_state: Arc<Mutex<IoState>>,
) {
    let addr = format!("{}:{}", config.host, config.port);
    let mut transport = TcpTransport::new(config);

    // Initial connection attempt
    if let Err(e) = transport.connect() {
        tracing::warn!("Modbus TCP initial connect to {addr} failed: {e}");
        let mut state = io_state.lock().unwrap();
        state.connected = false;
        state.error_code = 2; // Connect failed
    }

    loop {
        let start = std::time::Instant::now();
        let mut had_error = false;

        // Ensure connected (reconnect if needed)
        if !transport.is_connected() {
            if let Err(e) = transport.connect() {
                tracing::debug!("Modbus TCP reconnect to {addr} failed: {e}");
                let mut state = io_state.lock().unwrap();
                state.connected = false;
                state.error_code = 2;
                std::thread::sleep(interval);
                continue;
            }
        }

        let mut client = TcpModbusClient::new(&mut transport);

        // Snapshot write values
        let write_snapshot: Vec<Value> = { io_state.lock().unwrap().write_values.clone() };

        // Read inputs (batched)
        had_error |= read_batched_io(&mut client, unit_id, &profile, &io_state);

        // Write outputs (batched)
        had_error |= write_batched_io(&mut client, unit_id, &profile, &write_snapshot);

        // Update diagnostics
        {
            let elapsed = start.elapsed();
            let mut state = io_state.lock().unwrap();
            state.connected = !had_error;
            state.error_code = if had_error { 10 } else { 0 };
            state.io_cycles += 1;
            state.last_response_ms = elapsed.as_secs_f64() * 1000.0;
        }

        // If we had errors, the transport might be dead
        if had_error {
            transport.disconnect();
        }

        // Sleep for the remaining interval time
        let elapsed = start.elapsed();
        if elapsed < interval {
            std::thread::sleep(interval - elapsed);
        }
    }
}

// ── Batched I/O helpers ──────────────────────────────────────────────

/// Read input-direction fields in batches, storing results into `io_state`.
fn read_batched_io(
    client: &mut TcpModbusClient,
    unit_id: u8,
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
            RegisterKind::Coil => client.read_coils(unit_id, start_addr, count).map(|bools| {
                let mut state = io_state.lock().unwrap();
                for (j, &val) in bools.iter().enumerate() {
                    let (idx, _) = input_fields[i + j];
                    state.read_values[idx] = Value::Bool(val);
                }
            }),
            RegisterKind::DiscreteInput => {
                client
                    .read_discrete_inputs(unit_id, start_addr, count)
                    .map(|bools| {
                        let mut state = io_state.lock().unwrap();
                        for (j, &val) in bools.iter().enumerate() {
                            let (idx, _) = input_fields[i + j];
                            state.read_values[idx] = Value::Bool(val);
                        }
                    })
            }
            RegisterKind::HoldingRegister => {
                client
                    .read_holding_registers(unit_id, start_addr, count)
                    .map(|regs| {
                        let mut state = io_state.lock().unwrap();
                        for (j, &raw) in regs.iter().enumerate() {
                            let (idx, pf) = input_fields[i + j];
                            state.read_values[idx] =
                                register_to_value(raw, pf.data_type, &pf.register);
                        }
                    })
            }
            RegisterKind::InputRegister => {
                client
                    .read_input_registers(unit_id, start_addr, count)
                    .map(|regs| {
                        let mut state = io_state.lock().unwrap();
                        for (j, &raw) in regs.iter().enumerate() {
                            let (idx, pf) = input_fields[i + j];
                            state.read_values[idx] =
                                register_to_value(raw, pf.data_type, &pf.register);
                        }
                    })
            }
            RegisterKind::Virtual => Ok(()),
        };

        if let Err(e) = result {
            tracing::debug!("Modbus TCP batch read {}.{}: {e}", profile.name, first_pf.name);
            had_error = true;
        }

        i = end;
    }
    had_error
}

/// Write output-direction fields in batches from a snapshot of write values.
fn write_batched_io(
    client: &mut TcpModbusClient,
    unit_id: u8,
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
                // Batch consecutive coils into a single FC0F write
                let start_addr = first_pf.register.address as u16;
                let mut end = i + 1;
                while end < output_fields.len() {
                    let (_, next_pf) = output_fields[end];
                    let expected_addr = start_addr + (end - i) as u16;
                    if next_pf.register.kind != RegisterKind::Coil
                        || next_pf.register.address as u16 != expected_addr
                    {
                        break;
                    }
                    end += 1;
                }
                let count = end - i;
                if count == 1 {
                    let val = write_values[first_idx].as_bool();
                    if let Err(e) =
                        client.write_single_coil(unit_id, start_addr, val)
                    {
                        tracing::debug!(
                            "Modbus TCP write {}.{}: {e}",
                            profile.name,
                            first_pf.name
                        );
                        had_error = true;
                    }
                } else {
                    let coils: Vec<bool> = (i..end)
                        .map(|j| {
                            let (idx, _) = output_fields[j];
                            write_values[idx].as_bool()
                        })
                        .collect();
                    if let Err(e) =
                        client.write_multiple_coils(unit_id, start_addr, &coils)
                    {
                        tracing::debug!(
                            "Modbus TCP batch write {}.{}: {e}",
                            profile.name,
                            first_pf.name
                        );
                        had_error = true;
                    }
                }
                i = end;
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
                    let raw = value_to_register(
                        &write_values[first_idx],
                        first_pf.data_type,
                        &first_pf.register,
                    );
                    if let Err(e) =
                        client.write_single_register(unit_id, start_addr, raw)
                    {
                        tracing::debug!(
                            "Modbus TCP write {}.{}: {e}",
                            profile.name,
                            first_pf.name
                        );
                        had_error = true;
                    }
                } else {
                    let values: Vec<u16> = (i..end)
                        .map(|j| {
                            let (idx, pf) = output_fields[j];
                            value_to_register(&write_values[idx], pf.data_type, &pf.register)
                        })
                        .collect();
                    if let Err(e) =
                        client.write_multiple_registers(unit_id, start_addr, &values)
                    {
                        tracing::debug!(
                            "Modbus TCP batch write {}.{}: {e}",
                            profile.name,
                            first_pf.name
                        );
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

// ── Value conversion helpers ─────────────────────────────────────────

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
        FieldDataType::Usint
        | FieldDataType::Uint
        | FieldDataType::Udint
        | FieldDataType::Ulint
        | FieldDataType::Byte
        | FieldDataType::Word
        | FieldDataType::Dword
        | FieldDataType::Lword => Value::UInt(scaled as u64),
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
        Value::Bool(true) => 1.0,
        Value::Bool(false) => 0.0,
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

// ── Tests ────────────────────────────────────────────────────────────

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
        assert_eq!(register_to_value(42, FieldDataType::Int, &reg).as_int(), 42);
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
        assert!(
            (register_to_value(450, FieldDataType::Real, &reg).as_real() - 45.0).abs() < 0.01
        );
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
        assert_eq!(
            value_to_register(&Value::Real(45.0), FieldDataType::Real, &reg),
            450
        );
    }

    #[test]
    fn modbus_tcp_layout_from_profile() {
        let profile = DeviceProfile::from_yaml(
            r#"
name: TestModbusTcp
protocol: modbus-tcp
fields:
  - { name: DI_0, type: BOOL, direction: input, register: { address: 0, kind: discrete_input } }
  - { name: AO_0, type: INT, direction: output, register: { address: 0, kind: holding_register } }
"#,
        )
        .unwrap();

        let fb = ModbusTcpDeviceNativeFb::new(profile);
        let layout = fb.layout();
        assert_eq!(layout.type_name, "TestModbusTcp");
        // 8 fixed + 2 profile fields
        assert_eq!(layout.fields.len(), 10);
        assert_eq!(layout.fields[SLOT_HOST].name, "host");
        assert_eq!(layout.fields[SLOT_PORT].name, "port");
        assert_eq!(layout.fields[SLOT_UNIT_ID].name, "unit_id");
        assert_eq!(layout.fields[PROFILE_FIELD_OFFSET].name, "DI_0");
        assert_eq!(layout.fields[PROFILE_FIELD_OFFSET + 1].name, "AO_0");
    }

    #[test]
    fn slot_constants_match_layout_fields() {
        let profile = DeviceProfile::from_yaml(
            r#"
name: SlotCheck
protocol: modbus-tcp
fields:
  - { name: F0, type: INT, direction: input, register: { address: 0, kind: input_register } }
"#,
        )
        .unwrap();

        let fb = ModbusTcpDeviceNativeFb::new(profile);
        let layout = fb.layout();

        assert_eq!(layout.fields[SLOT_HOST].name, "host");
        assert_eq!(layout.fields[SLOT_PORT].name, "port");
        assert_eq!(layout.fields[SLOT_UNIT_ID].name, "unit_id");
        assert_eq!(layout.fields[SLOT_REFRESH_RATE].name, "refresh_rate");
        assert_eq!(layout.fields[SLOT_CONNECTED].name, "connected");
        assert_eq!(layout.fields[SLOT_ERROR_CODE].name, "error_code");
        assert_eq!(layout.fields[SLOT_IO_CYCLES].name, "io_cycles");
        assert_eq!(layout.fields[SLOT_LAST_RESPONSE_MS].name, "last_response_ms");
        assert_eq!(PROFILE_FIELD_OFFSET, 8);
        assert_eq!(layout.fields[PROFILE_FIELD_OFFSET].name, "F0");
    }

    #[test]
    fn shared_layout_matches_runtime_layout() {
        let profile = DeviceProfile::from_yaml(
            r#"
name: LayoutMatch
protocol: modbus-tcp
fields:
  - { name: AI_0, type: INT, direction: input, register: { address: 0, kind: input_register } }
  - { name: AI_1, type: INT, direction: input, register: { address: 1, kind: input_register } }
  - { name: AO_0, type: INT, direction: output, register: { address: 0, kind: holding_register } }
"#,
        )
        .unwrap();

        let fb = ModbusTcpDeviceNativeFb::new(profile.clone());
        let runtime_layout = fb.layout();
        let shared_layout = profile.to_modbus_tcp_device_layout();

        assert_eq!(
            runtime_layout.fields.len(),
            shared_layout.fields.len(),
            "Runtime and shared layouts must have same number of fields"
        );

        for (i, (rt, sh)) in runtime_layout
            .fields
            .iter()
            .zip(shared_layout.fields.iter())
            .enumerate()
        {
            assert_eq!(
                rt.name, sh.name,
                "Field name mismatch at slot {i}: runtime='{}', shared='{}'",
                rt.name, sh.name
            );
            assert_eq!(
                rt.data_type, sh.data_type,
                "Field type mismatch at slot {i} ('{}'): runtime={:?}, shared={:?}",
                rt.name, rt.data_type, sh.data_type
            );
            assert_eq!(
                rt.var_kind, sh.var_kind,
                "Field var_kind mismatch at slot {i} ('{}'): runtime={:?}, shared={:?}",
                rt.name, rt.var_kind, sh.var_kind
            );
        }
    }

    #[test]
    fn execute_no_host_configured() {
        let profile = DeviceProfile::from_yaml(
            r#"
name: NoHost
protocol: modbus-tcp
fields:
  - { name: AI_0, type: INT, direction: input, register: { address: 0, kind: input_register } }
"#,
        )
        .unwrap();

        let fb = ModbusTcpDeviceNativeFb::new(profile);
        let mut fields = vec![
            Value::String(String::new()), // host (empty)
            Value::Int(502),              // port
            Value::Int(1),                // unit_id
            Value::Int(50),               // refresh_rate
            Value::Bool(false),           // connected
            Value::Int(0),                // error_code
            Value::UInt(0),               // io_cycles
            Value::Real(0.0),             // last_response_ms
            Value::Int(0),                // AI_0
        ];
        fb.execute(&mut fields);
        assert_eq!(fields[SLOT_CONNECTED], Value::Bool(false));
        assert_eq!(fields[SLOT_ERROR_CODE], Value::Int(1)); // No host
    }
}
