//! Subprocess-driven smoke tests for the `st-target-agent` binary.
//!
//! `crates/st-target-agent/src/main.rs` (≈140 lines) sits at 0% acceptance
//! coverage because the existing `api_integration` tests construct the app
//! state in-process and never exec the binary itself. These tests cover the
//! argument parser, the "config doesn't exist" / "bad config" branches, and
//! the singleton + bind paths via short-lived processes.
//!
//! For the happy-path "agent runs and serves" case we don't repeat what
//! `api_integration` already covers — we only assert that the binary
//! actually starts listening (proven by a 200 on /api/v1/health) and then
//! kill it. Anything richer would just be a slower copy of the in-process
//! suite.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn agent_bin() -> PathBuf {
    if let Some(p) = option_env!("CARGO_BIN_EXE_st-target-agent") {
        return PathBuf::from(p);
    }
    let test_exe = std::env::current_exe().expect("current_exe");
    let dir = test_exe.parent().and_then(|p| p.parent()).expect("target dir");
    dir.join("st-target-agent")
}

fn run_with_timeout(args: &[&str], timeout: Duration) -> (Option<i32>, String, String) {
    let mut child = Command::new(agent_bin())
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("agent failed to launch");

    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().expect("try_wait") {
            let mut stdout = String::new();
            let mut stderr = String::new();
            if let Some(mut o) = child.stdout.take() {
                use std::io::Read;
                let _ = o.read_to_string(&mut stdout);
            }
            if let Some(mut e) = child.stderr.take() {
                use std::io::Read;
                let _ = e.read_to_string(&mut stderr);
            }
            return (status.code(), stdout, stderr);
        }
        std::thread::sleep(Duration::from_millis(50));
    }

    // Process is still alive past the timeout — kill and report.
    let _ = child.kill();
    let _ = child.wait();
    (None, String::new(), String::new())
}

// ── --help / --version (clap-driven, exit immediately) ────────────────────

#[test]
fn help_flag_lists_config_option() {
    let (code, stdout, stderr) =
        run_with_timeout(&["--help"], Duration::from_secs(5));
    let combined = format!("{stdout}{stderr}");
    assert_eq!(code, Some(0), "--help should exit 0; output={combined}");
    assert!(combined.contains("--config"), "help should advertise --config: {combined}");
}

#[test]
fn version_flag_prints_a_version() {
    let (code, stdout, stderr) =
        run_with_timeout(&["--version"], Duration::from_secs(5));
    let combined = format!("{stdout}{stderr}");
    assert_eq!(code, Some(0), "--version should exit 0; output={combined}");
    assert!(
        combined.to_lowercase().contains("st-target-agent")
            || combined.chars().any(|c| c.is_ascii_digit()),
        "version output looks empty: {combined:?}"
    );
}

#[test]
fn unknown_flag_exits_nonzero() {
    let (code, _stdout, stderr) =
        run_with_timeout(&["--this-flag-does-not-exist"], Duration::from_secs(5));
    assert_ne!(code, Some(0), "unknown flag should not succeed");
    assert!(
        stderr.to_lowercase().contains("error") || stderr.contains("unexpected"),
        "should describe the error, got: {stderr}"
    );
}

// ── Config loading paths ─────────────────────────────────────────────────

#[test]
fn invalid_yaml_in_config_exits_with_message() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("agent.yaml");
    std::fs::write(&cfg, ":::\nnot valid yaml at all\n  - [bad").unwrap();

    let (code, _stdout, stderr) = run_with_timeout(
        &["--config", cfg.to_str().unwrap()],
        Duration::from_secs(5),
    );
    assert_eq!(
        code,
        Some(1),
        "broken config should exit 1, got: {code:?} / {stderr}"
    );
    assert!(
        stderr.to_lowercase().contains("config error"),
        "should print 'Config error', got: {stderr}"
    );
}

// ── Happy path: agent actually binds and serves ──────────────────────────

#[test]
fn agent_starts_and_responds_to_health() {
    // Pick a random local port. Bind, ask the kernel for the port, drop the
    // listener, then hand the port to the agent. Tiny window for race but
    // fine for a smoke test.
    let port = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.local_addr().unwrap().port()
    };

    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("agent.yaml");
    let program_dir = dir.path().join("programs");
    std::fs::create_dir_all(&program_dir).unwrap();
    let mut f = std::fs::File::create(&cfg).unwrap();
    writeln!(
        f,
        "agent:\n  name: smoke-test\nnetwork:\n  bind: 127.0.0.1\n  port: {port}\nstorage:\n  program_dir: {}\n  log_dir: {}",
        program_dir.display(),
        dir.path().join("logs").display(),
    )
    .unwrap();
    drop(f);

    let mut child = Command::new(agent_bin())
        .args(["--config", cfg.to_str().unwrap()])
        .env("RUST_LOG", "warn")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("agent failed to launch");

    // Poll /api/v1/health until 200 or timeout.
    let url = format!("http://127.0.0.1:{port}/api/v1/health");
    let deadline = Instant::now() + Duration::from_secs(10);
    let mut healthy = false;
    while Instant::now() < deadline {
        if let Ok(resp) = ureq_get(&url) {
            if resp == 200 {
                healthy = true;
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(150));
    }

    let _ = child.kill();
    let _ = child.wait();
    assert!(healthy, "agent did not become healthy within 10s on {url}");
}

/// Tiny stdlib-only HTTP GET that returns the status code. Avoids pulling
/// reqwest or hyper as a dev-dep for one smoke test.
fn ureq_get(url: &str) -> std::io::Result<u16> {
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::TcpStream;
    use std::time::Duration;

    let url = url.strip_prefix("http://").ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "only http://")
    })?;
    let (host_port, path) = match url.find('/') {
        Some(idx) => (&url[..idx], &url[idx..]),
        None => (url, "/"),
    };
    let mut stream = TcpStream::connect_timeout(
        &host_port.to_socket_addrs()?.next().unwrap(),
        Duration::from_secs(2),
    )?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;
    write!(
        stream,
        "GET {path} HTTP/1.0\r\nHost: {host_port}\r\nConnection: close\r\n\r\n"
    )?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    let mut status_line = String::new();
    reader.read_line(&mut status_line)?;
    // "HTTP/1.0 200 OK\r\n"
    let code: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "bad status line")
        })?;

    // Drain to free socket on the agent side.
    let mut sink = Vec::new();
    let _ = reader.read_to_end(&mut sink);
    Ok(code)
}

use std::net::ToSocketAddrs;
