//! Full-stack integration test: ST program → compile → VM → Modbus RTU → socat → slave.
//!
//! Compiles a real ST program that uses a ModbusRtuDevice native FB,
//! runs it through the VM with a socat virtual serial pair, and verifies
//! that values flow from the Modbus slave through to ST variables.
//!
//! Requires `socat` (via `nix-shell -p socat pkg-config systemdLibs`).

use std::io::{Read, Write};
use std::process::{Child, Command};
use std::sync::Arc;
use std::time::Duration;

use st_comm_api::*;
use st_comm_modbus::crc;
use st_comm_modbus::device_fb::ModbusRtuDeviceNativeFb;
use st_ir::Value;

fn socat_available() -> bool {
    Command::new("socat").arg("-V").output().map(|o| o.status.success()).unwrap_or(false)
}

fn spawn_virtual_serial() -> (Child, String, String) {
    let port_a = format!("/tmp/st-fullstack-a-{}", std::process::id());
    let port_b = format!("/tmp/st-fullstack-b-{}", std::process::id());
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
    assert!(std::path::Path::new(&port_a).exists());
    (child, port_a, port_b)
}

/// Minimal Modbus slave (same as rtu_integration_test but trimmed).
fn run_slave(port_path: &str, stop: Arc<std::sync::atomic::AtomicBool>) {
    let mut port = serialport::new(port_path, 9600)
        .timeout(Duration::from_millis(10))
        .open()
        .expect("Slave: failed to open");

    // Slave state: 8 discrete inputs, 8 coils, 8 input regs, 8 holding regs
    let mut discrete_inputs = vec![false; 8];
    let mut coils = vec![false; 8];
    let mut input_registers = vec![0u16; 8];
    let mut holding_registers = vec![0u16; 8];

    // Set some test values
    discrete_inputs[0] = true;  // DI_0 = TRUE
    discrete_inputs[1] = false; // DI_1 = FALSE
    input_registers[0] = 4200;  // AI_0 = 4200

    let mut buf = [0u8; 256];
    while !stop.load(std::sync::atomic::Ordering::Relaxed) {
        let mut frame = Vec::new();
        match port.read(&mut buf) {
            Ok(n) if n > 0 => frame.extend_from_slice(&buf[..n]),
            Ok(_) => continue,
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => continue,
            Err(_) => continue,
        }
        // Read remaining
        loop {
            match port.read(&mut buf) {
                Ok(n) if n > 0 => frame.extend_from_slice(&buf[..n]),
                _ => break,
            }
        }
        if frame.len() < 4 || !crc::verify_crc(&frame) || frame[0] != 1 {
            continue;
        }

        let fc = frame[1];
        let start = ((frame[2] as u16) << 8) | frame[3] as u16;
        let count = if frame.len() >= 6 { ((frame[4] as u16) << 8) | frame[5] as u16 } else { 1 };

        let response = match fc {
            0x02 => { // Read Discrete Inputs
                let byte_count = count.div_ceil(8) as u8;
                let mut data = vec![0u8; byte_count as usize];
                for i in 0..count {
                    let idx = (start + i) as usize;
                    if idx < discrete_inputs.len() && discrete_inputs[idx] {
                        data[(i / 8) as usize] |= 1 << (i % 8);
                    }
                }
                let mut r = vec![1, 0x02, byte_count];
                r.extend_from_slice(&data);
                let (lo, hi) = crc::crc16(&r);
                r.push(lo); r.push(hi);
                r
            }
            0x04 => { // Read Input Registers
                let mut data = Vec::new();
                for i in 0..count {
                    let idx = (start + i) as usize;
                    let v = if idx < input_registers.len() { input_registers[idx] } else { 0 };
                    data.push((v >> 8) as u8);
                    data.push((v & 0xFF) as u8);
                }
                let mut r = vec![1, 0x04, (count * 2) as u8];
                r.extend_from_slice(&data);
                let (lo, hi) = crc::crc16(&r);
                r.push(lo); r.push(hi);
                r
            }
            0x01 => { // Read Coils
                let byte_count = count.div_ceil(8) as u8;
                let mut data = vec![0u8; byte_count as usize];
                for i in 0..count {
                    let idx = (start + i) as usize;
                    if idx < coils.len() && coils[idx] {
                        data[(i / 8) as usize] |= 1 << (i % 8);
                    }
                }
                let mut r = vec![1, 0x01, byte_count];
                r.extend_from_slice(&data);
                let (lo, hi) = crc::crc16(&r);
                r.push(lo); r.push(hi);
                r
            }
            0x05 => { // Write Single Coil
                let val = frame[4] == 0xFF;
                if (start as usize) < coils.len() { coils[start as usize] = val; }
                let mut r = frame[..6].to_vec();
                let (lo, hi) = crc::crc16(&r);
                r.push(lo); r.push(hi);
                r
            }
            0x06 => { // Write Single Register
                let val = ((frame[4] as u16) << 8) | frame[5] as u16;
                if (start as usize) < holding_registers.len() { holding_registers[start as usize] = val; }
                let mut r = frame[..6].to_vec();
                let (lo, hi) = crc::crc16(&r);
                r.push(lo); r.push(hi);
                r
            }
            0x03 => { // Read Holding Registers
                let mut data = Vec::new();
                for i in 0..count {
                    let idx = (start + i) as usize;
                    let v = if idx < holding_registers.len() { holding_registers[idx] } else { 0 };
                    data.push((v >> 8) as u8);
                    data.push((v & 0xFF) as u8);
                }
                let mut r = vec![1, 0x03, (count * 2) as u8];
                r.extend_from_slice(&data);
                let (lo, hi) = crc::crc16(&r);
                r.push(lo); r.push(hi);
                r
            }
            _ => continue,
        };

        let _ = port.write_all(&response);
        let _ = port.flush();
    }
}

