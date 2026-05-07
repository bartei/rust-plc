//! `UppDeviceNativeFb` — generic UPP pyrometer instance, parameterised
//! by a [`DeviceProfile`] whose fields each carry an `upp:` binding.
//!
//! Mirrors `ModbusRtuDeviceNativeFb` in shape: the FB takes a `link`
//! reference to a separate `SerialLink`, queues writes / reads cached
//! values via [`IoState`], and runs all I/O on the
//! [`BusManager`](st_comm_serial::BusManager) per-port background
//! thread so the PLC scan cycle never blocks.
//!
//! ## Layout (slot order, matched to [`to_upp_device_layout`])
//!
//! | Slot | Direction | Name              | Type   |
//! |------|-----------|-------------------|--------|
//! | 0    | INPUT     | `link`            | STRING |
//! | 1    | INPUT     | `device_id`       | INT    |
//! | 2    | INPUT     | `refresh_rate`    | TIME   |
//! | 3    | INPUT     | `timeout`         | TIME   |
//! | 4    | INPUT     | `cooldown`        | TIME   |
//! | 5    | VAR       | `connected`       | BOOL   |
//! | 6    | VAR       | `error_code`      | INT    |
//! | 7    | VAR       | `errors_count`    | UDINT  |
//! | 8    | VAR       | `io_cycles`       | UDINT  |
//! | 9    | VAR       | `last_response_ms`| REAL   |
//! | 10+  | VAR       | (profile fields)  | per profile |
//!
//! [`to_upp_device_layout`]: st_comm_api::native_fb::DeviceProfile::to_upp_device_layout

use crate::address::Address;
use crate::client::UppClient;
use crate::command::Command;
use crate::error::UppError;
use crate::parser::DecodedValue;
use crate::profile_binding::{self, ResolvedBinding, WriteCmdKind};
use st_comm_api::native_fb::*;
use st_comm_api::profile::{DeviceProfile, FieldDirection};
use st_comm_serial::bus::{BusDeviceIo, BusManager};
use st_comm_serial::transport::SerialTransport;
use st_ir::Value;
use std::sync::{Arc, Mutex};
use std::time::Duration;

// ── Slot constants (mirror the layout in `to_upp_device_layout`) ──

const SLOT_LINK: usize = 0;
const SLOT_DEVICE_ID: usize = 1;
const SLOT_REFRESH_RATE: usize = 2;
const SLOT_TIMEOUT: usize = 3;
const SLOT_COOLDOWN: usize = 4;
const SLOT_CONNECTED: usize = 5;
const SLOT_ERROR_CODE: usize = 6;
const SLOT_ERRORS_COUNT: usize = 7;
const SLOT_IO_CYCLES: usize = 8;
const SLOT_LAST_RESPONSE_MS: usize = 9;
const PROFILE_FIELD_OFFSET: usize = 10;

// ── Diagnostic codes (returned through `SLOT_ERROR_CODE`) ──────────
//
// 0 is "no error". Match `UppError::code()` for the protocol-level
// failure modes; the FB layer adds 100/101 for runtime config
// problems caught BEFORE we ever try to talk to the bus (so the user
// can tell "timeout" from "no link configured" without reading the
// last_response_ms field).
pub const ERR_OK: i64 = 0;
/// Protocol-level error codes: lifted directly from
/// [`UppError::code`].
pub const ERR_PROTO_BASE: i64 = 0;
/// VAR_INPUT `link` was empty or not a STRING.
pub const ERR_NO_LINK: i64 = 100;
/// VAR_INPUT `device_id` was outside 0..=99.
pub const ERR_BAD_ADDRESS: i64 = 101;
/// FB construction-time profile error (unknown command, decoder, etc.).
pub const ERR_PROFILE: i64 = 102;

/// Shared state between the scan-cycle thread (`execute()`) and the
/// background I/O thread (`UppDeviceIo::poll()`).
pub(crate) struct IoState {
    /// Latest values read from the device, one per profile field.
    pub read_values: Vec<Value>,
    /// Pending writes — `Some(v)` means "write this on the next
    /// poll cycle, then clear back to None". Indexed by profile
    /// field. `None` means "no pending write".
    pub write_values: Vec<Option<Value>>,
    pub connected: bool,
    pub error_code: i64,
    pub errors_count: u64,
    pub io_cycles: u64,
    pub last_response_ms: f64,
}

