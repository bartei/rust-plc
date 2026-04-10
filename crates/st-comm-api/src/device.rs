//! Communication device trait — application/protocol layer.
//!
//! A device is an addressable unit on a link: a Modbus slave, a simulated
//! I/O board, a PROFINET device, etc. Each device has a profile that defines
//! its I/O fields and becomes a named global struct in ST code.

use crate::error::{AcyclicRequest, AcyclicResponse, CommError, DeviceDiagnostics, IoValues};
use crate::link::CommLink;
use crate::profile::DeviceProfile;
use std::sync::{Arc, Mutex};

/// Application-layer communication device.
///
/// Each device instance corresponds to one entry in the `devices:` section
/// of `plc-project.yaml`. Its `name` becomes the global struct variable name
/// in ST code.
pub trait CommDevice: Send + Sync {
    /// Device instance name (from YAML config). Becomes the global variable name.
    fn name(&self) -> &str;

    /// Protocol identifier: "modbus-tcp", "modbus-rtu", "simulated", etc.
    fn protocol(&self) -> &str;

    /// Configure the device from its YAML config section.
    fn configure(&mut self, config: &serde_yaml::Value) -> Result<(), CommError>;

    /// Bind this device to a communication link.
    /// For simulated devices, this may be a no-op (they use internal state).
    fn bind_link(&mut self, link: Arc<Mutex<dyn CommLink>>) -> Result<(), CommError>;

    /// Return the device profile (struct schema + register map).
    fn device_profile(&self) -> &DeviceProfile;

    /// Cyclic I/O: read input fields from the physical device.
    /// Returns a map of field_name → value for all input-direction fields.
    /// Called by the communication manager BEFORE each scan cycle.
    fn read_inputs(&mut self) -> Result<IoValues, CommError>;

    /// Cyclic I/O: write output field values to the physical device.
    /// Receives a map of field_name → value for all output-direction fields.
    /// Called by the communication manager AFTER each scan cycle.
    fn write_outputs(&mut self, outputs: &IoValues) -> Result<(), CommError>;

    /// Acyclic (on-demand) request: read/write individual registers.
    fn acyclic_request(
        &mut self,
        request: AcyclicRequest,
    ) -> Result<AcyclicResponse, CommError>;

    /// Whether the device is currently connected and responding.
    fn is_connected(&self) -> bool;

    /// Current diagnostics for this device.
    fn diagnostics(&self) -> DeviceDiagnostics;
}