#[test]
fn full_stack_modbus_rtu_through_vm() {
    if !socat_available() {
        eprintln!("Skipping (socat not available)");
        return;
    }

    let (mut socat, port_a, port_b) = spawn_virtual_serial();

    // Start slave
    let stop = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let stop_clone = Arc::clone(&stop);
    let slave_thread = std::thread::spawn(move || run_slave(&port_b, stop_clone));
    std::thread::sleep(Duration::from_millis(100));

    // Build profile
    let profile_yaml = r#"
name: TestIO
protocol: modbus-rtu
fields:
  - { name: DI_0, type: BOOL, direction: input, register: { address: 0, kind: discrete_input } }
  - { name: DI_1, type: BOOL, direction: input, register: { address: 1, kind: discrete_input } }
  - { name: AI_0, type: INT, direction: input, register: { address: 0, kind: input_register } }
  - { name: DO_0, type: BOOL, direction: output, register: { address: 0, kind: coil } }
  - { name: AO_0, type: INT, direction: output, register: { address: 0, kind: holding_register } }
"#;
    let profile = DeviceProfile::from_yaml(profile_yaml).unwrap();

    // Build registry with ModbusRtuDevice + compile + run through VM
    let transport_map = st_comm_serial::new_transport_map();
    let mut registry = NativeFbRegistry::new();
    registry.register(Box::new(ModbusRtuDeviceNativeFb::new(
        profile, Arc::clone(&transport_map),
    )));

    // ST source: use the Modbus device, copy DI_0 → DO_0, AI_0 → AO_0
    let source = format!(r#"
VAR_GLOBAL
    g_connected : BOOL := FALSE;
    g_di0       : BOOL := FALSE;
    g_ai0       : INT := 0;
    g_do0_set   : BOOL := FALSE;
END_VAR

PROGRAM Main
VAR
    io : TestIO;
END_VAR
    io(
        port := '{port_a}',
        baud := 9600,
        parity := 'N',
        data_bits := 8,
        stop_bits := 1,
        slave_id := 1,
        refresh_rate := T#0ms
    );

    g_connected := io.connected;
    g_di0 := io.DI_0;
    g_ai0 := io.AI_0;

    io.DO_0 := io.DI_0;
    io.AO_0 := io.AI_0;
    g_do0_set := io.DO_0;
END_PROGRAM
"#);

    let stdlib = st_syntax::multi_file::builtin_stdlib();
    let mut all: Vec<&str> = stdlib;
    all.push(&source);
    let parse_result = st_syntax::multi_file::parse_multi(&all);
    assert!(parse_result.errors.is_empty(), "Parse errors: {:?}", parse_result.errors);

    let module = st_compiler::compile_with_native_fbs(&parse_result.source_file, Some(&registry))
        .expect("Compilation failed");

    let arc_reg = Arc::new(registry);
    let mut vm = st_engine::vm::Vm::new_with_native_fbs(
        module,
        st_engine::vm::VmConfig::default(),
        Some(arc_reg),
    );
    let _ = vm.run_global_init();

    // Run a few cycles — the Modbus device should communicate with the slave
    for _ in 0..3 {
        vm.scan_cycle("Main").expect("Scan cycle failed");
    }

    // Verify: slave had DI_0=true, AI_0=4200
    let connected = vm.get_global("g_connected");
    assert_eq!(connected, Some(&Value::Bool(true)), "Should be connected to Modbus slave");

    let di0 = vm.get_global("g_di0");
    assert_eq!(di0, Some(&Value::Bool(true)), "DI_0 should be TRUE (from slave)");

    let ai0 = vm.get_global("g_ai0");
    assert_eq!(ai0, Some(&Value::Int(4200)), "AI_0 should be 4200 (from slave)");

    // DO_0 should have been written (mirrors DI_0=true)
    let do0 = vm.get_global("g_do0_set");
    assert_eq!(do0, Some(&Value::Bool(true)), "DO_0 should be TRUE (written by program)");

    // Cleanup
    stop.store(true, std::sync::atomic::Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(50));
    let _ = slave_thread.join();
    let _ = socat.kill();
}