/// One concrete UPP device instance.
///
/// Use [`UppDeviceNativeFb::new`] to build from a parsed
/// [`DeviceProfile`] whose `protocol:` field is `"upp"` and whose
/// fields each carry an `upp:` binding (resolved up-front at
/// construction time via [`profile_binding::resolve`]).
pub struct UppDeviceNativeFb {
    layout: NativeFbLayout,
    profile: DeviceProfile,
    bindings: Vec<ResolvedBinding>,
    profile_error: Option<String>,
    bus_manager: Arc<BusManager>,
    io_state: Arc<Mutex<IoState>>,
    registered: Mutex<bool>,
}

impl UppDeviceNativeFb {
    /// Construct from a profile + a shared bus manager. Resolution of
    /// the per-field UPP bindings happens here; resolution failures
    /// are stored on the FB and surfaced via `error_code = 102`
    /// every cycle until the profile is corrected (the FB does not
    /// panic — a bad profile must not crash the runtime thread).
    pub fn new(profile: DeviceProfile, bus_manager: Arc<BusManager>) -> Self {
        let layout = profile.to_upp_device_layout();

        let mut bindings = Vec::with_capacity(profile.fields.len());
        let mut first_error: Option<String> = None;
        for pf in &profile.fields {
            match profile_binding::resolve(pf) {
                Ok(b) => bindings.push(b),
                Err(e) => {
                    let msg = format!("field {:?}: {e}", pf.name);
                    if first_error.is_none() {
                        first_error = Some(msg);
                    }
                    // Push a placeholder so indices stay aligned with
                    // the field list — the I/O thread skips fields
                    // whose binding didn't resolve.
                    bindings.push(ResolvedBinding {
                        read_cmd: Command::ReadMeasuringValue,
                        write_cmd_kind: None,
                        decoder: crate::parser::Decoder::Text,
                    });
                }
            }
        }

        let read_defaults: Vec<Value> = profile
            .fields
            .iter()
            .map(|pf| Value::default_for_type(field_data_type_to_var_type(pf.data_type)))
            .collect();
        let write_pending: Vec<Option<Value>> = vec![None; profile.fields.len()];

        Self {
            layout,
            profile,
            bindings,
            profile_error: first_error.clone(),
            bus_manager,
            io_state: Arc::new(Mutex::new(IoState {
                read_values: read_defaults,
                write_values: write_pending,
                connected: false,
                error_code: if first_error.is_some() { ERR_PROFILE } else { ERR_OK },
                errors_count: 0,
                io_cycles: 0,
                last_response_ms: 0.0,
            })),
            registered: Mutex::new(false),
        }
    }
}

impl NativeFb for UppDeviceNativeFb {
    fn type_name(&self) -> &str {
        &self.layout.type_name
    }

    fn layout(&self) -> &NativeFbLayout {
        &self.layout
    }

