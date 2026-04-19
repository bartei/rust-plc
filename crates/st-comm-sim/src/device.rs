//! Simulated device — in-memory register storage implementing CommDevice and NativeFb.

use st_comm_api::*;
use st_ir::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// A simulated communication device with in-memory registers.
///
/// Input values can be set externally (via the web UI or programmatically).
/// Output values are written by the PLC program and can be read back.
/// State is stored as `IoValue` keyed by field name.
pub struct SimulatedDevice {
    profile: DeviceProfile,
    /// Current values for all fields (inputs + outputs).
    state: Arc<Mutex<HashMap<String, IoValue>>>,
}

impl SimulatedDevice {
    /// Create a new simulated device from a device profile.
    pub fn new(_name: &str, profile: DeviceProfile) -> Self {
        // Initialize all fields with defaults
        let mut initial_state = HashMap::new();
        for field in &profile.fields {
            let default = match field.data_type {
                FieldDataType::Bool => IoValue::Bool(false),
                FieldDataType::Real | FieldDataType::Lreal => IoValue::Real(0.0),
                FieldDataType::String => IoValue::String(String::new()),
                _ => IoValue::Int(0),
            };
            initial_state.insert(field.name.clone(), default);
        }

        Self {
            profile,
            state: Arc::new(Mutex::new(initial_state)),
        }
    }

    /// Get the shared state handle (used by the web UI to read/write values).
    pub fn state_handle(&self) -> Arc<Mutex<HashMap<String, IoValue>>> {
        Arc::clone(&self.state)
    }

    /// Set an input value externally (e.g., from the web UI).
    pub fn set_input(&self, field_name: &str, value: IoValue) -> Result<(), CommError> {
        // Verify the field exists and is an input
        let field = self.profile.fields.iter()
            .find(|f| f.name.eq_ignore_ascii_case(field_name))
            .ok_or_else(|| CommError::InvalidConfig(format!("Unknown field: {field_name}")))?;

        if field.direction == FieldDirection::Output {
            return Err(CommError::InvalidConfig(
                format!("Cannot set output field '{field_name}' as input"),
            ));
        }

        let mut state = self.state.lock().unwrap();
        state.insert(field.name.clone(), value);
        Ok(())
    }

    /// Read an output value (e.g., for the web UI to display).
    pub fn get_output(&self, field_name: &str) -> Option<IoValue> {
        let state = self.state.lock().unwrap();
        state.get(field_name).cloned()
    }

    /// Get all current values (for the web UI).
    pub fn get_all_values(&self) -> HashMap<String, IoValue> {
        self.state.lock().unwrap().clone()
    }
}

// =========================================================================
// NativeFb implementation — allows SimulatedDevice to be used as an ST
// function block via the new native FB dispatch mechanism.
// =========================================================================

/// Number of fixed fields before the profile fields in the NativeFb layout.
/// These are: refresh_rate, connected, error_code, io_cycles, last_response_ms.
const DIAG_FIELD_COUNT: usize = 5;

impl NativeFb for SimulatedDevice {
    fn type_name(&self) -> &str {
        &self.profile.name
    }

    fn layout(&self) -> &NativeFbLayout {
        // Generate on demand. In a production setup this could be cached,
        // but `to_native_fb_layout()` is cheap and only called at compile/analysis time.
        // For the trait we need to return a reference, so we store it.
        // Actually, we can't return a reference to a temporary. Let's cache it.
        // For now, leak a Box (this is called a small number of times at startup).
        // A better approach is to store it in the struct.
        unreachable!("layout() on SimulatedDevice should not be called at runtime; use cached_layout()")
    }

    fn execute(&self, fields: &mut [Value]) {
        // Field layout (from DeviceProfile::to_native_fb_layout):
        //   [0] refresh_rate : TIME (VarInput)
        //   [1] connected    : BOOL (Var)
        //   [2] error_code   : INT  (Var)
        //   [3] io_cycles    : UDINT (Var)
        //   [4] last_response_ms : REAL (Var)
        //   [5..] profile fields (Var)

        let mut state = self.state.lock().unwrap();

        // Read input-direction fields from shared state → fields slice
        for (i, pf) in self.profile.fields.iter().enumerate() {
            let slot = DIAG_FIELD_COUNT + i;
            if slot >= fields.len() {
                break;
            }
            if matches!(pf.direction, FieldDirection::Input | FieldDirection::Inout) {
                if let Some(io_val) = state.get(&pf.name) {
                    fields[slot] = io_value_to_vm_value(io_val);
                }
            }
        }

        // Read output-direction fields from fields slice → shared state
        for (i, pf) in self.profile.fields.iter().enumerate() {
            let slot = DIAG_FIELD_COUNT + i;
            if slot >= fields.len() {
                break;
            }
            if matches!(pf.direction, FieldDirection::Output | FieldDirection::Inout) {
                state.insert(pf.name.clone(), vm_value_to_io_value(&fields[slot], pf.data_type));
            }
        }

        // Update diagnostics
        fields[1] = Value::Bool(true); // connected
        fields[2] = Value::Int(0);     // error_code
        let cycles = fields[3].as_int() as u64 + 1;
        fields[3] = Value::UInt(cycles);
        fields[4] = Value::Real(0.0);  // last_response_ms (simulated = instant)
    }
}

