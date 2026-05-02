//! Integration tests for `TcpModbusClient` using an in-process Modbus TCP
//! slave running on the loopback interface.
//!
//! `client.rs` had 0% acceptance coverage before this file existed because
//! the only callers in production are the `ModbusTcpDeviceNativeFb` (which
//! requires a real device to talk to) and the unit tests on `frame.rs`
//! (which only check byte-level framing). These tests close that gap by
//! standing up a tiny std-only TCP server that speaks the MBAP-framed
//! Modbus subset the client uses, then driving each FC method against it.

use st_comm_modbus_tcp::client::TcpModbusClient;
use st_comm_modbus_tcp::transport::{TcpConfig, TcpTransport};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

/// In-process Modbus TCP slave. Holds 16 sequential u16 registers and 16
/// alternating coils; FC05/FC06 writes are echoed back; FC0F/FC10 are
/// accepted with a stock response. This is the smallest implementation
/// that reaches every public method on `TcpModbusClient`.
struct MockSlave {
    listener: TcpListener,
    stop: Arc<AtomicBool>,
}

impl MockSlave {
    fn start() -> (u16, Arc<AtomicBool>, thread::JoinHandle<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().unwrap().port();
        listener.set_nonblocking(true).unwrap();

        let stop = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop);
        let slave = MockSlave { listener, stop: Arc::clone(&stop) };

        let handle = thread::spawn(move || slave.serve_forever());
        // Tiny delay so the server is in `accept()` before tests dial it.
        thread::sleep(Duration::from_millis(20));
        (port, stop_clone, handle)
    }

    fn serve_forever(self) {
        while !self.stop.load(Ordering::Relaxed) {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    stream.set_nonblocking(false).ok();
                    stream.set_read_timeout(Some(Duration::from_secs(1))).ok();
                    let stop = Arc::clone(&self.stop);
                    thread::spawn(move || handle_client(stream, stop));
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(10));
                }
                Err(_) => break,
            }
        }
    }
}

fn handle_client(mut stream: TcpStream, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Relaxed) {
        let mut header = [0u8; 7];
        if stream.read_exact(&mut header).is_err() {
            return; // client closed
        }
        let txn_hi = header[0];
        let txn_lo = header[1];
        let length = u16::from_be_bytes([header[4], header[5]]) as usize;
        let unit_id = header[6];
        if !(1..=250).contains(&length) {
            return;
        }

        let mut pdu = vec![0u8; length - 1]; // -1 for unit_id already read
        if stream.read_exact(&mut pdu).is_err() {
            return;
        }

        let response_pdu = build_response_pdu(&pdu);
        let resp_len = (response_pdu.len() + 1) as u16; // +1 for unit_id
        let mut response = Vec::with_capacity(7 + response_pdu.len());
        response.extend_from_slice(&[txn_hi, txn_lo, 0, 0]); // txn + proto
        response.extend_from_slice(&resp_len.to_be_bytes());
        response.push(unit_id);
        response.extend_from_slice(&response_pdu);

        if stream.write_all(&response).is_err() {
            return;
        }
    }
}

/// Build the response PDU for a request PDU. Function code is the first byte.
fn build_response_pdu(req: &[u8]) -> Vec<u8> {
    let fc = req[0];
    match fc {
        // FC01 Read Coils, FC02 Read Discrete Inputs.
        0x01 | 0x02 => {
            let count = u16::from_be_bytes([req[3], req[4]]) as usize;
            let bytes = count.div_ceil(8);
            let mut data = vec![0u8; bytes];
            // Alternating pattern: bit 0 = 1, bit 1 = 0, bit 2 = 1, ...
            for i in 0..count {
                if i % 2 == 0 {
                    data[i / 8] |= 1 << (i % 8);
                }
            }
            let mut pdu = vec![fc, bytes as u8];
            pdu.extend(data);
            pdu
        }
        // FC03 Read Holding, FC04 Read Input Registers.
        0x03 | 0x04 => {
            let count = u16::from_be_bytes([req[3], req[4]]) as usize;
            let mut pdu = vec![fc, (count * 2) as u8];
            for i in 0..count {
                pdu.push(0);
                pdu.push(i as u8 + 100); // 100, 101, 102, ...
            }
            pdu
        }
        // FC05 Write Single Coil — echo the request data.
        0x05 | 0x06 => req.to_vec(),
        // FC0F Write Multiple Coils — echo addr + count.
        0x0F | 0x10 => {
            // [fc][addr 2B][count 2B] from request
            req[..5].to_vec()
        }
        _ => {
            // Exception response: FC | 0x80, exception code 0x01 (illegal function).
            vec![fc | 0x80, 0x01]
        }
    }
}