    fn execute(&self, fields: &mut [Value]) {
        // Bail out early on construction-time profile errors — every
        // cycle reports the diagnostic code without touching the bus.
        if self.profile_error.is_some() {
            fields[SLOT_CONNECTED] = Value::Bool(false);
            fields[SLOT_ERROR_CODE] = Value::Int(ERR_PROFILE);
            return;
        }

        let link = match &fields[SLOT_LINK] {
            Value::String(s) if !s.is_empty() => s.clone(),
            _ => {
                fields[SLOT_CONNECTED] = Value::Bool(false);
                fields[SLOT_ERROR_CODE] = Value::Int(ERR_NO_LINK);
                return;
            }
        };

        let device_id_int = fields[SLOT_DEVICE_ID].as_int();
        let address = match resolve_address(device_id_int) {
            Some(a) => a,
            None => {
                fields[SLOT_CONNECTED] = Value::Bool(false);
                fields[SLOT_ERROR_CODE] = Value::Int(ERR_BAD_ADDRESS);
                return;
            }
        };

        // First-cycle: register on the BusManager. The interval / timing
        // come from VAR_INPUT slots so the user can tune per-device.
        {
            let mut registered = self.registered.lock().unwrap();
            if !*registered {
                let refresh = duration_from_slot(&fields[SLOT_REFRESH_RATE], 200);
                let timeout = duration_from_slot(&fields[SLOT_TIMEOUT], 5);
                let cooldown = duration_from_slot(&fields[SLOT_COOLDOWN], 2);

                let io = UppDeviceIo {
                    address,
                    timeout,
                    cooldown,
                    profile: self.profile.clone(),
                    bindings: self.bindings.clone(),
                    io_state: Arc::clone(&self.io_state),
                };
                self.bus_manager
                    .register(&link, refresh, Box::new(io));
                *registered = true;
            }
        }

        // Queue writes for the background thread. We diff against the
        // last published `read_values` so we only emit a UPP write
        // when the program actually changed the field — burning a bus
        // round-trip on every cycle would be wasteful and would
        // double the round-trip latency observed by the program.
        {
            let mut state = self.io_state.lock().unwrap();
            for (i, pf) in self.profile.fields.iter().enumerate() {
                let writable = matches!(
                    pf.direction,
                    FieldDirection::Output | FieldDirection::Inout
                );
                if !writable {
                    continue;
                }
                let slot = PROFILE_FIELD_OFFSET + i;
                if slot >= fields.len() {
                    continue;
                }
                let new_val = &fields[slot];
                let last_known = &state.read_values[i];
                if !values_equal(new_val, last_known) {
                    state.write_values[i] = Some(new_val.clone());
                }
            }
        }

        // Copy cached read values + diagnostics back into `fields`.
        {
            let state = self.io_state.lock().unwrap();
            for (i, _pf) in self.profile.fields.iter().enumerate() {
                let slot = PROFILE_FIELD_OFFSET + i;
                if slot < fields.len() && i < state.read_values.len() {
                    fields[slot] = state.read_values[i].clone();
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

/// Translate a VAR_INPUT INT value into a [`Address`]. Accepts:
///
/// - `0..=97` → individual
/// - `98` → broadcast-with-response (one device only)
/// - `99` → broadcast-no-response
///
/// Returns `None` for anything else (negative, > 99, etc.).
fn resolve_address(n: i64) -> Option<Address> {
    if !(0..=99).contains(&n) {
        return None;
    }
    Some(match n as u8 {
        98 => Address::BroadcastWithResponse,
        99 => Address::BroadcastNoResponse,
        n => Address::individual(n).ok()?,
    })
}

/// Read a `Value::Time` (ms) into a [`Duration`], falling back to a
/// default if the slot is zero / not a Time. Matches the convention
/// used by the Modbus RTU FB.
fn duration_from_slot(v: &Value, default_ms: u64) -> Duration {
    let ms = v.as_int();
    if ms > 0 {
        Duration::from_millis(ms as u64)
    } else {
        Duration::from_millis(default_ms)
    }
}

/// Cheap equality on the relevant subset of `Value` — we only need
/// to detect "did the program change this field?" so it's enough to
/// compare bytes / numbers; we avoid pulling in a deep `PartialEq`
/// requirement on `Value` by handling the variants we actually use.
fn values_equal(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Bool(x), Value::Bool(y)) => x == y,
        (Value::Int(x), Value::Int(y)) => x == y,
        (Value::UInt(x), Value::UInt(y)) => x == y,
        (Value::Real(x), Value::Real(y)) => x.to_bits() == y.to_bits(),
        (Value::String(x), Value::String(y)) => x == y,
        (Value::Time(x), Value::Time(y)) => x == y,
        _ => false,
    }
}

// ── BusDeviceIo: the per-port background-thread implementation ─────

/// One device's I/O routine. Owned by the bus thread.
pub(crate) struct UppDeviceIo {
    address: Address,
    timeout: Duration,
    cooldown: Duration,
    profile: DeviceProfile,
    bindings: Vec<ResolvedBinding>,
    io_state: Arc<Mutex<IoState>>,
}

impl BusDeviceIo for UppDeviceIo {
    fn poll(&mut self, transport: &Arc<Mutex<SerialTransport>>) -> bool {
        let client = UppClient::with_timing(
            Arc::clone(transport),
            self.timeout,
            self.cooldown,
            Duration::ZERO, // UPP needs no extra preamble
        );
        let cycle_start = std::time::Instant::now();

        // Snapshot pending writes and clear them — fire-and-forget;
        // even if a write fails we don't queue a retry here, the
        // program can re-assign the field if it cares.
        let pending: Vec<Option<Value>> = {
            let mut state = self.io_state.lock().unwrap();
            std::mem::replace(
                &mut state.write_values,
                vec![None; self.profile.fields.len()],
            )
        };

        let mut last_err: Option<UppError> = None;

        // 1. Drain writes
        for (i, pf) in self.profile.fields.iter().enumerate() {
            let Some(val) = pending.get(i).and_then(|w| w.as_ref()) else {
                continue;
            };
            let binding = &self.bindings[i];
            let Some(kind) = binding.write_cmd_kind else {
                continue;
            };
            if let Some(cmd) = build_write_command(kind, val) {
                if let Err(e) = client.transaction(self.address, &cmd) {
                    tracing::warn!(
                        "UPP write {:?} ({}) failed: {e}",
                        pf.name, self.address
                    );
                    last_err = Some(e);
                }
            } else {
                tracing::warn!(
                    "UPP write {:?}: cannot encode {val:?} as {kind:?}",
                    pf.name
                );
                last_err = Some(UppError::OutOfRange(format!(
                    "field {:?}: value not encodable",
                    pf.name
                )));
            }
        }

        // 2. Read all input / inout fields
        let mut new_reads: Vec<Option<Value>> = vec![None; self.profile.fields.len()];
        for (i, pf) in self.profile.fields.iter().enumerate() {
            if !matches!(pf.direction, FieldDirection::Input | FieldDirection::Inout) {
                continue;
            }
            let binding = &self.bindings[i];
            match client.transact(self.address, &binding.read_cmd, binding.decoder) {
                Ok((decoded, _stats)) => {
                    new_reads[i] = decoded_to_value(&decoded, pf.data_type);
                }
                Err(e) => {
                    tracing::debug!("UPP read {:?} failed: {e}", pf.name);
                    last_err = Some(e);
                }
            }
        }

        // 3. Publish results + diagnostics in one critical section
        let elapsed = cycle_start.elapsed();
        let cycle_ok = last_err.is_none();
        {
            let mut state = self.io_state.lock().unwrap();
            for (i, v) in new_reads.into_iter().enumerate() {
                if let Some(v) = v {
                    state.read_values[i] = v;
                }
            }
            state.io_cycles = state.io_cycles.saturating_add(1);
            state.last_response_ms = elapsed.as_secs_f64() * 1000.0;
            if cycle_ok {
                state.connected = true;
                state.error_code = ERR_OK;
            } else {
                let code = last_err
                    .as_ref()
                    .map(|e| e.code() as i64)
                    .unwrap_or(ERR_OK);
                state.error_code = code;
                state.errors_count = state.errors_count.saturating_add(1);
                // `connected` flips to false only after the FIRST
                // failure of a previously-good link; once flipped, it
                // stays false until a clean cycle.
                if code != ERR_OK {
                    state.connected = false;
                }
            }
        }

        cycle_ok
    }
}

/// Build a parameterised UPP write command from a runtime [`Value`].
/// Returns `None` when the value's variant isn't compatible with the
/// command kind (e.g. trying to write a `Real` through `WriteOpMode`
/// which expects a 1-digit selector).
fn build_write_command(kind: WriteCmdKind, v: &Value) -> Option<Command> {
    use Command::*;
    Some(match kind {
        WriteCmdKind::Em => WriteEmissivity { value: real_to_milli(v)? },
        WriteCmdKind::Et => WriteTransmittance { value: real_to_milli(v)? },
        WriteCmdKind::Ev => WriteEmissivityRatio { value: real_to_milli(v)? },
        WriteCmdKind::Dw => WriteDirtyWindow { value: int_as_u8(v)? },
        WriteCmdKind::Aw => WriteSwitchOff { value: int_as_u8(v)? },
        WriteCmdKind::Ez => WriteResponseTime { value: int_as_u8(v)? },
        WriteCmdKind::Lz => WriteClearPeak { value: int_as_u8(v)? },
        WriteCmdKind::Fh => WriteFahrenheit { value: int_as_u8(v)? },
        WriteCmdKind::Ka => WriteOpMode { value: int_as_u8(v)? },
        WriteCmdKind::La => WriteLaser { value: bool_as_u8(v)? },
        WriteCmdKind::As => WriteAnalogOutput { value: int_as_u8(v)? },
        WriteCmdKind::Ga => WriteDeviceAddress { value: int_as_u8(v)? },
        WriteCmdKind::Br => WriteBaudRate { value: int_as_u8(v)? },
        WriteCmdKind::M1 => {
            // The runtime would need to pass two halves; a single
            // `Value` cannot represent the (lo, hi) pair, so we
            // currently reject m1 writes. Future stretch: bind m1 to
            // a struct-typed field.
            return None;
        }
        WriteCmdKind::M2 => ConfirmSubRange,
        WriteCmdKind::Lx => SimulateClearPeak,
    })
}

/// Convert a `Value::Real(0.853)` → `853` for UPP's per-1000
/// integer-encoded parameters (em, et, ev). Accepts `Real` and
/// `Int` to be friendly to programs that haven't picked a type.
fn real_to_milli(v: &Value) -> Option<u16> {
    let f = match v {
        Value::Real(r) => *r,
        Value::Int(i) => *i as f64,
        _ => return None,
    };
    if !f.is_finite() || !(0.0..=65.535).contains(&f) {
        return None;
    }
    Some((f * 1000.0).round() as u16)
}

fn int_as_u8(v: &Value) -> Option<u8> {
    match v {
        Value::Int(i) if (0..=255).contains(i) => Some(*i as u8),
        Value::UInt(u) if *u <= 255 => Some(*u as u8),
        Value::Bool(b) => Some(*b as u8),
        _ => None,
    }
}

fn bool_as_u8(v: &Value) -> Option<u8> {
    match v {
        Value::Bool(b) => Some(*b as u8),
        Value::Int(i) if *i == 0 || *i == 1 => Some(*i as u8),
        _ => None,
    }
}

/// Project a [`DecodedValue`] back into the runtime's [`Value`] type
/// for the field's declared data_type. The decoder already did the
/// numeric work; this just picks the correct ST variant.
fn decoded_to_value(
    d: &DecodedValue,
    target: st_comm_api::profile::FieldDataType,
) -> Option<Value> {
    use st_comm_api::profile::FieldDataType as F;
    match (d, target) {
        (DecodedValue::Temperature(t), F::Real | F::Lreal) => Some(Value::Real(*t)),
        (DecodedValue::Temperature(t), F::Int | F::Dint | F::Lint) => Some(Value::Int(*t as i64)),
        (DecodedValue::InternalTemp(t), F::Real | F::Lreal) => Some(Value::Real(*t)),
        (DecodedValue::InternalTemp(t), F::Int | F::Dint | F::Lint) => Some(Value::Int(*t as i64)),
        (DecodedValue::UInt(u), F::Udint | F::Uint | F::Ulint | F::Usint | F::Word | F::Dword | F::Lword) => {
            Some(Value::UInt(*u as u64))
        }
        (DecodedValue::UInt(u), F::Int | F::Dint | F::Lint | F::Sint) => Some(Value::Int(*u as i64)),
        (DecodedValue::UInt(u), F::Real | F::Lreal) => Some(Value::Real(*u as f64)),
        (DecodedValue::Per1000(x), F::Real | F::Lreal) => Some(Value::Real(*x)),
        (DecodedValue::Enum(e), F::Int | F::Dint | F::Lint | F::Sint) => Some(Value::Int(*e as i64)),
        (DecodedValue::Enum(e), F::Udint | F::Uint | F::Ulint | F::Usint) => {
            Some(Value::UInt(*e as u64))
        }
        (DecodedValue::Bool(b), F::Bool) => Some(Value::Bool(*b)),
        (DecodedValue::Text(s), F::String) => Some(Value::String(s.clone())),
        (DecodedValue::Ack(b), F::Bool) => Some(Value::Bool(*b)),
        // HexPair / TemperaturePair don't map cleanly to a single
        // ST scalar; the decoder layer already projected
        // TemperaturePair via the channel selector. HexPair is
        // currently used only for limits queries and isn't bound to
        // ordinary fields in the reference profile.
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use st_comm_api::profile::{FieldDataType, FieldDirection, ProfileField, UppFieldBinding};
    use st_comm_serial::bus::BusManager;

    fn upp_field(name: &str, dt: FieldDataType, dir: FieldDirection, cmd: &str, dec: &str) -> ProfileField {
        ProfileField {
            name: name.into(),
            data_type: dt,
            direction: dir,
            register: None,
            upp: Some(UppFieldBinding {
                command: cmd.into(),
                decoder: dec.into(),
                channel: None,
            }),
            count: 1,
            description: None,
        }
    }

    fn make_profile() -> DeviceProfile {
        DeviceProfile {
            name: "ImpacIgar6Smart".into(),
            vendor: Some("Impac".into()),
            protocol: Some("upp".into()),
            description: None,
            fields: vec![
                upp_field("temperature", FieldDataType::Real, FieldDirection::Input, "ms", "temp_5d_tenth"),
                upp_field("emissivity", FieldDataType::Real, FieldDirection::Inout, "em", "u16_dec_milli"),
            ],
        }
    }

    #[test]
    fn layout_has_expected_fixed_prefix() {
        let p = make_profile();
        let layout = p.to_upp_device_layout();
        assert_eq!(layout.type_name, "ImpacIgar6Smart");
        // Fixed prefix is 10 fields, then one per profile field.
        assert_eq!(layout.fields.len(), PROFILE_FIELD_OFFSET + 2);
        assert_eq!(layout.fields[SLOT_LINK].name, "link");
        assert_eq!(layout.fields[SLOT_DEVICE_ID].name, "device_id");
        assert_eq!(layout.fields[SLOT_REFRESH_RATE].name, "refresh_rate");
        assert_eq!(layout.fields[SLOT_TIMEOUT].name, "timeout");
        assert_eq!(layout.fields[SLOT_COOLDOWN].name, "cooldown");
        assert_eq!(layout.fields[SLOT_CONNECTED].name, "connected");
        assert_eq!(layout.fields[SLOT_ERROR_CODE].name, "error_code");
        assert_eq!(layout.fields[SLOT_LAST_RESPONSE_MS].name, "last_response_ms");
        assert_eq!(layout.fields[PROFILE_FIELD_OFFSET].name, "temperature");
        assert_eq!(layout.fields[PROFILE_FIELD_OFFSET + 1].name, "emissivity");
    }

    #[test]
    fn execute_with_empty_link_reports_no_link() {
        let p = make_profile();
        let bm = Arc::new(BusManager::new(st_comm_serial::new_transport_map()));
        let fb = UppDeviceNativeFb::new(p, bm);

        let mut fields = vec![Value::default(); fb.layout().fields.len()];
        fields[SLOT_LINK] = Value::String(String::new());
        fields[SLOT_DEVICE_ID] = Value::Int(0);
        fb.execute(&mut fields);

        assert_eq!(fields[SLOT_CONNECTED], Value::Bool(false));
        assert_eq!(fields[SLOT_ERROR_CODE], Value::Int(ERR_NO_LINK));
    }

    #[test]
    fn execute_with_invalid_address_reports_bad_address() {
        let p = make_profile();
        let bm = Arc::new(BusManager::new(st_comm_serial::new_transport_map()));
        let fb = UppDeviceNativeFb::new(p, bm);

        let mut fields = vec![Value::default(); fb.layout().fields.len()];
        fields[SLOT_LINK] = Value::String("/dev/null".into());
        fields[SLOT_DEVICE_ID] = Value::Int(123); // > 99
        fb.execute(&mut fields);

        assert_eq!(fields[SLOT_ERROR_CODE], Value::Int(ERR_BAD_ADDRESS));
    }

    #[test]
    fn execute_with_bad_profile_reports_err_profile() {
        let mut p = make_profile();
        // Inject an unknown decoder name on one field — resolve()
        // will fail and the FB must surface it as ERR_PROFILE.
        p.fields[0].upp = Some(UppFieldBinding {
            command: "ms".into(),
            decoder: "no_such_decoder".into(),
            channel: None,
        });
        let bm = Arc::new(BusManager::new(st_comm_serial::new_transport_map()));
        let fb = UppDeviceNativeFb::new(p, bm);

        let mut fields = vec![Value::default(); fb.layout().fields.len()];
        fields[SLOT_LINK] = Value::String("/dev/null".into());
        fields[SLOT_DEVICE_ID] = Value::Int(0);
        fb.execute(&mut fields);

        assert_eq!(fields[SLOT_CONNECTED], Value::Bool(false));
        assert_eq!(fields[SLOT_ERROR_CODE], Value::Int(ERR_PROFILE));
    }

    #[test]
    fn resolve_address_accepts_full_range() {
        for n in 0..=97 {
            assert!(matches!(resolve_address(n), Some(Address::Individual(_))));
        }
        assert!(matches!(resolve_address(98), Some(Address::BroadcastWithResponse)));
        assert!(matches!(resolve_address(99), Some(Address::BroadcastNoResponse)));
        assert!(resolve_address(-1).is_none());
        assert!(resolve_address(100).is_none());
    }

    #[test]
    fn real_to_milli_round_trips_in_range() {
        assert_eq!(real_to_milli(&Value::Real(0.0)), Some(0));
        assert_eq!(real_to_milli(&Value::Real(0.853)), Some(853));
        assert_eq!(real_to_milli(&Value::Real(1.000)), Some(1000));
        assert!(real_to_milli(&Value::Real(-0.1)).is_none());
        assert!(real_to_milli(&Value::Real(f64::NAN)).is_none());
        assert!(real_to_milli(&Value::Real(f64::INFINITY)).is_none());
    }

    #[test]
    fn build_write_command_emissivity() {
        let cmd = build_write_command(WriteCmdKind::Em, &Value::Real(0.853));
        assert!(matches!(cmd, Some(Command::WriteEmissivity { value: 853 })));
    }

    #[test]
    fn build_write_command_baud_rate_from_int() {
        let cmd = build_write_command(WriteCmdKind::Br, &Value::Int(4));
        assert!(matches!(cmd, Some(Command::WriteBaudRate { value: 4 })));
    }

    #[test]
    fn build_write_command_laser_from_bool() {
        let cmd = build_write_command(WriteCmdKind::La, &Value::Bool(true));
        assert!(matches!(cmd, Some(Command::WriteLaser { value: 1 })));
    }

    #[test]
    fn build_write_command_lx_no_param() {
        let cmd = build_write_command(WriteCmdKind::Lx, &Value::Bool(true));
        assert!(matches!(cmd, Some(Command::SimulateClearPeak)));
    }

    #[test]
    fn build_write_command_m1_rejects_until_struct_binding() {
        // Documented limitation: m1 needs a (lo, hi) pair — single
        // Value can't carry that. Return None until we add a
        // struct-typed binding.
        assert!(build_write_command(WriteCmdKind::M1, &Value::Int(0)).is_none());
    }

    #[test]
    fn decoded_to_value_temperature_to_real() {
        let v = decoded_to_value(
            &DecodedValue::Temperature(1234.5),
            FieldDataType::Real,
        );
        assert_eq!(v, Some(Value::Real(1234.5)));
    }

    #[test]
    fn decoded_to_value_per1000_to_real() {
        let v = decoded_to_value(
            &DecodedValue::Per1000(0.853),
            FieldDataType::Real,
        );
        assert_eq!(v, Some(Value::Real(0.853)));
    }

    #[test]
    fn decoded_to_value_bool_to_bool() {
        let v = decoded_to_value(&DecodedValue::Bool(true), FieldDataType::Bool);
        assert_eq!(v, Some(Value::Bool(true)));
    }

    #[test]
    fn decoded_to_value_rejects_mismatch() {
        // Temperature into a BOOL field — there's no sensible
        // mapping; return None so the runtime layer logs and skips.
        let v = decoded_to_value(&DecodedValue::Temperature(50.0), FieldDataType::Bool);
        assert!(v.is_none());
    }
}
