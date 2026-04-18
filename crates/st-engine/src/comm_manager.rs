//! Communication Manager: bridges CommDevices with the VM's global variables.
//!
//! Integrates into the scan cycle:
//! 1. read_inputs(): device → VM globals (before program execution)
//! 2. write_outputs(): VM globals → device (after program execution)

use crate::vm::Vm;
use st_comm_api::*;
use st_ir::Value;
use std::time::{Duration, Instant};

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
    /// Minimum interval between I/O updates. `None` means every scan cycle.
    cycle_time: Option<Duration>,
    /// When this device last executed I/O. `None` means never (first cycle always runs).
    last_io_time: Option<Instant>,
    /// Set by `read_inputs` when this device was read in the current cycle.
    /// Checked by `write_outputs` to keep reads and writes paired.
    ran_this_cycle: bool,
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
        cycle_time: Option<Duration>,
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

        self.device_entries.push(DeviceEntry {
            device,
            mappings,
            cycle_time,
            last_io_time: None,
            ran_this_cycle: false,
        });
    }

    /// Read input fields from devices and write them into VM globals.
    /// Called BEFORE each scan cycle. Devices with a `cycle_time` are only
    /// polled once the interval has elapsed; their VM globals hold the
    /// last-known value in between.
    pub fn read_inputs(&mut self, vm: &mut Vm) {
        let now = Instant::now();
        for entry in &mut self.device_entries {
            // Multi-rate gating: skip devices whose cycle_time hasn't elapsed.
            entry.ran_this_cycle = false;
            if let Some(ct) = entry.cycle_time {
                if let Some(last) = entry.last_io_time {
                    if now.duration_since(last) < ct {
                        continue;
                    }
                }
            }
            entry.ran_this_cycle = true;
            entry.last_io_time = Some(now);

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

    /// Write output fields from VM globals to devices.
    /// Called AFTER each scan cycle. Only writes to devices that were read
    /// in the current cycle (keeps reads and writes paired for multi-rate).
    pub fn write_outputs(&mut self, vm: &Vm) {
        for entry in &mut self.device_entries {
            if !entry.ran_this_cycle {
                continue;
            }
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

    /// Returns per-device diagnostics as `(name, diagnostics)` pairs.
    pub fn device_diagnostics(&self) -> Vec<(&str, st_comm_api::DeviceDiagnostics)> {
        self.device_entries
            .iter()
            .map(|e| (e.device.name(), e.device.diagnostics()))
            .collect()
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
            native_fb_indices: vec![],
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
        mgr.register_device(Box::new(StubDevice::new("ok_a", true)), "ok_a", &vm, None);
        mgr.register_device(Box::new(StubDevice::new("ok_b", true)), "ok_b", &vm, None);
        mgr.register_device(Box::new(StubDevice::new("down", false)), "down", &vm, None);

        assert_eq!(mgr.device_count(), 3);
        assert_eq!(mgr.health_counts(), (2, 1));
    }

    // ── Multi-rate scheduling tests ─────────────────────────────────────

    /// StubDevice that counts how many times read_inputs/write_outputs are called.
    struct CountingDevice {
        name: String,
        profile: DeviceProfile,
        read_count: Arc<std::sync::atomic::AtomicU32>,
        write_count: Arc<std::sync::atomic::AtomicU32>,
    }

    impl CountingDevice {
        fn new(name: &str) -> (Self, Arc<std::sync::atomic::AtomicU32>, Arc<std::sync::atomic::AtomicU32>) {
            let read_count = Arc::new(std::sync::atomic::AtomicU32::new(0));
            let write_count = Arc::new(std::sync::atomic::AtomicU32::new(0));
            let dev = Self {
                name: name.to_string(),
                profile: DeviceProfile {
                    name: name.to_string(),
                    vendor: None,
                    protocol: None,
                    description: None,
                    fields: vec![],
                },
                read_count: Arc::clone(&read_count),
                write_count: Arc::clone(&write_count),
            };
            (dev, read_count, write_count)
        }
    }

    impl CommDevice for CountingDevice {
        fn name(&self) -> &str { &self.name }
        fn protocol(&self) -> &str { "counting" }
        fn configure(&mut self, _: &serde_yaml::Value) -> Result<(), CommError> { Ok(()) }
        fn bind_link(&mut self, _: Arc<Mutex<dyn st_comm_api::CommLink>>) -> Result<(), CommError> { Ok(()) }
        fn device_profile(&self) -> &DeviceProfile { &self.profile }
        fn read_inputs(&mut self) -> Result<IoValues, CommError> {
            self.read_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(IoValues::new())
        }
        fn write_outputs(&mut self, _: &IoValues) -> Result<(), CommError> {
            self.write_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }
        fn acyclic_request(&mut self, _: AcyclicRequest) -> Result<AcyclicResponse, CommError> {
            Ok(AcyclicResponse { success: true, data: vec![], error: None })
        }
        fn is_connected(&self) -> bool { true }
        fn diagnostics(&self) -> DeviceDiagnostics { DeviceDiagnostics { connected: true, ..Default::default() } }
    }

    #[test]
    fn device_without_cycle_time_runs_every_cycle() {
        let mut vm = empty_vm();
        let mut mgr = CommManager::new();
        let (dev, reads, writes) = CountingDevice::new("fast");
        mgr.register_device(Box::new(dev), "fast", &vm, None);

        for _ in 0..5 {
            mgr.read_inputs(&mut vm);
            mgr.write_outputs(&vm);
        }

        assert_eq!(reads.load(std::sync::atomic::Ordering::SeqCst), 5);
        assert_eq!(writes.load(std::sync::atomic::Ordering::SeqCst), 5);
    }

    #[test]
    fn device_skipped_when_cycle_time_not_elapsed() {
        let mut vm = empty_vm();
        let mut mgr = CommManager::new();
        let (dev, reads, writes) = CountingDevice::new("slow");
        mgr.register_device(Box::new(dev), "slow", &vm, Some(Duration::from_millis(100)));

        // First call always runs (last_io_time is None)
        mgr.read_inputs(&mut vm);
        mgr.write_outputs(&vm);
        assert_eq!(reads.load(std::sync::atomic::Ordering::SeqCst), 1);

        // Second call immediately after — should be skipped
        mgr.read_inputs(&mut vm);
        mgr.write_outputs(&vm);
        assert_eq!(reads.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(writes.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn device_runs_after_cycle_time_elapsed() {
        let mut vm = empty_vm();
        let mut mgr = CommManager::new();
        let (dev, reads, _writes) = CountingDevice::new("slow");
        mgr.register_device(Box::new(dev), "slow", &vm, Some(Duration::from_millis(50)));

        mgr.read_inputs(&mut vm);
        assert_eq!(reads.load(std::sync::atomic::Ordering::SeqCst), 1);

        std::thread::sleep(Duration::from_millis(60));

        mgr.read_inputs(&mut vm);
        assert_eq!(reads.load(std::sync::atomic::Ordering::SeqCst), 2);
    }

    #[test]
    fn write_outputs_paired_with_read_inputs() {
        let mut vm = empty_vm();
        let mut mgr = CommManager::new();
        let (dev, reads, writes) = CountingDevice::new("slow");
        mgr.register_device(Box::new(dev), "slow", &vm, Some(Duration::from_millis(100)));

        // First cycle: both run
        mgr.read_inputs(&mut vm);
        mgr.write_outputs(&vm);
        assert_eq!(reads.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(writes.load(std::sync::atomic::Ordering::SeqCst), 1);

        // Second cycle: both skipped
        mgr.read_inputs(&mut vm);
        mgr.write_outputs(&vm);
        assert_eq!(reads.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(writes.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn mixed_rate_devices() {
        let mut vm = empty_vm();
        let mut mgr = CommManager::new();
        let (fast_dev, fast_reads, _) = CountingDevice::new("fast");
        let (slow_dev, slow_reads, _) = CountingDevice::new("slow");
        mgr.register_device(Box::new(fast_dev), "fast", &vm, None);
        mgr.register_device(Box::new(slow_dev), "slow", &vm, Some(Duration::from_millis(100)));

        // Run 5 fast cycles without sleeping
        for _ in 0..5 {
            mgr.read_inputs(&mut vm);
        }

        // Fast device ran every cycle, slow device ran only the first
        assert_eq!(fast_reads.load(std::sync::atomic::Ordering::SeqCst), 5);
        assert_eq!(slow_reads.load(std::sync::atomic::Ordering::SeqCst), 1);
    }
}
