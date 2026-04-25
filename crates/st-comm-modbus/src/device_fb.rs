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
const SLOT_TIMEOUT: usize = 3;
const SLOT_PREAMBLE: usize = 4;
const SLOT_CONNECTED: usize = 5;
const SLOT_ERROR_CODE: usize = 6;
const SLOT_ERRORS_COUNT: usize = 7;
const SLOT_IO_CYCLES: usize = 8;
const SLOT_LAST_RESPONSE_MS: usize = 9;
const PROFILE_FIELD_OFFSET: usize = 10;

// ── Diagnostic error codes exposed via the FB's `error_code` VAR ──────
// Kept stable so user ST code (and the monitor UI) can compare against
// them. The poll cycle reports the last non-zero code observed.
//
// 0  : OK — no error this cycle.
// 10 : Receive deadline elapsed before the response was complete.
// 11 : Response received but CRC didn't match.
// 12 : Response slave_id didn't match the request (stale frame from
//      another device, or a spurious frame on the bus).
// 13 : Response function code didn't match the request (or unknown FC).
// 14 : Slave returned a Modbus exception (FC | 0x80) — the slave is
//      reachable but rejected the request.
// 15 : Other transport / protocol error (short response, I/O error,
//      buffer-too-small, etc.).
pub const ERR_OK: i64 = 0;
pub const ERR_TIMEOUT: i64 = 10;
pub const ERR_CRC: i64 = 11;
pub const ERR_SLAVE_MISMATCH: i64 = 12;
pub const ERR_FC_MISMATCH: i64 = 13;
pub const ERR_MODBUS_EXCEPTION: i64 = 14;
pub const ERR_OTHER: i64 = 15;

/// Compute the expanded offset for each profile field.
/// Returns a vec where `offsets[i]` is the starting index for profile field `i`
/// in the expanded Value arrays (IoState.read_values/write_values and the
/// profile-field portion of the `execute()` slice).
fn compute_field_offsets(profile: &DeviceProfile) -> Vec<usize> {
    let mut offsets = Vec::with_capacity(profile.fields.len());
    let mut offset = 0;
    for pf in &profile.fields {
        offsets.push(offset);
        offset += pf.count.max(1) as usize;
    }
    offsets
}

/// Total number of expanded values across all profile fields.
fn total_expanded_values(profile: &DeviceProfile) -> usize {
    profile.fields.iter().map(|pf| pf.count.max(1) as usize).sum()
}

