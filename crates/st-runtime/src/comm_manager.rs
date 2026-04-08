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
