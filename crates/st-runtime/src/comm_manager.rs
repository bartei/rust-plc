//! Communication Manager: bridges CommDevices with the VM's global variables.
//!
//! Integrates into the scan cycle:
//! 1. read_inputs(): device → VM globals (before program execution)
//! 2. write_outputs(): VM globals → device (after program execution)

use crate::vm::Vm;
use st_comm_api::*;
use st_ir::Value;

/// Maps device fields to VM global variable slots.
#[derive(Debug, Clone)]
struct FieldMapping {
    /// Global variable slot index in the VM.
    global_slot: u16,
    /// Field name in the device profile.
    field_name: String,
    /// Field direction.
    direction: FieldDirection,
    /// Field data type.
    data_type: FieldDataType,
}

/// Manages all communication devices and maps their I/O to VM globals.
pub struct CommManager {
    /// Registered devices with their field mappings.
    device_entries: Vec<DeviceEntry>,
}

struct DeviceEntry {
    device: Box<dyn CommDevice>,
    /// Maps device field names to VM global slots.
    mappings: Vec<FieldMapping>,
}

impl CommManager {
    pub fn new() -> Self {
        Self {
            device_entries: Vec::new(),
        }
    }

    /// Register a device with field-to-global mappings.
    /// `instance_name` is the device name from `plc-project.yaml` (e.g., "io_rack").
    /// Each profile field is mapped to a flat global named `{instance_name}_{field}`.
    pub fn register_device(
        &mut self,
        device: Box<dyn CommDevice>,
        instance_name: &str,
        vm: &Vm,
    ) {
        let profile = device.device_profile().clone();
        let mut mappings = Vec::new();

        for field in &profile.fields {
            let global_name = format!("{instance_name}_{}", field.name);
            if let Some((slot, _)) = vm.module().globals.find_slot(&global_name) {
                mappings.push(FieldMapping {
                    global_slot: slot,
                    field_name: field.name.clone(),
                    direction: field.direction,
                    data_type: field.data_type,
                });
            } else {
                eprintln!(
                    "[COMM] warning: device '{instance_name}' field '{}' not found in globals (expected '{global_name}')",
                    field.name
                );
            }
        }

        self.device_entries.push(DeviceEntry { device, mappings });
    }

    /// Read all input fields from devices and write them into VM globals.
    /// Called BEFORE each scan cycle.
    pub fn read_inputs(&mut self, vm: &mut Vm) {
        for entry in &mut self.device_entries {
            match entry.device.read_inputs() {
                Ok(values) => {
                    for mapping in &entry.mappings {
                        if !matches!(
                            mapping.direction,
                            FieldDirection::Input | FieldDirection::Inout
                        ) {
                            continue;
                        }
                        if let Some(io_val) = values.get(&mapping.field_name) {
                            let vm_val = io_value_to_vm_value(io_val, mapping.data_type);
                            vm.set_global_by_slot(mapping.global_slot, vm_val);
                        }
                    }
                }
                Err(e) => {
                    eprintln!(
                        "[COMM] Error reading inputs from '{}': {e}",
                        entry.device.name()
                    );
                }
            }
        }
    }

    /// Read all output fields from VM globals and write them to devices.
    /// Called AFTER each scan cycle.
    pub fn write_outputs(&mut self, vm: &Vm) {
        for entry in &mut self.device_entries {
            let mut outputs = IoValues::new();
            for mapping in &entry.mappings {
                if !matches!(
                    mapping.direction,
                    FieldDirection::Output | FieldDirection::Inout
                ) {
                    continue;
                }
                if let Some(vm_val) = vm.get_global_by_slot(mapping.global_slot) {
                    let io_val = vm_value_to_io_value(vm_val, mapping.data_type);
                    outputs.insert(mapping.field_name.clone(), io_val);
                }
            }
            if let Err(e) = entry.device.write_outputs(&outputs) {
                eprintln!(
                    "[COMM] Error writing outputs to '{}': {e}",
                    entry.device.name()
                );
            }
        }
    }

    /// Number of registered devices.
    pub fn device_count(&self) -> usize {
        self.device_entries.len()
    }