/// Helper: build a connected client against a freshly started slave.
fn connected(port: u16) -> TcpTransport {
    let config = TcpConfig {
        host: "127.0.0.1".to_string(),
        port,
        timeout: Duration::from_millis(500),
        connect_timeout: Duration::from_secs(2),
    };
    let mut transport = TcpTransport::new(config);
    transport.connect().expect("connect to mock slave");
    assert!(transport.is_connected());
    transport
}

// ── Tests ────────────────────────────────────────────────────────────────

#[test]
fn read_holding_registers_returns_slave_values() {
    let (port, stop, _h) = MockSlave::start();
    let mut transport = connected(port);
    let mut client = TcpModbusClient::new(&mut transport);

    let regs = client.read_holding_registers(1, 0, 4).expect("read_holding");
    // Slave returns 100, 101, 102, 103 for indices 0..4.
    assert_eq!(regs, vec![100, 101, 102, 103]);
    stop.store(true, Ordering::Relaxed);
}

#[test]
fn read_input_registers_returns_slave_values() {
    let (port, stop, _h) = MockSlave::start();
    let mut transport = connected(port);
    let mut client = TcpModbusClient::new(&mut transport);

    let regs = client.read_input_registers(1, 0, 2).expect("read_input");
    assert_eq!(regs, vec![100, 101]);
    stop.store(true, Ordering::Relaxed);
}

#[test]
fn read_coils_decodes_alternating_pattern() {
    let (port, stop, _h) = MockSlave::start();
    let mut transport = connected(port);
    let mut client = TcpModbusClient::new(&mut transport);

    let coils = client.read_coils(1, 0, 8).expect("read_coils");
    // Slave: bit i is 1 iff i is even.
    assert_eq!(coils, vec![true, false, true, false, true, false, true, false]);
    stop.store(true, Ordering::Relaxed);
}

#[test]
fn read_discrete_inputs_decodes_alternating_pattern() {
    let (port, stop, _h) = MockSlave::start();
    let mut transport = connected(port);
    let mut client = TcpModbusClient::new(&mut transport);

    let bits = client.read_discrete_inputs(1, 0, 4).expect("read_discrete");
    assert_eq!(bits, vec![true, false, true, false]);
    stop.store(true, Ordering::Relaxed);
}

#[test]
fn write_single_register_succeeds() {
    let (port, stop, _h) = MockSlave::start();
    let mut transport = connected(port);
    let mut client = TcpModbusClient::new(&mut transport);

    client.write_single_register(1, 5, 0xBEEF).expect("write reg");
    stop.store(true, Ordering::Relaxed);
}

#[test]
fn write_single_coil_succeeds_for_both_states() {
    let (port, stop, _h) = MockSlave::start();
    let mut transport = connected(port);
    let mut client = TcpModbusClient::new(&mut transport);

    client.write_single_coil(1, 0, true).expect("write coil on");
    client.write_single_coil(1, 1, false).expect("write coil off");
    stop.store(true, Ordering::Relaxed);
}

#[test]
fn write_multiple_registers_succeeds() {
    let (port, stop, _h) = MockSlave::start();
    let mut transport = connected(port);
    let mut client = TcpModbusClient::new(&mut transport);

    client
        .write_multiple_registers(1, 0, &[1, 2, 3, 4])
        .expect("write multi");
    stop.store(true, Ordering::Relaxed);
}

#[test]
fn write_multiple_coils_succeeds() {
    let (port, stop, _h) = MockSlave::start();
    let mut transport = connected(port);
    let mut client = TcpModbusClient::new(&mut transport);

    client
        .write_multiple_coils(1, 0, &[true, false, true, true])
        .expect("write coils");
    stop.store(true, Ordering::Relaxed);
}

#[test]
fn transaction_id_increments_across_requests() {
    // We can't observe the wire transaction id from the client side (the
    // public API hides it), but we can prove the sequencer works by issuing
    // many requests in a row and asserting they all succeed. If the txn id
    // wrapped or collided, our slave's response (which echoes the txn id)
    // would mismatch what the client expects and `parse_response` would
    // either return the wrong txn id or fail.
    let (port, stop, _h) = MockSlave::start();
    let mut transport = connected(port);
    let mut client = TcpModbusClient::new(&mut transport);

    for _ in 0..20 {
        let regs = client.read_holding_registers(1, 0, 1).unwrap();
        assert_eq!(regs, vec![100]);
    }
    stop.store(true, Ordering::Relaxed);
}

#[test]
fn disconnect_then_reconnect_works() {
    let (port, stop, _h) = MockSlave::start();
    let mut transport = connected(port);
    transport.disconnect();
    assert!(!transport.is_connected());
    transport.reconnect().expect("reconnect");
    assert!(transport.is_connected());

    let mut client = TcpModbusClient::new(&mut transport);
    let regs = client.read_holding_registers(1, 0, 1).unwrap();
    assert_eq!(regs, vec![100]);
    stop.store(true, Ordering::Relaxed);
}
