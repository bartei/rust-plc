//! Simulated device — in-memory register storage implementing CommDevice.

use st_comm_api::*;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// A simulated communication device with in-memory registers.
///
/// Input values can be set externally (via the web UI or programmatically).
/// Output values are written by the PLC program and can be read back.
/// State is stored as `IoValue` keyed by field name.
pub struct SimulatedDevice {
    name: String,
    profile: DeviceProfile,
    /// Current values for all fields (inputs + outputs).
    state: Arc<Mutex<HashMap<String, IoValue>>>,
    connected: bool,
    cycle_count: u64,
}

impl SimulatedDevice {
    /// Create a new simulated device from a device profile.
    pub fn new(name: &str, profile: DeviceProfile) -> Self {
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
            name: name.to_string(),
            profile,
            state: Arc::new(Mutex::new(initial_state)),
            connected: true,
            cycle_count: 0,
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

impl CommDevice for SimulatedDevice {
    fn name(&self) -> &str {
        &self.name
    }

    fn protocol(&self) -> &str {
        "simulated"
    }

    fn configure(&mut self, _config: &serde_yaml::Value) -> Result<(), CommError> {
        Ok(())
    }

    fn bind_link(&mut self, _link: Arc<Mutex<dyn CommLink>>) -> Result<(), CommError> {
        // Simulated devices don't need a real link
        Ok(())
    }

    fn device_profile(&self) -> &DeviceProfile {
        &self.profile
    }

    fn read_inputs(&mut self) -> Result<IoValues, CommError> {
        self.cycle_count += 1;
        let state = self.state.lock().unwrap();
        let mut inputs = IoValues::new();
        for field in self.profile.input_fields() {
            if let Some(value) = state.get(&field.name) {
                inputs.insert(field.name.clone(), value.clone());
            }
        }
        Ok(inputs)
    }

    fn write_outputs(&mut self, outputs: &IoValues) -> Result<(), CommError> {
        let mut state = self.state.lock().unwrap();
        for (name, value) in outputs {
            // Only write output-direction fields
            let is_output = self.profile.fields.iter().any(|f| {
                f.name.eq_ignore_ascii_case(name)
                    && matches!(f.direction, FieldDirection::Output | FieldDirection::Inout)
            });
            if is_output {
                state.insert(name.clone(), value.clone());
            }
        }
        Ok(())
    }

    fn acyclic_request(&mut self, _request: AcyclicRequest) -> Result<AcyclicResponse, CommError> {
        Ok(AcyclicResponse {
            success: true,
            data: vec![],
            error: None,
        })
    }

    fn is_connected(&self) -> bool {
        self.connected
    }

    fn diagnostics(&self) -> DeviceDiagnostics {
        DeviceDiagnostics {
            connected: self.connected,
            error: false,
            error_count: 0,
            successful_cycles: self.cycle_count,
            last_response_ms: 0,
            last_error: None,
        }
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
    fn read_inputs_returns_only_inputs() {
        let mut dev = SimulatedDevice::new("test", test_profile());
        dev.set_input("DI_0", IoValue::Bool(true)).unwrap();

        let inputs = dev.read_inputs().unwrap();
        assert!(inputs.contains_key("DI_0"));
        assert!(inputs.contains_key("AI_0"));
        assert!(!inputs.contains_key("DO_0")); // output, not in inputs
        assert!(!inputs.contains_key("AO_0"));
    }

    #[test]
    fn write_outputs_updates_state() {
        let mut dev = SimulatedDevice::new("test", test_profile());
        let mut outputs = IoValues::new();
        outputs.insert("DO_0".to_string(), IoValue::Bool(true));
        outputs.insert("AO_0".to_string(), IoValue::Int(750));

        dev.write_outputs(&outputs).unwrap();

        assert_eq!(dev.get_output("DO_0"), Some(IoValue::Bool(true)));
        assert_eq!(dev.get_output("AO_0"), Some(IoValue::Int(750)));
    }

    #[test]
    fn write_outputs_ignores_input_fields() {
        let mut dev = SimulatedDevice::new("test", test_profile());
        dev.set_input("DI_0", IoValue::Bool(true)).unwrap();

        // Try to overwrite an input via write_outputs — should be ignored
        let mut outputs = IoValues::new();
        outputs.insert("DI_0".to_string(), IoValue::Bool(false));
        dev.write_outputs(&outputs).unwrap();

        // DI_0 should still be true (not overwritten)
        assert_eq!(dev.get_output("DI_0"), Some(IoValue::Bool(true)));
    }

    #[test]
    fn diagnostics_track_cycles() {
        let mut dev = SimulatedDevice::new("test", test_profile());
        assert_eq!(dev.diagnostics().successful_cycles, 0);

        dev.read_inputs().unwrap();
        dev.read_inputs().unwrap();
        dev.read_inputs().unwrap();

        assert_eq!(dev.diagnostics().successful_cycles, 3);
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
