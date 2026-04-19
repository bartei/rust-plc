//! Integration test: full Modbus RTU stack over virtual serial ports.
//!
//! Spawns a socat virtual serial pair, runs a Modbus slave simulator
//! on one end, and exercises SerialTransport → RtuClient → read/write
//! registers on the other end.
//!
//! Requires `socat` (via `nix-shell -p socat pkg-config systemdLibs`).

use std::io::{Read, Write};
use std::process::{Child, Command};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use st_comm_modbus::crc;
use st_comm_modbus::rtu_client::RtuClient;
use st_comm_serial::transport::{SerialConfig, SerialTransport};

fn socat_available() -> bool {
    Command::new("socat")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn spawn_virtual_serial() -> (Child, String, String) {
    let port_a = format!("/tmp/st-modbus-a-{}", std::process::id());
    let port_b = format!("/tmp/st-modbus-b-{}", std::process::id());
    let _ = std::fs::remove_file(&port_a);
    let _ = std::fs::remove_file(&port_b);

    let child = Command::new("socat")
        .args([
            &format!("pty,raw,echo=0,link={port_a}"),
            &format!("pty,raw,echo=0,link={port_b}"),
        ])
        .spawn()
        .expect("Failed to spawn socat");

    for _ in 0..50 {
        if std::path::Path::new(&port_a).exists() && std::path::Path::new(&port_b).exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(std::path::Path::new(&port_a).exists(), "socat didn't create {port_a}");
    (child, port_a, port_b)
}

// ── Modbus RTU Slave Simulator ──────────────────────────────────────────

/// A simple Modbus RTU slave with in-memory registers.
struct ModbusSlave {
    slave_id: u8,
    coils: Vec<bool>,           // FC01/FC05/FC0F
    discrete_inputs: Vec<bool>, // FC02
    holding_registers: Vec<u16>,// FC03/FC06/FC10
    input_registers: Vec<u16>,  // FC04
}

impl ModbusSlave {
    fn new(slave_id: u8) -> Self {
        Self {
            slave_id,
            coils: vec![false; 100],
            discrete_inputs: vec![false; 100],
            holding_registers: vec![0u16; 100],
            input_registers: vec![0u16; 100],
        }
    }

    /// Process a Modbus RTU request and return the response frame.
    fn process_request(&mut self, request: &[u8]) -> Option<Vec<u8>> {
        if request.len() < 4 {
            return None;
        }
        if !crc::verify_crc(request) {
            eprintln!("[SLAVE] CRC mismatch on request");
            return None;
        }
        if request[0] != self.slave_id {
            return None; // Not for us
        }

        let fc = request[1];
        let result = match fc {
            0x01 => self.handle_read_coils(request),
            0x02 => self.handle_read_discrete_inputs(request),
            0x03 => self.handle_read_holding_registers(request),
            0x04 => self.handle_read_input_registers(request),
            0x05 => self.handle_write_single_coil(request),
            0x06 => self.handle_write_single_register(request),
            0x0F => self.handle_write_multiple_coils(request),
            0x10 => self.handle_write_multiple_registers(request),
            _ => self.build_exception(fc, 0x01), // Illegal function
        };
        Some(result)
    }

    fn handle_read_coils(&self, req: &[u8]) -> Vec<u8> {
        let start = ((req[2] as u16) << 8) | req[3] as u16;
        let count = ((req[4] as u16) << 8) | req[5] as u16;
        let byte_count = count.div_ceil(8) as u8;
        let mut data = Vec::new();
        for i in 0..byte_count as u16 {
            let mut byte = 0u8;
            for bit in 0..8u16 {
                let idx = start + i * 8 + bit;
                if (idx - start) < count && (idx as usize) < self.coils.len() && self.coils[idx as usize] {
                    byte |= 1 << bit;
                }
            }
            data.push(byte);
        }
        self.build_read_response(0x01, &data)
    }

    fn handle_read_discrete_inputs(&self, req: &[u8]) -> Vec<u8> {
        let start = ((req[2] as u16) << 8) | req[3] as u16;
        let count = ((req[4] as u16) << 8) | req[5] as u16;
        let byte_count = count.div_ceil(8) as u8;
        let mut data = Vec::new();
        for i in 0..byte_count as u16 {
            let mut byte = 0u8;
            for bit in 0..8u16 {
                let idx = start + i * 8 + bit;
                if (idx - start) < count && (idx as usize) < self.discrete_inputs.len() && self.discrete_inputs[idx as usize] {
                    byte |= 1 << bit;
                }
            }
            data.push(byte);
        }
        self.build_read_response(0x02, &data)
    }

    fn handle_read_holding_registers(&self, req: &[u8]) -> Vec<u8> {
        let start = ((req[2] as u16) << 8) | req[3] as u16;
        let count = ((req[4] as u16) << 8) | req[5] as u16;
        let mut data = Vec::new();
        for i in 0..count {
            let idx = (start + i) as usize;
            let val = if idx < self.holding_registers.len() { self.holding_registers[idx] } else { 0 };
            data.push((val >> 8) as u8);
            data.push((val & 0xFF) as u8);
        }
        self.build_read_response(0x03, &data)
    }

    fn handle_read_input_registers(&self, req: &[u8]) -> Vec<u8> {
        let start = ((req[2] as u16) << 8) | req[3] as u16;
        let count = ((req[4] as u16) << 8) | req[5] as u16;
        let mut data = Vec::new();
        for i in 0..count {
            let idx = (start + i) as usize;
            let val = if idx < self.input_registers.len() { self.input_registers[idx] } else { 0 };
            data.push((val >> 8) as u8);
            data.push((val & 0xFF) as u8);
        }
        self.build_read_response(0x04, &data)
    }

    fn handle_write_single_coil(&mut self, req: &[u8]) -> Vec<u8> {
        let addr = ((req[2] as u16) << 8) | req[3] as u16;
        let value = req[4] == 0xFF;
        if (addr as usize) < self.coils.len() {
            self.coils[addr as usize] = value;
        }
        // Echo the request (minus CRC, add new CRC)
        let mut resp = req[..6].to_vec();
        let (lo, hi) = crc::crc16(&resp);
        resp.push(lo);
        resp.push(hi);
        resp
    }

    fn handle_write_single_register(&mut self, req: &[u8]) -> Vec<u8> {
        let addr = ((req[2] as u16) << 8) | req[3] as u16;
        let value = ((req[4] as u16) << 8) | req[5] as u16;
        if (addr as usize) < self.holding_registers.len() {
            self.holding_registers[addr as usize] = value;
        }
        let mut resp = req[..6].to_vec();
        let (lo, hi) = crc::crc16(&resp);
        resp.push(lo);
        resp.push(hi);
        resp
    }

    fn handle_write_multiple_coils(&mut self, req: &[u8]) -> Vec<u8> {
        let start = ((req[2] as u16) << 8) | req[3] as u16;
        let count = ((req[4] as u16) << 8) | req[5] as u16;
        let _byte_count = req[6];
        for i in 0..count {
            let byte_idx = 7 + (i / 8) as usize;
            let bit_idx = i % 8;
            if byte_idx < req.len() - 2 {
                let val = req[byte_idx] & (1 << bit_idx) != 0;
                let addr = (start + i) as usize;
                if addr < self.coils.len() {
                    self.coils[addr] = val;
                }
            }
        }
        // Response: echo slave_id, fc, start_addr, count
        let mut resp = req[..6].to_vec();
        let (lo, hi) = crc::crc16(&resp);
        resp.push(lo);
        resp.push(hi);
        resp
    }

    fn handle_write_multiple_registers(&mut self, req: &[u8]) -> Vec<u8> {
        let start = ((req[2] as u16) << 8) | req[3] as u16;
        let count = ((req[4] as u16) << 8) | req[5] as u16;
        for i in 0..count {
            let offset = 7 + (i as usize) * 2;
            if offset + 1 < req.len() - 2 {
                let val = ((req[offset] as u16) << 8) | req[offset + 1] as u16;
                let addr = (start + i) as usize;
                if addr < self.holding_registers.len() {
                    self.holding_registers[addr] = val;
                }
            }
        }
        let mut resp = req[..6].to_vec();
        let (lo, hi) = crc::crc16(&resp);
        resp.push(lo);
        resp.push(hi);
        resp
    }

    fn build_read_response(&self, fc: u8, data: &[u8]) -> Vec<u8> {
        let mut resp = vec![self.slave_id, fc, data.len() as u8];
        resp.extend_from_slice(data);
        let (lo, hi) = crc::crc16(&resp);
        resp.push(lo);
        resp.push(hi);
        resp
    }

    fn build_exception(&self, fc: u8, exception_code: u8) -> Vec<u8> {
        let mut resp = vec![self.slave_id, fc | 0x80, exception_code];
        let (lo, hi) = crc::crc16(&resp);
        resp.push(lo);
        resp.push(hi);
        resp
    }
}

/// Run the slave simulator on a serial port. Reads requests, processes them,
/// sends responses. Stops when the channel is dropped.
fn run_slave(
    port_path: &str,
    slave: Arc<Mutex<ModbusSlave>>,
    stop: Arc<std::sync::atomic::AtomicBool>,
) {
    let mut port = serialport::new(port_path, 9600)
        .timeout(Duration::from_millis(10)) // Short timeout for inter-char gap detection
        .open()
        .expect("Slave: failed to open port");

    let mut buf = [0u8; 256];
    while !stop.load(std::sync::atomic::Ordering::Relaxed) {
        // Read first byte (blocks until data arrives or stop flag)
        let mut frame = Vec::new();
        match port.read(&mut buf) {
            Ok(n) if n > 0 => frame.extend_from_slice(&buf[..n]),
            Ok(_) => continue,
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
            Err(_) => continue,
        }

        // Read remaining bytes until inter-character timeout
        loop {
            match port.read(&mut buf) {
                Ok(n) if n > 0 => frame.extend_from_slice(&buf[..n]),
                _ => break, // Timeout = end of frame
            }
        }

        if frame.len() < 4 {
            continue;
        }

        // Process and respond
        let mut slave = slave.lock().unwrap();
        if let Some(response) = slave.process_request(&frame) {
            let _ = port.write_all(&response);
            let _ = port.flush();
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

fn setup_test() -> Option<(Child, Arc<Mutex<ModbusSlave>>, Arc<std::sync::atomic::AtomicBool>, Arc<Mutex<SerialTransport>>, std::thread::JoinHandle<()>)> {
    if !socat_available() {
        eprintln!("Skipping (socat not available)");
        return None;
    }

    let (socat, port_a, port_b) = spawn_virtual_serial();

    // Create slave with test data
    let slave = Arc::new(Mutex::new(ModbusSlave::new(1)));
    {
        let mut s = slave.lock().unwrap();
        s.discrete_inputs[0] = true;
        s.discrete_inputs[1] = false;
        s.discrete_inputs[2] = true;
        s.input_registers[0] = 1234;
        s.input_registers[1] = 5678;
        s.input_registers[2] = 9999;
        s.holding_registers[0] = 100;
        s.holding_registers[1] = 200;
        s.coils[0] = true;
        s.coils[1] = false;
        s.coils[2] = true;
    }

    // Start slave thread
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let slave_clone = Arc::clone(&slave);
    let stop_clone = Arc::clone(&stop);
    let port_b_clone = port_b.clone();
    let slave_thread = std::thread::spawn(move || {
        run_slave(&port_b_clone, slave_clone, stop_clone);
    });

    // Small delay for slave to open port
    std::thread::sleep(Duration::from_millis(100));

    // Open client transport
    let config = SerialConfig {
        port: port_a,
        baud_rate: 9600,
        timeout: Duration::from_millis(500),
        ..Default::default()
    };
    let mut transport = SerialTransport::new(config);
    transport.open().expect("Client: failed to open port");
    let transport = Arc::new(Mutex::new(transport));

    Some((socat, slave, stop, transport, slave_thread))
}

fn teardown(mut socat: Child, stop: Arc<std::sync::atomic::AtomicBool>, thread: std::thread::JoinHandle<()>) {
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(150)); // let slave thread exit
    let _ = thread.join();
    let _ = socat.kill();
}

#[test]
fn read_discrete_inputs() {
    let Some((socat, _slave, stop, transport, thread)) = setup_test() else { return };
    let client = RtuClient::new(transport);

    let inputs = client.read_discrete_inputs(1, 0, 3).expect("FC02 failed");
    assert_eq!(inputs, vec![true, false, true], "Discrete inputs mismatch");

    teardown(socat, stop, thread);
}

#[test]
fn read_input_registers() {
    let Some((socat, _slave, stop, transport, thread)) = setup_test() else { return };
    let client = RtuClient::new(transport);

    let regs = client.read_input_registers(1, 0, 3).expect("FC04 failed");
    assert_eq!(regs, vec![1234, 5678, 9999], "Input registers mismatch");

    teardown(socat, stop, thread);
}

#[test]
fn read_holding_registers() {
    let Some((socat, _slave, stop, transport, thread)) = setup_test() else { return };
    let client = RtuClient::new(transport);

    let regs = client.read_holding_registers(1, 0, 2).expect("FC03 failed");
    assert_eq!(regs, vec![100, 200], "Holding registers mismatch");

    teardown(socat, stop, thread);
}

#[test]
fn read_coils() {
    let Some((socat, _slave, stop, transport, thread)) = setup_test() else { return };
    let client = RtuClient::new(transport);

    let coils = client.read_coils(1, 0, 3).expect("FC01 failed");
    assert_eq!(coils, vec![true, false, true], "Coils mismatch");

    teardown(socat, stop, thread);
}

#[test]
fn write_single_coil_and_read_back() {
    let Some((socat, slave, stop, transport, thread)) = setup_test() else { return };
    let client = RtuClient::new(Arc::clone(&transport));

    // Coil 5 starts as false
    assert!(!slave.lock().unwrap().coils[5]);

    // Write coil 5 = true
    client.write_single_coil(1, 5, true).expect("FC05 write failed");

    // Verify in slave memory
    assert!(slave.lock().unwrap().coils[5], "Coil 5 should be true after write");

    // Read back via Modbus
    let coils = client.read_coils(1, 5, 1).expect("FC01 readback failed");
    assert_eq!(coils, vec![true], "Coil 5 readback mismatch");

    // Write false
    client.write_single_coil(1, 5, false).expect("FC05 write false failed");
    assert!(!slave.lock().unwrap().coils[5], "Coil 5 should be false");

    teardown(socat, stop, thread);
}

#[test]
fn write_single_register_and_read_back() {
    let Some((socat, slave, stop, transport, thread)) = setup_test() else { return };
    let client = RtuClient::new(Arc::clone(&transport));

    // Write register 10 = 42
    client.write_single_register(1, 10, 42).expect("FC06 failed");
    assert_eq!(slave.lock().unwrap().holding_registers[10], 42);

    // Read back
    let regs = client.read_holding_registers(1, 10, 1).expect("FC03 readback failed");
    assert_eq!(regs, vec![42], "Register readback mismatch");

    // Write a different value
    client.write_single_register(1, 10, 9999).expect("FC06 second write failed");
    let regs = client.read_holding_registers(1, 10, 1).expect("FC03 second readback failed");
    assert_eq!(regs, vec![9999]);

    teardown(socat, stop, thread);
}

#[test]
fn write_multiple_registers_and_read_back() {
    let Some((socat, slave, stop, transport, thread)) = setup_test() else { return };
    let client = RtuClient::new(Arc::clone(&transport));

    // Write registers 20-22
    client.write_multiple_registers(1, 20, &[111, 222, 333]).expect("FC10 failed");
    {
        let s = slave.lock().unwrap();
        assert_eq!(s.holding_registers[20], 111);
        assert_eq!(s.holding_registers[21], 222);
        assert_eq!(s.holding_registers[22], 333);
    }

    // Read back
    let regs = client.read_holding_registers(1, 20, 3).expect("FC03 readback failed");
    assert_eq!(regs, vec![111, 222, 333]);

    teardown(socat, stop, thread);
}

#[test]
fn write_multiple_coils_and_read_back() {
    let Some((socat, _slave, stop, transport, thread)) = setup_test() else { return };
    let client = RtuClient::new(Arc::clone(&transport));

    let pattern = vec![true, false, true, true, false];
    client.write_multiple_coils(1, 10, &pattern).expect("FC0F failed");

    let coils = client.read_coils(1, 10, 5).expect("FC01 readback failed");
    assert_eq!(coils, pattern, "Multiple coils readback mismatch");

    teardown(socat, stop, thread);
}

#[test]
fn wrong_slave_id_timeout() {
    let Some((socat, _slave, stop, transport, thread)) = setup_test() else { return };
    let client = RtuClient::new(transport);

    // Slave ID 99 doesn't exist — should timeout
    let result = client.read_holding_registers(99, 0, 1);
    assert!(result.is_err(), "Expected timeout for wrong slave ID");

    teardown(socat, stop, thread);
}