/// A wrapper that provides a cached `NativeFbLayout` and delegates to `SimulatedDevice`.
/// This is needed because `NativeFb::layout()` returns `&NativeFbLayout` (a reference),
/// and `SimulatedDevice` doesn't store the layout internally.
pub struct SimulatedNativeFb {
    device: SimulatedDevice,
    layout: NativeFbLayout,
}

impl SimulatedNativeFb {
    pub fn new(name: &str, profile: DeviceProfile) -> Self {
        let layout = profile.to_native_fb_layout();
        let device = SimulatedDevice::new(name, profile);
        Self { device, layout }
    }

    /// Get the shared state handle (for the web UI).
    pub fn state_handle(&self) -> Arc<Mutex<HashMap<String, IoValue>>> {
        self.device.state_handle()
    }

    /// Get the device profile.
    pub fn profile(&self) -> &DeviceProfile {
        &self.device.profile
    }
}

impl NativeFb for SimulatedNativeFb {
    fn type_name(&self) -> &str {
        &self.layout.type_name
    }

    fn layout(&self) -> &NativeFbLayout {
        &self.layout
    }

    fn execute(&self, fields: &mut [Value]) {
        self.device.execute(fields);
    }
}

/// Convert an `IoValue` to an `st_ir::Value`.
fn io_value_to_vm_value(io: &IoValue) -> Value {
    match io {
        IoValue::Bool(b) => Value::Bool(*b),
        IoValue::Int(i) => Value::Int(*i),
        IoValue::UInt(u) => Value::UInt(*u),
        IoValue::Real(r) => Value::Real(*r),
        IoValue::String(s) => Value::String(s.clone()),
    }
}

/// Convert an `st_ir::Value` to an `IoValue`, using the field's data type to
/// choose the right IoValue variant.
fn vm_value_to_io_value(val: &Value, dt: FieldDataType) -> IoValue {
    match dt {
        FieldDataType::Bool => IoValue::Bool(val.as_bool()),
        FieldDataType::Real | FieldDataType::Lreal => IoValue::Real(val.as_real()),
        FieldDataType::String => {
            if let Value::String(s) = val {
                IoValue::String(s.clone())
            } else {
                IoValue::String(String::new())
            }
        }
        FieldDataType::Usint | FieldDataType::Uint | FieldDataType::Udint | FieldDataType::Ulint
        | FieldDataType::Byte | FieldDataType::Word | FieldDataType::Dword | FieldDataType::Lword => {
            match val {
                Value::UInt(u) => IoValue::UInt(*u),
                Value::Int(i) => IoValue::UInt(*i as u64),
                _ => IoValue::UInt(0),
            }
        }
        _ => IoValue::Int(val.as_int()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_profile() -> DeviceProfile {
        DeviceProfile::from_yaml(r#"
name: TestIO
fields:
  - { name: DI_0, type: BOOL, direction: input, register: { address: 0, kind: virtual } }
  - { name: DI_1, type: BOOL, direction: input, register: { address: 1, kind: virtual } }
  - { name: AI_0, type: INT, direction: input, register: { address: 10, kind: virtual } }
  - { name: DO_0, type: BOOL, direction: output, register: { address: 20, kind: virtual } }
  - { name: AO_0, type: INT, direction: output, register: { address: 30, kind: virtual } }
"#).unwrap()
    }

    #[test]
    fn device_initializes_with_defaults() {
        let dev = SimulatedDevice::new("test", test_profile());
        let values = dev.get_all_values();
        assert_eq!(values.get("DI_0"), Some(&IoValue::Bool(false)));
        assert_eq!(values.get("AI_0"), Some(&IoValue::Int(0)));
        assert_eq!(values.get("DO_0"), Some(&IoValue::Bool(false)));
    }

    #[test]
    fn set_input_and_read_back() {
        let dev = SimulatedDevice::new("test", test_profile());
        dev.set_input("DI_0", IoValue::Bool(true)).unwrap();
        dev.set_input("AI_0", IoValue::Int(500)).unwrap();

        let values = dev.get_all_values();
        assert_eq!(values.get("DI_0"), Some(&IoValue::Bool(true)));
        assert_eq!(values.get("AI_0"), Some(&IoValue::Int(500)));
    }

    #[test]
    fn cannot_set_output_as_input() {
        let dev = SimulatedDevice::new("test", test_profile());
        let result = dev.set_input("DO_0", IoValue::Bool(true));
        assert!(result.is_err());
    }

    #[test]
    fn state_handle_is_shared() {
        let dev = SimulatedDevice::new("test", test_profile());
        let handle = dev.state_handle();

        // Set via handle (simulating web UI)
        handle.lock().unwrap().insert("DI_0".to_string(), IoValue::Bool(true));

        // Read via device
        assert_eq!(dev.get_output("DI_0"), Some(IoValue::Bool(true)));
    }
}
