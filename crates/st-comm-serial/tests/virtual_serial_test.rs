//! Integration tests using a virtual serial port pair via socat.
//!
//! Requires `socat` (available via `nix-shell -p socat`).
//! Skipped automatically if socat is not in PATH.

use st_comm_api::NativeFb; // for execute()
use st_comm_serial::framing::{FrameParser, FrameStatus};
use std::io::{Read, Write};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

fn socat_available() -> bool {
    Command::new("socat")
        .arg("-V")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Spawns socat to create a virtual serial port pair.
/// Returns (socat_child, port_a_path, port_b_path).
fn spawn_virtual_serial() -> (Child, String, String) {
    let port_a = format!("/tmp/st-vpty-a-{}", std::process::id());
    let port_b = format!("/tmp/st-vpty-b-{}", std::process::id());

    // Clean up stale symlinks
    let _ = std::fs::remove_file(&port_a);
    let _ = std::fs::remove_file(&port_b);

    let child = Command::new("socat")
        .args([
            &format!("pty,raw,echo=0,link={port_a}"),
            &format!("pty,raw,echo=0,link={port_b}"),
        ])
        .spawn()
        .expect("Failed to spawn socat — is it installed? (nix-shell -p socat)");

    // Wait for the pty symlinks to appear
    for _ in 0..50 {
        if std::path::Path::new(&port_a).exists() && std::path::Path::new(&port_b).exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        std::path::Path::new(&port_a).exists(),
        "socat did not create {port_a}"
    );

    (child, port_a, port_b)
}

// ── SerialTransport tests ──────────────────────────────────────────────

#[test]
fn transport_open_virtual_port() {
    if !socat_available() {
        eprintln!("Skipping (socat not available)");
        return;
    }

    let (mut socat, port_a, _port_b) = spawn_virtual_serial();

    let config = st_comm_serial::transport::SerialConfig {
        port: port_a,
        baud_rate: 9600,
        timeout: Duration::from_millis(100),
        ..Default::default()
    };
    let mut transport = st_comm_serial::transport::SerialTransport::new(config);
    assert!(!transport.is_open());

    transport.open().expect("Failed to open virtual port");
    assert!(transport.is_open());

    transport.close();
    assert!(!transport.is_open());

    socat.kill().unwrap();
}

#[test]
fn transport_send_receive_loopback() {
    if !socat_available() {
        eprintln!("Skipping (socat not available)");
        return;
    }

    let (mut socat, port_a, port_b) = spawn_virtual_serial();

    // Open transport on port A
    let config = st_comm_serial::transport::SerialConfig {
        port: port_a,
        baud_rate: 9600,
        timeout: Duration::from_millis(500),
        ..Default::default()
    };
    let mut transport = st_comm_serial::transport::SerialTransport::new(config);
    transport.open().expect("Failed to open port A");

    // Open port B as a raw serial port (simulated slave)
    let slave = serialport::new(&port_b, 9600)
        .timeout(Duration::from_millis(500))
        .open()
        .expect("Failed to open port B");

    // Spawn echo thread on the slave side
    let slave_clone = slave.try_clone().unwrap();
    let echo_thread = std::thread::spawn(move || {
        let mut port = slave_clone;
        let mut buf = [0u8; 256];
        match port.read(&mut buf) {
            Ok(n) if n > 0 => {
                // Echo back with a marker byte prepended
                let mut response = vec![0xAA]; // marker
                response.extend_from_slice(&buf[..n]);
                let _ = port.write_all(&response);
                let _ = port.flush();
            }
            _ => {}
        }
    });

    // Send a test frame from the transport
    let request = [0x01, 0x03, 0x00, 0x00, 0x00, 0x0A]; // Modbus-like read request
    transport.send(&request).expect("Send failed");

    // Read the echoed response with a known length (1 marker + request).
    let expected_len = 1 + request.len();
    let mut response = [0u8; 64];
    transport
        .receive_exact(&mut response, expected_len)
        .expect("receive_exact failed");

    assert_eq!(response[0], 0xAA, "Expected echo marker byte");
    assert_eq!(&response[1..1 + request.len()], &request, "Echoed data mismatch");

    echo_thread.join().unwrap();
    drop(slave);
    socat.kill().unwrap();
}

/// A minimal fixed-length frame parser used to drive `transaction_framed`
/// without depending on any specific protocol — exercises the generic
/// transport API end-to-end.
struct FixedLenParser(usize);
impl FrameParser for FixedLenParser {
    fn parse(&mut self, buf: &[u8]) -> FrameStatus {
        if buf.len() < self.0 {
            FrameStatus::Need(self.0)
        } else {
            FrameStatus::Complete(self.0)
        }
    }
}

#[test]
fn transport_transaction_framed_roundtrip() {
    if !socat_available() {
        eprintln!("Skipping (socat not available)");
        return;
    }

    let (mut socat, port_a, port_b) = spawn_virtual_serial();

    let config = st_comm_serial::transport::SerialConfig {
        port: port_a,
        baud_rate: 19200,
        timeout: Duration::from_millis(500),
        ..Default::default()
    };
    let mut transport = st_comm_serial::transport::SerialTransport::new(config);
    transport.open().unwrap();

    // Slave: read request, respond with fixed-length data.
    let slave = serialport::new(&port_b, 19200)
        .timeout(Duration::from_millis(500))
        .open()
        .unwrap();

    let slave_clone = slave.try_clone().unwrap();
    let responder = std::thread::spawn(move || {
        let mut port = slave_clone;
        let mut buf = [0u8; 256];
        if let Ok(n) = port.read(&mut buf) {
            if n > 0 {
                // Modbus-shaped response: slave, fc, byte_count, 4 data bytes
                let response = [0x01, 0x03, 0x04, 0x00, 0x01, 0x00, 0x02];
                let _ = port.write_all(&response);
                let _ = port.flush();
            }
        }
    });

    let request = [0x01, 0x03, 0x00, 0x00, 0x00, 0x02];
    let mut response = [0u8; 64];
    let mut parser = FixedLenParser(7);
    let n = transport
        .transaction_framed(&request, &mut response, &mut parser, Duration::from_millis(500), Duration::ZERO)
        .unwrap();

    assert_eq!(n, 7, "Expected exactly 7 response bytes, got {n}");
    assert_eq!(response[0], 0x01, "Slave ID mismatch");
    assert_eq!(response[1], 0x03, "Function code mismatch");
    assert_eq!(response[2], 0x04, "Byte count mismatch");

    responder.join().unwrap();
    drop(slave);
    socat.kill().unwrap();
}

/// Regression test for the original performance bug: a healthy transaction
/// must NOT block until the configured timeout once the response has been
/// fully received. This is the whole point of the [`FrameParser`] abstraction.
///
/// We set a very generous timeout (1 second) and assert the transaction
/// completes in under ~50 ms. With the old "read until buffer full or
/// timeout fires" code, this test would take >= 1 second.
#[test]
fn transaction_framed_returns_before_timeout_when_frame_complete() {
    if !socat_available() {
        eprintln!("Skipping (socat not available)");
        return;
    }

    let (mut socat, port_a, port_b) = spawn_virtual_serial();

    let timeout = Duration::from_millis(1000);
    let config = st_comm_serial::transport::SerialConfig {
        port: port_a,
        baud_rate: 115200,
        timeout,
        ..Default::default()
    };
    let mut transport = st_comm_serial::transport::SerialTransport::new(config);
    transport.open().unwrap();

    let slave = serialport::new(&port_b, 115200)
        .timeout(Duration::from_millis(500))
        .open()
        .unwrap();

    // Slave responds promptly with a 7-byte FC03 frame, then stays silent.
    // The transport must NOT wait for further bytes once the frame is complete.
    let slave_clone = slave.try_clone().unwrap();
    let responder = std::thread::spawn(move || {
        let mut port = slave_clone;
        let mut buf = [0u8; 256];
        if let Ok(n) = port.read(&mut buf) {
            if n > 0 {
                let response = [0x01, 0x03, 0x04, 0x00, 0x2A, 0x00, 0x2B];
                let _ = port.write_all(&response);
                let _ = port.flush();
            }
        }
        // Deliberately stays idle — DOES NOT close the port. This is the
        // condition that used to cause the receive() loop to drain until
        // the inactivity timeout fired.
        std::thread::sleep(Duration::from_secs(2));
    });

    let request = [0x01, 0x03, 0x00, 0x00, 0x00, 0x02];
    let mut response = [0u8; 64];
    let mut parser = FixedLenParser(7);

    let started = Instant::now();
    let n = transport
        .transaction_framed(&request, &mut response, &mut parser, timeout, Duration::ZERO)
        .expect("framed transaction failed");
    let elapsed = started.elapsed();

    assert_eq!(n, 7, "Expected exactly 7 bytes, got {n}");
    // The bug used to add a full timeout (1s here) of dead time. Allow a
    // generous CI margin (well under the timeout) to keep the test stable.
    assert!(
        elapsed < Duration::from_millis(150),
        "Transaction took {elapsed:?} — should return immediately after the \
         frame is complete, not wait for the {timeout:?} inactivity timeout"
    );

    drop(slave);
    socat.kill().unwrap();
    // Don't join responder — it is sleeping so the port stays open.
    drop(responder);
}

/// Confirms that the inactivity timeout still fires correctly when a frame
/// never finishes arriving — i.e. we removed the buggy drain without
/// removing the legitimate timeout safety net.
#[test]
fn transaction_framed_times_out_on_truncated_response() {
    if !socat_available() {
        eprintln!("Skipping (socat not available)");
        return;
    }

    let (mut socat, port_a, port_b) = spawn_virtual_serial();

    let timeout = Duration::from_millis(150);
    let config = st_comm_serial::transport::SerialConfig {
        port: port_a,
        baud_rate: 115200,
        timeout,
        ..Default::default()
    };
    let mut transport = st_comm_serial::transport::SerialTransport::new(config);
    transport.open().unwrap();

    let slave = serialport::new(&port_b, 115200)
        .timeout(Duration::from_millis(500))
        .open()
        .unwrap();

    // Slave sends only 3 bytes, then stays silent — frame is incomplete.
    let slave_clone = slave.try_clone().unwrap();
    let responder = std::thread::spawn(move || {
        let mut port = slave_clone;
        let mut buf = [0u8; 256];
        if let Ok(n) = port.read(&mut buf) {
            if n > 0 {
                let _ = port.write_all(&[0x01, 0x03, 0x04]); // header only
                let _ = port.flush();
            }
        }
        std::thread::sleep(Duration::from_secs(2));
    });

    let request = [0x01, 0x03, 0x00, 0x00, 0x00, 0x02];
    let mut response = [0u8; 64];
    let mut parser = FixedLenParser(7);

    let started = Instant::now();
    let result = transport.transaction_framed(&request, &mut response, &mut parser, timeout, Duration::ZERO);
    let elapsed = started.elapsed();

    assert!(result.is_err(), "Expected timeout error, got {result:?}");
    let err = result.unwrap_err();
    assert!(err.contains("timeout"), "Error should mention timeout: {err}");
    // The error should fire close to the configured timeout — not 10x it.
    assert!(
        elapsed < timeout * 3,
        "Transaction took {elapsed:?}, expected close to {timeout:?}"
    );

    drop(slave);
    socat.kill().unwrap();
    drop(responder);
}

/// Confirms `receive_exact` does the right thing for fixed-length protocols
/// and doesn't wait past the requested byte count.
#[test]
fn receive_exact_returns_immediately_when_bytes_available() {
    if !socat_available() {
        eprintln!("Skipping (socat not available)");
        return;
    }

    let (mut socat, port_a, port_b) = spawn_virtual_serial();

    let timeout = Duration::from_millis(1000);
    let config = st_comm_serial::transport::SerialConfig {
        port: port_a,
        baud_rate: 115200,
        timeout,
        ..Default::default()
    };
    let mut transport = st_comm_serial::transport::SerialTransport::new(config);
    transport.open().unwrap();

    let mut slave = serialport::new(&port_b, 115200)
        .timeout(Duration::from_millis(500))
        .open()
        .unwrap();

    // Slave dumps 5 bytes, then sits idle.
    let payload = [0xDE, 0xAD, 0xBE, 0xEF, 0x42];
    slave.write_all(&payload).unwrap();
    slave.flush().unwrap();

    let mut buf = [0u8; 16];
    let started = Instant::now();
    transport.receive_exact(&mut buf, payload.len()).unwrap();
    let elapsed = started.elapsed();

    assert_eq!(&buf[..payload.len()], &payload);
    assert!(
        elapsed < Duration::from_millis(150),
        "receive_exact took {elapsed:?} for {} bytes — should be near-instant",
        payload.len()
    );

    drop(slave);
    socat.kill().unwrap();
}

// ── SerialLinkNativeFb tests ───────────────────────────────────────────

#[test]
fn serial_link_fb_connects_to_virtual_port() {
    if !socat_available() {
        eprintln!("Skipping (socat not available)");
        return;
    }

    let (mut socat, port_a, _port_b) = spawn_virtual_serial();

    let fb = st_comm_serial::SerialLinkNativeFb::new();
    let mut fields = vec![
        st_ir::Value::String(port_a),         // port
        st_ir::Value::Int(9600),              // baud
        st_ir::Value::String("N".into()),     // parity
        st_ir::Value::Int(8),                 // data_bits
        st_ir::Value::Int(1),                 // stop_bits
        st_ir::Value::Bool(false),            // connected
        st_ir::Value::Int(0),                 // error_code
    ];

    // First call: should open the port
    fb.execute(&mut fields);
    assert_eq!(fields[5], st_ir::Value::Bool(true), "Should be connected");
    assert_eq!(fields[6], st_ir::Value::Int(0), "Error code should be 0");

    // Second call: should maintain connection
    fb.execute(&mut fields);
    assert_eq!(fields[5], st_ir::Value::Bool(true), "Still connected");

    // Verify the transport handle is usable
    let handle = fb.transport_handle();
    let transport = handle.lock().unwrap();
    assert!(transport.is_open(), "Transport should be open via handle");

    drop(transport);
    socat.kill().unwrap();
}

#[test]
fn serial_link_fb_shared_transport_usable() {
    if !socat_available() {
        eprintln!("Skipping (socat not available)");
        return;
    }

    let (mut socat, port_a, port_b) = spawn_virtual_serial();

    // Open the link FB
    let fb = st_comm_serial::SerialLinkNativeFb::new();
    let mut fields = vec![
        st_ir::Value::String(port_a),
        st_ir::Value::Int(9600),
        st_ir::Value::String("N".into()),
        st_ir::Value::Int(8),
        st_ir::Value::Int(1),
        st_ir::Value::Bool(false),
        st_ir::Value::Int(0),
    ];
    fb.execute(&mut fields);
    assert_eq!(fields[5], st_ir::Value::Bool(true));

    // Open slave side
    let slave = serialport::new(&port_b, 9600)
        .timeout(Duration::from_millis(500))
        .open()
        .unwrap();

    // Echo responder
    let slave_clone = slave.try_clone().unwrap();
    let echo = std::thread::spawn(move || {
        let mut port = slave_clone;
        let mut buf = [0u8; 256];
        if let Ok(n) = port.read(&mut buf) {
            if n > 0 {
                let _ = port.write_all(&buf[..n]); // echo back
                let _ = port.flush();
            }
        }
    });

    // Use the transport handle directly (as a device FB would)
    let handle = fb.transport_handle();
    let mut transport = handle.lock().unwrap();
    let test_data = [0x42, 0x43, 0x44];
    let mut response = [0u8; 16];
    let mut parser = FixedLenParser(test_data.len());
    let n = transport
        .transaction_framed(&test_data, &mut response, &mut parser, Duration::from_millis(500), Duration::ZERO)
        .unwrap();
    assert_eq!(n, 3, "Expected 3 echoed bytes");
    assert_eq!(&response[..3], &test_data, "Echoed data should match");

    drop(transport);
    echo.join().unwrap();
    drop(slave);
    socat.kill().unwrap();
}