/// Shared state between the scan-cycle thread and the background I/O thread.
pub(crate) struct IoState {
    /// Latest values read from the device (expanded: one Value per register).
    pub read_values: Vec<Value>,
    /// Values to write to the device (expanded: one Value per register).
    pub write_values: Vec<Value>,
    /// Diagnostics
    pub connected: bool,
    pub error_code: i64,
    /// Cumulative count of poll cycles that ended with `error_code != ERR_OK`
    /// since the FB instance was created. Never reset; useful for spotting
    /// transient reliability issues over long uptimes.
    pub errors_count: u64,
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
        let total = total_expanded_values(&profile);
        let mut default_values: Vec<Value> = Vec::with_capacity(total);
        for pf in &profile.fields {
            let def = Value::default_for_type(st_comm_api::field_data_type_to_var_type(pf.data_type));
            for _ in 0..pf.count.max(1) {
                default_values.push(def.clone());
            }
        }
        Self {
            layout,
            bus_manager,
            io_state: Arc::new(Mutex::new(IoState {
                read_values: default_values.clone(),
                write_values: default_values,
                connected: false,
                error_code: 0,
                errors_count: 0,
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

                let timeout_ms = fields[SLOT_TIMEOUT].as_int();
                let timeout = if timeout_ms > 0 {
                    Duration::from_millis(timeout_ms as u64)
                } else {
                    crate::rtu_client::DEFAULT_TIMEOUT
                };

                let preamble_ms = fields[SLOT_PREAMBLE].as_int();
                let preamble = if preamble_ms > 0 {
                    Duration::from_millis(preamble_ms as u64)
                } else {
                    crate::rtu_client::DEFAULT_PREAMBLE
                };

                let io = ModbusDeviceIo::new(
                    slave_id,
                    timeout,
                    preamble,
                    self.profile.clone(),
                    Arc::clone(&self.io_state),
                );
                self.bus_manager.register(&link, interval, Box::new(io));
                *registered = true;
            }
        }

        // Queue output values for the background thread to write
        let offsets = compute_field_offsets(&self.profile);
        {
            let mut state = self.io_state.lock().unwrap();
            for (i, pf) in self.profile.fields.iter().enumerate() {
                if matches!(pf.direction, FieldDirection::Output | FieldDirection::Inout) {
                    let count = pf.count.max(1) as usize;
                    let base = PROFILE_FIELD_OFFSET + offsets[i];
                    let io_base = offsets[i];
                    for j in 0..count {
                        if base + j < fields.len() && io_base + j < state.write_values.len() {
                            state.write_values[io_base + j] = fields[base + j].clone();
                        }
                    }
                }
            }
        }

        // Copy cached read values and diagnostics into the field slice
        {
            let state = self.io_state.lock().unwrap();
            for (i, pf) in self.profile.fields.iter().enumerate() {
                if matches!(pf.direction, FieldDirection::Input | FieldDirection::Inout) {
                    let count = pf.count.max(1) as usize;
                    let base = PROFILE_FIELD_OFFSET + offsets[i];
                    let io_base = offsets[i];
                    for j in 0..count {
                        if base + j < fields.len() && io_base + j < state.read_values.len() {
                            fields[base + j] = state.read_values[io_base + j].clone();
                        }
                    }
                }
            }
            fields[SLOT_CONNECTED] = Value::Bool(state.connected);
            fields[SLOT_ERROR_CODE] = Value::Int(state.error_code);
            fields[SLOT_ERRORS_COUNT] = Value::UInt(state.errors_count);
            fields[SLOT_IO_CYCLES] = Value::UInt(state.io_cycles);
            fields[SLOT_LAST_RESPONSE_MS] = Value::Real(state.last_response_ms);
        }
    }
}

// ── BusDeviceIo implementation for Modbus RTU ─────────────────────────

/// Protocol-specific I/O for a Modbus RTU device on a serial bus.
struct ModbusDeviceIo {
    slave_id: u8,
    timeout: Duration,
    preamble: Duration,
    profile: DeviceProfile,
    io_state: Arc<Mutex<IoState>>,
}

impl ModbusDeviceIo {
    fn new(
        slave_id: u8,
        timeout: Duration,
        preamble: Duration,
        profile: DeviceProfile,
        io_state: Arc<Mutex<IoState>>,
    ) -> Self {
        Self { slave_id, timeout, preamble, profile, io_state }
    }
}

impl BusDeviceIo for ModbusDeviceIo {
    fn poll(&mut self, transport: &Arc<Mutex<SerialTransport>>) -> bool {
        let client = RtuClient::with_timing(Arc::clone(transport), self.timeout, self.preamble);
        let start = std::time::Instant::now();

        // Snapshot write values
        let write_snapshot: Vec<Value> = {
            self.io_state.lock().unwrap().write_values.clone()
        };

        // Read inputs (batched)
        let read_err = read_batched_io(&client, self.slave_id, &self.profile, &self.io_state);

        // Write outputs (batched)
        let write_err = write_batched_io(&client, self.slave_id, &self.profile, &write_snapshot);

        let cycle_err = combine_cycle_errors(read_err, write_err);

        // Update diagnostics
        {
            let elapsed = start.elapsed();
            let mut state = self.io_state.lock().unwrap();
            state.connected = cycle_err == ERR_OK;
            state.error_code = cycle_err;
            state.io_cycles += 1;
            if cycle_err != ERR_OK {
                state.errors_count = state.errors_count.saturating_add(1);
            }
            state.last_response_ms = elapsed.as_secs_f64() * 1000.0;
        }

        cycle_err == ERR_OK
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

/// An expanded register entry: one register in the flat expanded list.
struct ExpandedReg {
    /// Index in IoState.read_values / write_values (expanded).
    io_idx: usize,
    /// Register address on the device.
    address: u16,
    /// Register kind (coil, discrete_input, holding_register, input_register).
    kind: RegisterKind,
    /// Data type for scaling.
    data_type: FieldDataType,
    /// Register mapping (for scale/offset).
    register: st_comm_api::profile::RegisterMapping,
    /// Profile field name (for logging).
    field_name: String,
}

/// Expand profile fields into a flat register list, accounting for `count`.
fn expand_registers(
    profile: &DeviceProfile,
    direction_filter: impl Fn(&FieldDirection) -> bool,
) -> Vec<ExpandedReg> {
    let offsets = compute_field_offsets(profile);
    let mut regs = Vec::new();
    for (i, pf) in profile.fields.iter().enumerate() {
        if !direction_filter(&pf.direction) {
            continue;
        }
        let count = pf.count.max(1) as usize;
        for j in 0..count {
            regs.push(ExpandedReg {
                io_idx: offsets[i] + j,
                address: pf.register.address as u16 + j as u16,
                kind: pf.register.kind,
                data_type: pf.data_type,
                register: pf.register.clone(),
                field_name: pf.name.clone(),
            });
        }
    }
    regs
}

/// Map a transaction error message to a numeric error code. Pattern
/// matches against the strings produced by `transport.rs`, `frame.rs`,
/// and `frame_parser.rs`. Anything we don't recognise falls into
/// [`ERR_OTHER`] so an unfamiliar error never silently looks like OK.
fn classify_error(msg: &str) -> i64 {
    if msg.contains("Receive timeout") {
        ERR_TIMEOUT
    } else if msg.contains("CRC") {
        ERR_CRC
    } else if msg.contains("Slave ID mismatch") {
        ERR_SLAVE_MISMATCH
    } else if msg.contains("Function code mismatch")
        || msg.contains("Unrecognised Modbus function code")
    {
        ERR_FC_MISMATCH
    } else if msg.contains("Modbus exception") {
        ERR_MODBUS_EXCEPTION
    } else {
        ERR_OTHER
    }
}

/// Combine two cycle-level error codes, preferring the later (post-read,
/// i.e. write-pass) one when both are non-zero. A zero in either slot
/// means "that pass was clean," so the other pass's code wins.
fn combine_cycle_errors(read_err: i64, write_err: i64) -> i64 {
    if write_err != ERR_OK { write_err } else { read_err }
}

/// Read input-direction fields in batches, storing results into `io_state`.
/// Returns the last non-zero error code observed, or [`ERR_OK`] if every
/// transaction succeeded.
fn read_batched_io(
    client: &RtuClient,
    slave_id: u8,
    profile: &DeviceProfile,
    io_state: &Arc<Mutex<IoState>>,
) -> i64 {
    let mut last_err = ERR_OK;
    let regs = expand_registers(profile, |d| {
        matches!(d, FieldDirection::Input | FieldDirection::Inout)
    });

    let mut i = 0;
    while i < regs.len() {
        let first = &regs[i];

        if first.kind == RegisterKind::Virtual {
            i += 1;
            continue;
        }

        // Find consecutive registers of the same kind
        let start_addr = first.address;
        let kind = first.kind;
        let mut end = i + 1;
        while end < regs.len() {
            let next = &regs[end];
            let expected_addr = start_addr + (end - i) as u16;
            if next.kind != kind || next.address != expected_addr {
                break;
            }
            end += 1;
        }
        let count = (end - i) as u16;

        let result = match kind {
            RegisterKind::Coil => client.read_coils(slave_id, start_addr, count).map(|bools| {
                let mut state = io_state.lock().unwrap();
                for (j, &val) in bools.iter().enumerate() {
                    state.read_values[regs[i + j].io_idx] = Value::Bool(val);
                }
            }),
            RegisterKind::DiscreteInput => client
                .read_discrete_inputs(slave_id, start_addr, count)
                .map(|bools| {
                    let mut state = io_state.lock().unwrap();
                    for (j, &val) in bools.iter().enumerate() {
                        state.read_values[regs[i + j].io_idx] = Value::Bool(val);
                    }
                }),
            RegisterKind::HoldingRegister => client
                .read_holding_registers(slave_id, start_addr, count)
                .map(|regs_data| {
                    let mut state = io_state.lock().unwrap();
                    for (j, &raw) in regs_data.iter().enumerate() {
                        let r = &regs[i + j];
                        state.read_values[r.io_idx] =
                            register_to_value(raw, r.data_type, &r.register);
                    }
                }),
            RegisterKind::InputRegister => client
                .read_input_registers(slave_id, start_addr, count)
                .map(|regs_data| {
                    let mut state = io_state.lock().unwrap();
                    for (j, &raw) in regs_data.iter().enumerate() {
                        let r = &regs[i + j];
                        state.read_values[r.io_idx] =
                            register_to_value(raw, r.data_type, &r.register);
                    }
                }),
            RegisterKind::Virtual => Ok(()),
        };

        if let Err(e) = result {
            tracing::debug!(
                "modbus read fail profile={} field={} slave={} kind={:?} addr={} count={}: {e}",
                profile.name, first.field_name, slave_id, kind, start_addr, count,
            );
            last_err = classify_error(&e);
        }

        i = end;
    }
    last_err
}

/// Write output-direction fields in batches from a snapshot of write values.
/// Returns the last non-zero error code observed, or [`ERR_OK`] if every
/// transaction succeeded.
fn write_batched_io(
    client: &RtuClient,
    slave_id: u8,
    profile: &DeviceProfile,
    write_values: &[Value],
) -> i64 {
    let mut last_err = ERR_OK;
    let regs = expand_registers(profile, |d| {
        matches!(d, FieldDirection::Output | FieldDirection::Inout)
    });

    let mut i = 0;
    while i < regs.len() {
        let first = &regs[i];

        match first.kind {
            RegisterKind::Coil => {
                // Batch consecutive coils into a single FC0F write
                let start_addr = first.address;
                let mut end = i + 1;
                while end < regs.len() {
                    let next = &regs[end];
                    let expected_addr = start_addr + (end - i) as u16;
                    if next.kind != RegisterKind::Coil || next.address != expected_addr {
                        break;
                    }
                    end += 1;
                }
                let count = end - i;
                if count == 1 {
                    let val = write_values[first.io_idx].as_bool();
                    if let Err(e) = client.write_single_coil(slave_id, start_addr, val) {
                        tracing::debug!(
                            "modbus write fail profile={} field={} slave={} kind=Coil addr={} count=1 op=single: {e}",
                            profile.name, first.field_name, slave_id, start_addr,
                        );
                        last_err = classify_error(&e);
                    }
                } else {
                    let coils: Vec<bool> = (i..end)
                        .map(|j| write_values[regs[j].io_idx].as_bool())
                        .collect();
                    if let Err(e) = client.write_multiple_coils(slave_id, start_addr, &coils) {
                        tracing::debug!(
                            "modbus write fail profile={} field={} slave={} kind=Coil addr={} count={} op=multi: {e}",
                            profile.name, first.field_name, slave_id, start_addr, coils.len(),
                        );
                        last_err = classify_error(&e);
                    }
                }
                i = end;
            }
            RegisterKind::HoldingRegister => {
                let start_addr = first.address;
                let mut end = i + 1;
                while end < regs.len() {
                    let next = &regs[end];
                    let expected_addr = start_addr + (end - i) as u16;
                    if next.kind != RegisterKind::HoldingRegister
                        || next.address != expected_addr
                    {
                        break;
                    }
                    end += 1;
                }
                let count = end - i;
                if count == 1 {
                    let raw = value_to_register(
                        &write_values[first.io_idx],
                        first.data_type,
                        &first.register,
                    );
                    if let Err(e) = client.write_single_register(slave_id, start_addr, raw) {
                        tracing::debug!(
                            "modbus write fail profile={} field={} slave={} kind=HoldingRegister addr={} count=1 op=single: {e}",
                            profile.name, first.field_name, slave_id, start_addr,
                        );
                        last_err = classify_error(&e);
                    }
                } else {
                    let values: Vec<u16> = (i..end)
                        .map(|j| {
                            let r = &regs[j];
                            value_to_register(&write_values[r.io_idx], r.data_type, &r.register)
                        })
                        .collect();
                    if let Err(e) = client.write_multiple_registers(slave_id, start_addr, &values) {
                        tracing::debug!(
                            "modbus write fail profile={} field={} slave={} kind=HoldingRegister addr={} count={} op=multi: {e}",
                            profile.name, first.field_name, slave_id, start_addr, values.len(),
                        );
                        last_err = classify_error(&e);
                    }
                }
                i = end;
            }
            _ => {
                i += 1;
            }
        }
    }
    last_err
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
    fn classify_error_recognises_each_failure_class() {
        // Strings here are samples of the actual error messages produced
        // by the transport / parser code paths, so this test would catch
        // a future rewording that breaks classification.
        assert_eq!(
            classify_error("Receive timeout: got 0 of expected 5+ bytes within 100ms"),
            ERR_TIMEOUT
        );
        assert_eq!(classify_error("CRC mismatch"), ERR_CRC);
        assert_eq!(
            classify_error("Slave ID mismatch: expected 0x14, got 0x15"),
            ERR_SLAVE_MISMATCH
        );
        assert_eq!(
            classify_error(
                "Function code mismatch: expected 0x02 (or exception), got 0x03"
            ),
            ERR_FC_MISMATCH
        );
        assert_eq!(
            classify_error("Unrecognised Modbus function code 0x42 in response"),
            ERR_FC_MISMATCH
        );
        assert_eq!(
            classify_error("Modbus exception: Illegal data address (code 2)"),
            ERR_MODBUS_EXCEPTION
        );
        assert_eq!(classify_error("Serial read error: Connection reset"), ERR_OTHER);
    }

    #[test]
    fn combine_cycle_errors_prefers_write_error_when_both_present() {
        assert_eq!(combine_cycle_errors(ERR_OK, ERR_OK), ERR_OK);
        assert_eq!(combine_cycle_errors(ERR_TIMEOUT, ERR_OK), ERR_TIMEOUT);
        assert_eq!(combine_cycle_errors(ERR_OK, ERR_CRC), ERR_CRC);
        // Write pass happens after the read pass, so its error supersedes.
        assert_eq!(combine_cycle_errors(ERR_TIMEOUT, ERR_CRC), ERR_CRC);
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
    fn array_field_expands_offsets_and_total() {
        let profile = DeviceProfile::from_yaml(r#"
name: ArrayCounts
protocol: modbus-rtu
fields:
  - { name: DO, type: BOOL, direction: output, count: 8, register: { address: 0, kind: coil } }
  - { name: DI, type: BOOL, direction: input,  count: 8, register: { address: 0, kind: discrete_input } }
  - { name: AI, type: INT,  direction: input,  count: 4, register: { address: 0, kind: input_register } }
"#).unwrap();

        // DO occupies 0..8, DI occupies 8..16, AI occupies 16..20.
        assert_eq!(compute_field_offsets(&profile), vec![0, 8, 16]);
        assert_eq!(total_expanded_values(&profile), 20);
    }

    #[test]
    fn array_field_expands_into_consecutive_registers() {
        let profile = DeviceProfile::from_yaml(r#"
name: ExpandRegs
protocol: modbus-rtu
fields:
  - { name: DI, type: BOOL, direction: input, count: 8, register: { address: 0, kind: discrete_input } }
"#).unwrap();

        let regs = expand_registers(&profile, |d| matches!(d, FieldDirection::Input));
        assert_eq!(regs.len(), 8);
        for (j, r) in regs.iter().enumerate() {
            assert_eq!(r.io_idx, j);
            assert_eq!(r.address, j as u16);
            assert_eq!(r.kind, RegisterKind::DiscreteInput);
        }
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
        // 10 fixed + 2 profile fields
        assert_eq!(layout.fields.len(), 12);
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
        assert_eq!(layout.fields[SLOT_TIMEOUT].name, "timeout");
        assert_eq!(layout.fields[SLOT_PREAMBLE].name, "preamble");
        assert_eq!(layout.fields[SLOT_CONNECTED].name, "connected");
        assert_eq!(layout.fields[SLOT_ERROR_CODE].name, "error_code");
        assert_eq!(layout.fields[SLOT_ERRORS_COUNT].name, "errors_count");
        assert_eq!(layout.fields[SLOT_IO_CYCLES].name, "io_cycles");
        assert_eq!(layout.fields[SLOT_LAST_RESPONSE_MS].name, "last_response_ms");
        assert_eq!(PROFILE_FIELD_OFFSET, 10);
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