    /// Returns `(healthy, error)` counts based on each device's `is_connected()`.
    /// Used by diagnostic surfaces (status bar, monitor server) without exposing
    /// the device list itself.
    pub fn health_counts(&self) -> (u32, u32) {
        let mut ok = 0u32;
        let mut err = 0u32;
        for entry in &self.device_entries {
            if entry.device.is_connected() {
                ok += 1;
            } else {
                err += 1;
            }
        }
        (ok, err)
    }
}

impl Default for CommManager {
    fn default() -> Self {
        Self::new()
    }
}

fn io_value_to_vm_value(io: &IoValue, _data_type: FieldDataType) -> Value {
    match io {
        IoValue::Bool(b) => Value::Bool(*b),
        IoValue::Int(i) => Value::Int(*i),
        IoValue::UInt(u) => Value::UInt(*u),
        IoValue::Real(r) => Value::Real(*r),
        IoValue::String(s) => Value::String(s.clone()),
    }
}

fn vm_value_to_io_value(vm: &Value, data_type: FieldDataType) -> IoValue {
    match data_type {
        FieldDataType::Bool => IoValue::Bool(vm.as_bool()),
        FieldDataType::Real | FieldDataType::Lreal => IoValue::Real(vm.as_real()),
        FieldDataType::String => {
            if let Value::String(s) = vm {
                IoValue::String(s.clone())
            } else {
                IoValue::String(String::new())
            }
        }
        _ => IoValue::Int(vm.as_int()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::{Vm, VmConfig};
    use st_comm_api::{
        AcyclicRequest, AcyclicResponse, CommError, DeviceDiagnostics, DeviceProfile, IoValues,
    };
    use st_ir::{MemoryLayout, Module};
    use std::sync::{Arc, Mutex};

    /// Minimal CommDevice stub for testing CommManager bookkeeping. Carries
    /// only the bits relevant to `health_counts()`.
    struct StubDevice {
        name: String,
        connected: bool,
        profile: DeviceProfile,
    }

    impl StubDevice {
        fn new(name: &str, connected: bool) -> Self {
            Self {
                name: name.to_string(),
                connected,
                profile: DeviceProfile {
                    name: name.to_string(),
                    vendor: None,
                    protocol: None,
                    description: None,
                    fields: vec![],
                },
            }
        }
    }

    impl CommDevice for StubDevice {
        fn name(&self) -> &str {
            &self.name
        }
        fn protocol(&self) -> &str {
            "stub"
        }
        fn configure(&mut self, _: &serde_yaml::Value) -> Result<(), CommError> {
            Ok(())
        }
        fn bind_link(
            &mut self,
            _: Arc<Mutex<dyn st_comm_api::CommLink>>,
        ) -> Result<(), CommError> {
            Ok(())
        }
        fn device_profile(&self) -> &DeviceProfile {
            &self.profile
        }
        fn read_inputs(&mut self) -> Result<IoValues, CommError> {
            Ok(IoValues::new())
        }
        fn write_outputs(&mut self, _: &IoValues) -> Result<(), CommError> {
            Ok(())
        }
        fn acyclic_request(
            &mut self,
            _: AcyclicRequest,
        ) -> Result<AcyclicResponse, CommError> {
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
                ..Default::default()
            }
        }
    }

    fn empty_vm() -> Vm {
        let module = Module {
            functions: vec![],
            globals: MemoryLayout::default(),
            type_defs: vec![],
        };
        Vm::new(module, VmConfig::default())
    }

    #[test]
    fn health_counts_empty_manager() {
        let mgr = CommManager::new();
        assert_eq!(mgr.device_count(), 0);
        assert_eq!(mgr.health_counts(), (0, 0));
    }

    #[test]
    fn health_counts_mixed_devices() {
        let vm = empty_vm();
        let mut mgr = CommManager::new();
        mgr.register_device(Box::new(StubDevice::new("ok_a", true)), "ok_a", &vm);
        mgr.register_device(Box::new(StubDevice::new("ok_b", true)), "ok_b", &vm);
        mgr.register_device(Box::new(StubDevice::new("down", false)), "down", &vm);

        assert_eq!(mgr.device_count(), 3);
        assert_eq!(mgr.health_counts(), (2, 1));
    }
}
