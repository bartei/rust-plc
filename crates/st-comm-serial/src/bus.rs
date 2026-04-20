//! Shared bus manager for serial communication.
//!
//! Provides one background I/O thread per serial port. Devices register
//! themselves on a bus; the bus thread round-robins through registered
//! devices, respecting each device's refresh rate. This prevents bus
//! contention when multiple devices share the same RS-485 line.
//!
//! The bus manager is protocol-agnostic — protocol-specific I/O is
//! performed by the [`BusDeviceIo`] trait, implemented by each protocol
//! crate (e.g., st-comm-modbus).

use crate::shared::TransportMap;
use crate::transport::SerialTransport;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Trait for protocol-specific I/O on a serial bus device.
///
/// Implementors perform their protocol's read/write operations using the
/// provided serial transport. The bus thread calls `poll()` when the device
/// is due for I/O (based on its refresh rate).
pub trait BusDeviceIo: Send + 'static {
    /// Perform one I/O cycle: read inputs, write outputs.
    ///
    /// `transport` is the shared serial transport for the bus (already locked
    /// by the bus thread — no other device is using it concurrently).
    ///
    /// Returns `true` if the I/O cycle succeeded (device is connected).
    fn poll(&mut self, transport: &Arc<Mutex<SerialTransport>>) -> bool;
}

/// A registered device on a bus.
struct BusEntry {
    io: Box<dyn BusDeviceIo>,
    interval: Duration,
    last_io: Option<Instant>,
}

/// A shared bus: one I/O thread per serial port.
struct Bus {
    devices: Mutex<Vec<BusEntry>>,
    thread_started: Mutex<bool>,
}

/// Manages all serial buses. One bus per port path, one thread per bus.
///
/// Shared across all device FBs via `Arc<BusManager>`. Devices register
/// themselves on first `execute()` call; the bus thread starts automatically
/// when the first device registers and the transport is available.
pub struct BusManager {
    buses: Mutex<HashMap<String, Arc<Bus>>>,
    transport_map: Arc<TransportMap>,
}

impl BusManager {
    /// Create a new bus manager that looks up transports from the given map.
    pub fn new(transport_map: Arc<TransportMap>) -> Self {
        Self {
            buses: Mutex::new(HashMap::new()),
            transport_map,
        }
    }

    /// Register a device on the bus for the given port path.
    ///
    /// If no bus thread exists for this port yet, one is spawned. The bus
    /// thread waits for the transport to appear in the TransportMap (opened
    /// by a SerialLink FB) before starting I/O.
    pub fn register(
        &self,
        port_path: &str,
        interval: Duration,
        io: Box<dyn BusDeviceIo>,
    ) {
        let mut buses = self.buses.lock().unwrap();

        let bus = buses
            .entry(port_path.to_string())
            .or_insert_with(|| {
                Arc::new(Bus {
                    devices: Mutex::new(Vec::new()),
                    thread_started: Mutex::new(false),
                })
            });

        // Add the device
        {
            let mut devices = bus.devices.lock().unwrap();
            devices.push(BusEntry {
                io,
                interval,
                last_io: None,
            });
        }

        // Start the bus thread if not already running
        let mut started = bus.thread_started.lock().unwrap();
        if !*started {
            let bus_clone = Arc::clone(bus);
            let transport_map = Arc::clone(&self.transport_map);
            let port_name = port_path.to_string();

            std::thread::Builder::new()
                .name(format!("serial-bus-{port_name}"))
                .spawn(move || bus_thread_loop(&port_name, &bus_clone, &transport_map))
                .expect("Failed to spawn serial bus thread");

            *started = true;
        }
    }
}

/// Main loop for a bus thread. Waits for the transport to become available,
/// then round-robins through registered devices.
fn bus_thread_loop(
    port_name: &str,
    bus: &Arc<Bus>,
    transport_map: &Arc<TransportMap>,
) {
    // Wait for the transport to appear in the map (SerialLink opens it)
    let transport = loop {
        if let Ok(map) = transport_map.lock() {
            if let Some(t) = map.get(port_name) {
                break Arc::clone(t);
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    };

    tracing::info!("Serial bus thread started for {port_name}");

    loop {
        let mut any_polled = false;
        let mut devices = bus.devices.lock().unwrap();

        for entry in devices.iter_mut() {
            // Check if this device is due for polling
            let due = match entry.last_io {
                None => true,
                Some(last) => last.elapsed() >= entry.interval,
            };
            if !due {
                continue;
            }

            any_polled = true;
            entry.io.poll(&transport);
            entry.last_io = Some(Instant::now());
        }

        drop(devices); // Release lock before sleeping

        if !any_polled {
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}
