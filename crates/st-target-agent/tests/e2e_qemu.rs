//! QEMU/KVM end-to-end tests for the target agent.
//!
//! These tests boot real QEMU virtual machines, deploy the agent binary via SSH,
//! and exercise the full deployment pipeline over the network.
//!
//! **Gated by `ST_E2E_QEMU=1`** — not run during normal `cargo test`.
//!
//! ## Prerequisites
//!
//! 1. QEMU installed (`qemu-system-x86_64`, `qemu-system-aarch64`)
//! 2. KVM available (`/dev/kvm` for x86_64, QEMU emulation for aarch64)
//! 3. Cloud images downloaded: `tests/e2e-deploy/vm/setup-images.sh`
//! 4. Agent built: `cargo build -p st-target-agent`
//! 5. For aarch64: `cross build -p st-target-agent --target aarch64-unknown-linux-gnu`
//!
//! ## Running
//!
//! ```bash
//! # Setup (once)
//! cd tests/e2e-deploy/vm && ./setup-images.sh
//!
//! # Run x86_64 tests
//! ST_E2E_QEMU=1 cargo test -p st-target-agent --test e2e_qemu
//!
//! # Run with aarch64 tests too
//! ST_E2E_QEMU=1 ST_E2E_AARCH64=1 cargo test -p st-target-agent --test e2e_qemu
//! ```

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

fn qemu_enabled() -> bool {
    std::env::var("ST_E2E_QEMU")
        .map(|v| v == "1")
        .unwrap_or(false)
}

fn aarch64_enabled() -> bool {
    std::env::var("ST_E2E_AARCH64")
        .map(|v| v == "1")
        .unwrap_or(false)
}

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn vm_scripts_dir() -> PathBuf {
    project_root().join("tests/e2e-deploy/vm")
}

fn fixtures_dir() -> PathBuf {
    project_root().join("tests/e2e-deploy/fixtures")
}

fn agent_binary(arch: &str) -> PathBuf {
    match arch {
        "x86_64" => project_root().join("target/debug/st-target-agent"),
        "aarch64" => project_root().join("target/aarch64-unknown-linux-gnu/debug/st-target-agent"),
        _ => panic!("Unknown arch: {arch}"),
    }
}

fn ssh_key_path() -> PathBuf {
    vm_scripts_dir().join("images/test_key")
}

struct VmHandle {
    arch: String,
    ssh_port: u16,
    agent_port: u16,
}

impl VmHandle {
    fn ssh_cmd(&self, remote_cmd: &str) -> Command {
        let mut cmd = Command::new("ssh");
        cmd.args([
            "-o", "StrictHostKeyChecking=no",
            "-o", "UserKnownHostsFile=/dev/null",
            "-o", "LogLevel=ERROR",
            "-o", "ConnectTimeout=10",
            "-i", ssh_key_path().to_str().unwrap(),
            "-p", &self.ssh_port.to_string(),
            "plc@127.0.0.1",
            remote_cmd,
        ]);
        cmd
    }

    fn scp_to(&self, local: &Path, remote: &str) -> Command {
        let mut cmd = Command::new("scp");
        cmd.args([
            "-o", "StrictHostKeyChecking=no",
            "-o", "UserKnownHostsFile=/dev/null",
            "-o", "LogLevel=ERROR",
            "-i", ssh_key_path().to_str().unwrap(),
            "-P", &self.ssh_port.to_string(),
            local.to_str().unwrap(),
            &format!("plc@127.0.0.1:{remote}"),
        ]);
        cmd
    }

    fn agent_url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.agent_port)
    }
}

impl Drop for VmHandle {
    fn drop(&mut self) {
        let _ = Command::new(vm_scripts_dir().join("stop-vm.sh").to_str().unwrap())
            .arg(&self.arch)
            .output();
    }
}

fn boot_vm(arch: &str) -> VmHandle {
    let (ssh_port, agent_port) = match arch {
        "x86_64" => (2222u16, 4840u16),
        "aarch64" => (2223u16, 4841u16),
        _ => panic!("Unknown arch: {arch}"),
    };

    // Start VM
    let output = Command::new(vm_scripts_dir().join("start-vm.sh").to_str().unwrap())
        .arg(arch)
        .output()
        .expect("Failed to start VM");

    if !output.status.success() {
        panic!(
            "VM start failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // Wait for SSH
    let output = Command::new(vm_scripts_dir().join("wait-ssh.sh").to_str().unwrap())
        .args([&ssh_port.to_string(), "90"])
        .output()
        .expect("Failed to wait for SSH");

    if !output.status.success() {
        panic!(
            "SSH not ready: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    VmHandle {
        arch: arch.to_string(),
        ssh_port,
        agent_port,
    }
}

fn deploy_agent(vm: &VmHandle) {
    let bin = agent_binary(&vm.arch);
    if !bin.exists() {
        panic!(
            "Agent binary not found at {}. Build with: cargo build -p st-target-agent",
            bin.display()
        );
    }

    // Also deploy st-cli (needed by the DAP proxy subprocess)
    let cli_bin = match vm.arch.as_str() {
        "x86_64" => project_root().join("target/debug/st-cli"),
        "aarch64" => project_root().join("target/aarch64-unknown-linux-gnu/debug/st-cli"),
        _ => panic!("Unknown arch"),
    };

    // Upload agent binary
    let output = vm.scp_to(&bin, "/tmp/st-target-agent").output().unwrap();
    assert!(output.status.success(), "SCP agent failed: {}", String::from_utf8_lossy(&output.stderr));

    // Upload st-cli binary (if it exists — skip for cross-arch if not built)
    if cli_bin.exists() {
        let output = vm.scp_to(&cli_bin, "/tmp/st-cli").output().unwrap();
        assert!(output.status.success(), "SCP st-cli failed: {}", String::from_utf8_lossy(&output.stderr));
    }

    // Make executable and create config directory
    let output = vm
        .ssh_cmd("chmod +x /tmp/st-target-agent /tmp/st-cli 2>/dev/null; sudo mkdir -p /etc/st-agent /var/lib/st-agent/programs /var/log/st-agent")
        .output()
        .unwrap();
    assert!(output.status.success(), "Setup failed: {}", String::from_utf8_lossy(&output.stderr));

    // Write minimal agent config
    let output = vm
        .ssh_cmd("sudo tee /etc/st-agent/agent.yaml << 'EOF'\nagent:\n  name: e2e-test\nnetwork:\n  bind: 0.0.0.0\n  port: 4840\nruntime:\n  restart_on_crash: true\n  max_restarts: 3\nstorage:\n  program_dir: /var/lib/st-agent/programs\n  log_dir: /var/log/st-agent\nEOF")
        .output()
        .unwrap();
    assert!(output.status.success(), "Config write failed");

    // Start agent in background
    let output = vm
        .ssh_cmd("nohup /tmp/st-target-agent --config /etc/st-agent/agent.yaml > /var/log/st-agent/stdout.log 2>&1 &")
        .output()
        .unwrap();
    assert!(output.status.success(), "Agent start failed");

    // Wait for agent to be ready
    std::thread::sleep(Duration::from_secs(2));
}

fn create_test_bundle(fixture: &str) -> Vec<u8> {
    let fixture_dir = fixtures_dir().join(fixture);
    let bundle = st_deploy::bundle::create_bundle(&fixture_dir, &st_deploy::bundle::BundleOptions::default())
        .unwrap_or_else(|e| panic!("Failed to create bundle from {fixture}: {e}"));
    let tmp = tempfile::NamedTempFile::new().unwrap();
    st_deploy::bundle::write_bundle(&bundle, tmp.path()).unwrap();
    std::fs::read(tmp.path()).unwrap()
}

fn agent_get(vm: &VmHandle, path: &str) -> (u16, serde_json::Value) {
    let url = vm.agent_url(path);
    let output = Command::new("curl")
        .args(["-s", "-w", "\n%{http_code}", &url])
        .output()
        .unwrap_or_else(|e| panic!("curl GET {url} failed: {e}"));

    let full = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = full.trim().rsplitn(2, '\n').collect();
    let status: u16 = lines[0].parse().unwrap_or(0);
    let body: serde_json::Value = if lines.len() > 1 && !lines[1].is_empty() {
        serde_json::from_str(lines[1]).unwrap_or(serde_json::Value::Null)
    } else {
        serde_json::Value::Null
    };
    (status, body)
}

fn agent_post(vm: &VmHandle, path: &str) -> (u16, serde_json::Value) {
    let url = vm.agent_url(path);
    let output = Command::new("curl")
        .args(["-s", "-X", "POST", "-w", "\n%{http_code}", &url])
        .output()
        .unwrap_or_else(|e| panic!("curl POST {url} failed: {e}"));

    let full = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = full.trim().rsplitn(2, '\n').collect();
    let status: u16 = lines[0].parse().unwrap_or(0);
    let body: serde_json::Value = if lines.len() > 1 && !lines[1].is_empty() {
        serde_json::from_str(lines[1]).unwrap_or(serde_json::Value::Null)
    } else {
        serde_json::Value::Null
    };
    (status, body)
}

fn agent_upload(vm: &VmHandle, bundle_data: &[u8]) -> (u16, serde_json::Value) {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), bundle_data).unwrap();

    let url = vm.agent_url("/api/v1/program/upload");
    let output = Command::new("curl")
        .args([
            "-s", "-X", "POST",
            "-F", &format!("file=@{}", tmp.path().display()),
            "-w", "\n%{http_code}",
            &url,
        ])
        .output()
        .unwrap_or_else(|e| panic!("curl upload failed: {e}"));

    let full = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = full.trim().rsplitn(2, '\n').collect();
    let status: u16 = lines[0].parse().unwrap_or(0);
    let body: serde_json::Value = if lines.len() > 1 && !lines[1].is_empty() {
        serde_json::from_str(lines[1]).unwrap_or(serde_json::Value::Null)
    } else {
        serde_json::Value::Null
    };
    (status, body)
}

fn agent_delete(vm: &VmHandle, path: &str) -> (u16, serde_json::Value) {
    let url = vm.agent_url(path);
    let output = Command::new("curl")
        .args(["-s", "-X", "DELETE", "-w", "\n%{http_code}", &url])
        .output()
        .unwrap_or_else(|e| panic!("curl DELETE {url} failed: {e}"));

    let full = String::from_utf8_lossy(&output.stdout);
    let lines: Vec<&str> = full.trim().rsplitn(2, '\n').collect();
    let status: u16 = lines[0].parse().unwrap_or(0);
    let body: serde_json::Value = if lines.len() > 1 && !lines[1].is_empty() {
        serde_json::from_str(lines[1]).unwrap_or(serde_json::Value::Null)
    } else {
        serde_json::Value::Null
    };
    (status, body)
}

// ─── x86_64 E2E Tests ──────────────────────────────────────────────────

#[test]
fn e2e_x86_64_bootstrap_and_health() {
    if !qemu_enabled() { eprintln!("Skipping (set ST_E2E_QEMU=1)"); return; }

    let vm = boot_vm("x86_64");
    deploy_agent(&vm);

    let (status, body) = agent_get(&vm, "/api/v1/health");
    assert_eq!(status, 200, "Health check failed: {body}");
    assert_eq!(body["healthy"], true);
}

#[test]
fn e2e_x86_64_target_info() {
    if !qemu_enabled() { eprintln!("Skipping (set ST_E2E_QEMU=1)"); return; }

    let vm = boot_vm("x86_64");
    deploy_agent(&vm);

    let (status, body) = agent_get(&vm, "/api/v1/target-info");
    assert_eq!(status, 200);
    assert_eq!(body["os"], "linux");
    assert_eq!(body["arch"], "x86_64");
    assert!(body["agent_version"].as_str().is_some());
}

#[test]
fn e2e_x86_64_upload_bundle() {
    if !qemu_enabled() { eprintln!("Skipping (set ST_E2E_QEMU=1)"); return; }

    let vm = boot_vm("x86_64");
    deploy_agent(&vm);

    let bundle = create_test_bundle("test-project");
    let (status, body) = agent_upload(&vm, &bundle);
    assert_eq!(status, 200, "Upload failed: {body}");
    assert_eq!(body["success"], true);
    assert_eq!(body["program"]["name"], "E2ETestProject");

    // Verify program info
    let (status, body) = agent_get(&vm, "/api/v1/program/info");
    assert_eq!(status, 200);
    assert_eq!(body["name"], "E2ETestProject");
    assert_eq!(body["version"], "1.0.0");
}

#[test]
fn e2e_x86_64_start_and_status() {
    if !qemu_enabled() { eprintln!("Skipping (set ST_E2E_QEMU=1)"); return; }

    let vm = boot_vm("x86_64");
    deploy_agent(&vm);

    let bundle = create_test_bundle("test-project");
    agent_upload(&vm, &bundle);

    let (status, _) = agent_post(&vm, "/api/v1/program/start");
    assert_eq!(status, 200);

    std::thread::sleep(Duration::from_millis(500));

    let (status, body) = agent_get(&vm, "/api/v1/status");
    assert_eq!(status, 200);
    assert_eq!(body["status"], "running");
    let cycle_count = body["cycle_stats"]["cycle_count"].as_u64().unwrap_or(0);
    assert!(cycle_count > 0, "Cycle count should be > 0, got {cycle_count}");
}

#[test]
fn e2e_x86_64_stop() {
    if !qemu_enabled() { eprintln!("Skipping (set ST_E2E_QEMU=1)"); return; }

    let vm = boot_vm("x86_64");
    deploy_agent(&vm);

    let bundle = create_test_bundle("test-project");
    agent_upload(&vm, &bundle);
    agent_post(&vm, "/api/v1/program/start");
    std::thread::sleep(Duration::from_millis(500));

    let (status, _) = agent_post(&vm, "/api/v1/program/stop");
    assert_eq!(status, 200);
    std::thread::sleep(Duration::from_millis(200));

    let (_, body) = agent_get(&vm, "/api/v1/status");
    assert_eq!(body["status"], "idle");
}

#[test]
fn e2e_x86_64_restart() {
    if !qemu_enabled() { eprintln!("Skipping (set ST_E2E_QEMU=1)"); return; }

    let vm = boot_vm("x86_64");
    deploy_agent(&vm);

    let bundle = create_test_bundle("test-project");
    agent_upload(&vm, &bundle);
    agent_post(&vm, "/api/v1/program/start");
    std::thread::sleep(Duration::from_millis(500));

    let (status, _) = agent_post(&vm, "/api/v1/program/restart");
    assert_eq!(status, 200);
    std::thread::sleep(Duration::from_millis(500));

    let (_, body) = agent_get(&vm, "/api/v1/status");
    assert_eq!(body["status"], "running");
}

#[test]
fn e2e_x86_64_delete_program() {
    if !qemu_enabled() { eprintln!("Skipping (set ST_E2E_QEMU=1)"); return; }

    let vm = boot_vm("x86_64");
    deploy_agent(&vm);

    let bundle = create_test_bundle("test-project");
    agent_upload(&vm, &bundle);

    let (status, _) = agent_delete(&vm, "/api/v1/program");
    assert_eq!(status, 200);

    let (status, _) = agent_get(&vm, "/api/v1/program/info");
    assert_eq!(status, 404);
}

#[test]
fn e2e_x86_64_health_while_running() {
    if !qemu_enabled() { eprintln!("Skipping (set ST_E2E_QEMU=1)"); return; }

    let vm = boot_vm("x86_64");
    deploy_agent(&vm);

    let bundle = create_test_bundle("test-project");
    agent_upload(&vm, &bundle);
    agent_post(&vm, "/api/v1/program/start");
    std::thread::sleep(Duration::from_millis(500));

    let (status, body) = agent_get(&vm, "/api/v1/health");
    assert_eq!(status, 200);
    assert_eq!(body["healthy"], true);
}

#[test]
fn e2e_x86_64_logs_endpoint() {
    if !qemu_enabled() { eprintln!("Skipping (set ST_E2E_QEMU=1)"); return; }

    let vm = boot_vm("x86_64");
    deploy_agent(&vm);

    let (status, body) = agent_get(&vm, "/api/v1/logs");
    assert_eq!(status, 200);
    assert!(body["entries"].as_array().is_some());
}

#[test]
fn e2e_x86_64_upload_then_start() {
    if !qemu_enabled() { eprintln!("Skipping (set ST_E2E_QEMU=1)"); return; }

    let vm = boot_vm("x86_64");
    deploy_agent(&vm);

    let bundle = create_test_bundle("test-project");
    let (upload_status, _) = agent_upload(&vm, &bundle);
    assert_eq!(upload_status, 200);

    let (start_status, _) = agent_post(&vm, "/api/v1/program/start");
    assert_eq!(start_status, 200);

    std::thread::sleep(Duration::from_millis(500));

    let (_, body) = agent_get(&vm, "/api/v1/status");
    assert_eq!(body["status"], "running");
    assert!(body["cycle_stats"]["cycle_count"].as_u64().unwrap_or(0) > 0);
}

#[test]
fn e2e_x86_64_update_with_v2() {
    if !qemu_enabled() { eprintln!("Skipping (set ST_E2E_QEMU=1)"); return; }

    let vm = boot_vm("x86_64");
    deploy_agent(&vm);

    // Deploy and start v1
    let bundle_v1 = create_test_bundle("test-project");
    agent_upload(&vm, &bundle_v1);
    agent_post(&vm, "/api/v1/program/start");
    std::thread::sleep(Duration::from_millis(500));

    // Upload v2 (stop + replace + start)
    agent_post(&vm, "/api/v1/program/stop");
    std::thread::sleep(Duration::from_millis(200));

    let bundle_v2 = create_test_bundle("test-project-v2");
    let (status, body) = agent_upload(&vm, &bundle_v2);
    assert_eq!(status, 200);
    assert_eq!(body["program"]["version"], "2.0.0");

    agent_post(&vm, "/api/v1/program/start");
    std::thread::sleep(Duration::from_millis(500));

    let (_, body) = agent_get(&vm, "/api/v1/status");
    assert_eq!(body["status"], "running");

    let (_, info) = agent_get(&vm, "/api/v1/program/info");
    assert_eq!(info["version"], "2.0.0");
}

#[test]
fn e2e_x86_64_update_with_v3_incompatible() {
    if !qemu_enabled() { eprintln!("Skipping (set ST_E2E_QEMU=1)"); return; }

    let vm = boot_vm("x86_64");
    deploy_agent(&vm);

    // Deploy v1
    let bundle_v1 = create_test_bundle("test-project");
    agent_upload(&vm, &bundle_v1);
    agent_post(&vm, "/api/v1/program/start");
    std::thread::sleep(Duration::from_millis(500));

    // Upload v3 with different layout
    agent_post(&vm, "/api/v1/program/stop");
    std::thread::sleep(Duration::from_millis(200));

    let bundle_v3 = create_test_bundle("test-project-v3");
    let (status, body) = agent_upload(&vm, &bundle_v3);
    assert_eq!(status, 200);
    assert_eq!(body["program"]["version"], "3.0.0");

    // Start v3 — full restart
    agent_post(&vm, "/api/v1/program/start");
    std::thread::sleep(Duration::from_millis(500));

    let (_, body) = agent_get(&vm, "/api/v1/status");
    assert_eq!(body["status"], "running");
}

#[test]
fn e2e_x86_64_full_lifecycle() {
    if !qemu_enabled() { eprintln!("Skipping (set ST_E2E_QEMU=1)"); return; }

    let vm = boot_vm("x86_64");
    deploy_agent(&vm);

    // Health
    let (s, _) = agent_get(&vm, "/api/v1/health");
    assert_eq!(s, 200);

    // Status = idle
    let (_, b) = agent_get(&vm, "/api/v1/status");
    assert_eq!(b["status"], "idle");

    // Upload
    let bundle = create_test_bundle("test-project");
    let (s, _) = agent_upload(&vm, &bundle);
    assert_eq!(s, 200);

    // Info
    let (s, b) = agent_get(&vm, "/api/v1/program/info");
    assert_eq!(s, 200);
    assert_eq!(b["name"], "E2ETestProject");

    // Start
    let (s, _) = agent_post(&vm, "/api/v1/program/start");
    assert_eq!(s, 200);
    std::thread::sleep(Duration::from_millis(500));

    // Running with advancing cycles
    let (_, b) = agent_get(&vm, "/api/v1/status");
    assert_eq!(b["status"], "running");
    let c1 = b["cycle_stats"]["cycle_count"].as_u64().unwrap();
    assert!(c1 > 0);

    std::thread::sleep(Duration::from_millis(500));
    let (_, b) = agent_get(&vm, "/api/v1/status");
    let c2 = b["cycle_stats"]["cycle_count"].as_u64().unwrap();
    assert!(c2 > c1, "Cycles should advance: {c1} -> {c2}");

    // Stop
    let (s, _) = agent_post(&vm, "/api/v1/program/stop");
    assert_eq!(s, 200);
    std::thread::sleep(Duration::from_millis(200));
    let (_, b) = agent_get(&vm, "/api/v1/status");
    assert_eq!(b["status"], "idle");

    // Delete
    let (s, _) = agent_delete(&vm, "/api/v1/program");
    assert_eq!(s, 200);
    let (s, _) = agent_get(&vm, "/api/v1/program/info");
    assert_eq!(s, 404);
}

// ─── aarch64 E2E Tests ─────────────────────────────────────────────────

#[test]
fn e2e_aarch64_bootstrap_and_health() {
    if !qemu_enabled() || !aarch64_enabled() {
        eprintln!("Skipping (set ST_E2E_QEMU=1 ST_E2E_AARCH64=1)");
        return;
    }

    let vm = boot_vm("aarch64");
    deploy_agent(&vm);

    let (status, body) = agent_get(&vm, "/api/v1/health");
    assert_eq!(status, 200, "ARM64 health check failed: {body}");
    assert_eq!(body["healthy"], true);
}

#[test]
fn e2e_aarch64_upload_and_run() {
    if !qemu_enabled() || !aarch64_enabled() {
        eprintln!("Skipping (set ST_E2E_QEMU=1 ST_E2E_AARCH64=1)");
        return;
    }

    let vm = boot_vm("aarch64");
    deploy_agent(&vm);

    let bundle = create_test_bundle("test-project");
    let (status, _) = agent_upload(&vm, &bundle);
    assert_eq!(status, 200);

    let (status, _) = agent_post(&vm, "/api/v1/program/start");
    assert_eq!(status, 200);
    std::thread::sleep(Duration::from_millis(1000)); // ARM emulation is slower

    let (_, body) = agent_get(&vm, "/api/v1/status");
    assert_eq!(body["status"], "running");
    assert!(body["cycle_stats"]["cycle_count"].as_u64().unwrap_or(0) > 0);
}

#[test]
fn e2e_aarch64_full_lifecycle() {
    if !qemu_enabled() || !aarch64_enabled() {
        eprintln!("Skipping (set ST_E2E_QEMU=1 ST_E2E_AARCH64=1)");
        return;
    }

    let vm = boot_vm("aarch64");
    deploy_agent(&vm);

    // Upload
    let bundle = create_test_bundle("test-project");
    let (s, _) = agent_upload(&vm, &bundle);
    assert_eq!(s, 200);

    // Start
    let (s, _) = agent_post(&vm, "/api/v1/program/start");
    assert_eq!(s, 200);
    std::thread::sleep(Duration::from_millis(1000));

    let (_, b) = agent_get(&vm, "/api/v1/status");
    assert_eq!(b["status"], "running");

    // Target info shows ARM
    let (_, info) = agent_get(&vm, "/api/v1/target-info");
    assert_eq!(info["os"], "linux");
    assert_eq!(info["arch"], "aarch64");

    // Stop
    let (s, _) = agent_post(&vm, "/api/v1/program/stop");
    assert_eq!(s, 200);
    std::thread::sleep(Duration::from_millis(500));

    let (_, b) = agent_get(&vm, "/api/v1/status");
    assert_eq!(b["status"], "idle");

    // Delete
    let (s, _) = agent_delete(&vm, "/api/v1/program");
    assert_eq!(s, 200);
}

// ─── DAP Remote Debug via Direct Port Forwarding ────────────────────────

/// DAP wire protocol helpers (Content-Length framing)
fn send_dap(stream: &mut TcpStream, seq: i64, command: &str, args: serde_json::Value) {
    let msg = if args.is_null() {
        serde_json::json!({ "seq": seq, "type": "request", "command": command })
    } else {
        serde_json::json!({ "seq": seq, "type": "request", "command": command, "arguments": args })
    };
    let json = serde_json::to_string(&msg).unwrap();
    let framed = format!("Content-Length: {}\r\n\r\n{}", json.len(), json);
    stream.write_all(framed.as_bytes()).unwrap();
    stream.flush().unwrap();
}

fn read_dap_until(
    reader: &mut BufReader<TcpStream>,
    predicate: impl Fn(&serde_json::Value) -> bool,
    timeout_ms: u64,
) -> serde_json::Value {
    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);
    reader.get_ref().set_read_timeout(Some(Duration::from_millis(500))).unwrap();

    loop {
        if std::time::Instant::now() > deadline {
            panic!("Timeout waiting for DAP message");
        }
        if let Some(msg) = try_read_dap(reader) {
            if predicate(&msg) {
                reader.get_ref().set_read_timeout(None).unwrap();
                return msg;
            }
        }
    }
}

fn try_read_dap(reader: &mut BufReader<TcpStream>) -> Option<serde_json::Value> {
    let mut content_length: usize = 0;
    loop {
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => return None,
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock || e.kind() == std::io::ErrorKind::TimedOut => return None,
            Err(_) => return None,
        }
        let trimmed = line.trim();
        if trimmed.is_empty() { break; }
        if let Some(rest) = trimmed.strip_prefix("Content-Length:") {
            content_length = rest.trim().parse().unwrap_or(0);
        }
    }
    if content_length == 0 { return None; }
    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body).ok()?;
    serde_json::from_slice(&body).ok()
}

#[test]
fn e2e_x86_64_remote_debug_direct_port() {
    if !qemu_enabled() { eprintln!("Skipping (set ST_E2E_QEMU=1)"); return; }

    let vm = boot_vm("x86_64");
    deploy_agent(&vm);

    // Upload development bundle (with source)
    let bundle = create_test_bundle("test-project");
    let (s, _) = agent_upload(&vm, &bundle);
    assert_eq!(s, 200);

    std::thread::sleep(Duration::from_secs(1));

    // Connect to DAP proxy via direct port forwarding (host:4840+1 → guest:4841)
    let dap_port = vm.agent_port + 1;
    let stream = TcpStream::connect(format!("127.0.0.1:{dap_port}"))
        .expect("Cannot connect to DAP proxy");
    stream.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
    let reader_stream = stream.try_clone().unwrap();
    let mut reader = BufReader::new(reader_stream);
    let mut writer = stream;

    // Initialize
    send_dap(&mut writer, 1, "initialize", serde_json::json!({
        "adapterID": "st", "clientID": "qemu-e2e"
    }));
    let resp = read_dap_until(&mut reader, |m| m["type"] == "response" && m["command"] == "initialize", 15000);
    assert_eq!(resp["success"], true, "Initialize failed: {resp}");

    // Launch with stopOnEntry
    send_dap(&mut writer, 2, "launch", serde_json::json!({ "stopOnEntry": true }));
    let resp = read_dap_until(&mut reader, |m| m["type"] == "response" && m["command"] == "launch", 15000);
    assert_eq!(resp["success"], true, "Launch failed: {resp}");

    // Wait for Stopped event (entry)
    let stopped = read_dap_until(&mut reader, |m| m["type"] == "event" && m["event"] == "stopped", 15000);
    assert_eq!(stopped["body"]["reason"], "entry");

    // Wait for Initialized
    let _ = read_dap_until(&mut reader, |m| m["type"] == "event" && m["event"] == "initialized", 5000);

    // ConfigurationDone
    send_dap(&mut writer, 3, "configurationDone", serde_json::Value::Null);
    let _ = read_dap_until(&mut reader, |m| m["type"] == "response" && m["command"] == "configurationDone", 5000);

    // StackTrace — verify we're in Main
    send_dap(&mut writer, 4, "stackTrace", serde_json::json!({
        "threadId": 1, "startFrame": 0, "levels": 10
    }));
    let st = read_dap_until(&mut reader, |m| m["type"] == "response" && m["command"] == "stackTrace", 5000);
    let frames = st["body"]["stackFrames"].as_array().unwrap();
    assert!(!frames.is_empty(), "Should have stack frames");
    assert!(frames[0]["name"].as_str().unwrap().contains("Main"), "Top frame should be Main");

    // Scopes + Variables — verify counter exists
    let frame_id = frames[0]["id"].as_i64().unwrap();
    send_dap(&mut writer, 5, "scopes", serde_json::json!({ "frameId": frame_id }));
    let scopes = read_dap_until(&mut reader, |m| m["type"] == "response" && m["command"] == "scopes", 5000);
    let locals_ref = scopes["body"]["scopes"].as_array().unwrap()
        .iter().find(|s| s["name"] == "Locals").unwrap()["variablesReference"].as_i64().unwrap();

    send_dap(&mut writer, 6, "variables", serde_json::json!({ "variablesReference": locals_ref }));
    let vars = read_dap_until(&mut reader, |m| m["type"] == "response" && m["command"] == "variables", 5000);
    let variables = vars["body"]["variables"].as_array().unwrap();
    let has_counter = variables.iter().any(|v| v["name"].as_str().map(|n| n.eq_ignore_ascii_case("counter")).unwrap_or(false));
    assert!(has_counter, "Should find counter variable: {variables:?}");

    // Step
    send_dap(&mut writer, 7, "next", serde_json::json!({ "threadId": 1 }));
    let _ = read_dap_until(&mut reader, |m| m["type"] == "event" && m["event"] == "stopped", 15000);

    // Disconnect
    send_dap(&mut writer, 8, "disconnect", serde_json::json!({ "terminateDebuggee": true }));
}

#[test]
fn e2e_x86_64_remote_debug_ssh_tunnel() {
    if !qemu_enabled() { eprintln!("Skipping (set ST_E2E_QEMU=1)"); return; }

    let vm = boot_vm("x86_64");
    deploy_agent(&vm);

    // Upload development bundle
    let bundle = create_test_bundle("test-project");
    agent_upload(&vm, &bundle);
    std::thread::sleep(Duration::from_secs(1));

    // Create SSH tunnel: local:14841 → remote:4841 (DAP proxy)
    let tunnel_local_port = 14841u16;
    let tunnel = Command::new("ssh")
        .args([
            "-o", "StrictHostKeyChecking=no",
            "-o", "UserKnownHostsFile=/dev/null",
            "-o", "LogLevel=ERROR",
            "-i", ssh_key_path().to_str().unwrap(),
            "-p", &vm.ssh_port.to_string(),
            "-L", &format!("{tunnel_local_port}:127.0.0.1:4841"),
            "-N", "-f",  // background, no command
            "plc@127.0.0.1",
        ])
        .spawn()
        .expect("Failed to create SSH tunnel");

    // Wait for tunnel to establish
    std::thread::sleep(Duration::from_secs(2));

    // Connect via the SSH tunnel
    let stream = TcpStream::connect(format!("127.0.0.1:{tunnel_local_port}"))
        .expect("Cannot connect via SSH tunnel");
    stream.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
    let reader_stream = stream.try_clone().unwrap();
    let mut reader = BufReader::new(reader_stream);
    let mut writer = stream;

    // Initialize
    send_dap(&mut writer, 1, "initialize", serde_json::json!({
        "adapterID": "st", "clientID": "ssh-tunnel-e2e"
    }));
    let resp = read_dap_until(&mut reader, |m| m["type"] == "response" && m["command"] == "initialize", 15000);
    assert_eq!(resp["success"], true, "Initialize via SSH tunnel failed: {resp}");

    // Launch
    send_dap(&mut writer, 2, "launch", serde_json::json!({ "stopOnEntry": true }));
    let resp = read_dap_until(&mut reader, |m| m["type"] == "response" && m["command"] == "launch", 15000);
    assert_eq!(resp["success"], true);

    // Stopped on entry
    let stopped = read_dap_until(&mut reader, |m| m["type"] == "event" && m["event"] == "stopped", 15000);
    assert_eq!(stopped["body"]["reason"], "entry");

    // Verify stack trace works through the tunnel
    let _ = read_dap_until(&mut reader, |m| m["type"] == "event" && m["event"] == "initialized", 5000);
    send_dap(&mut writer, 3, "configurationDone", serde_json::Value::Null);
    let _ = read_dap_until(&mut reader, |m| m["type"] == "response" && m["command"] == "configurationDone", 5000);

    send_dap(&mut writer, 4, "stackTrace", serde_json::json!({
        "threadId": 1, "startFrame": 0, "levels": 10
    }));
    let st = read_dap_until(&mut reader, |m| m["type"] == "response" && m["command"] == "stackTrace", 5000);
    assert!(st["body"]["stackFrames"].as_array().unwrap().len() > 0, "Stack trace via SSH tunnel should work");

    // Disconnect
    send_dap(&mut writer, 5, "disconnect", serde_json::json!({ "terminateDebuggee": true }));

    // Clean up tunnel (the -f flag backgrounds it, kill by port)
    let _ = Command::new("pkill").args(["-f", &format!("{tunnel_local_port}:127.0.0.1:4841")]).output();
}

#[test]
fn e2e_x86_64_remote_debug_release_rejected() {
    if !qemu_enabled() { eprintln!("Skipping (set ST_E2E_QEMU=1)"); return; }

    let vm = boot_vm("x86_64");
    deploy_agent(&vm);

    // Upload a release bundle (no source)
    let release_bundle = {
        let fixture_dir = fixtures_dir().join("test-project");
        let bundle = st_deploy::bundle::create_bundle(
            &fixture_dir,
            &st_deploy::bundle::BundleOptions {
                mode: st_deploy::BundleMode::Release,
                ..Default::default()
            },
        ).unwrap();
        let tmp = tempfile::NamedTempFile::new().unwrap();
        st_deploy::bundle::write_bundle(&bundle, tmp.path()).unwrap();
        std::fs::read(tmp.path()).unwrap()
    };

    agent_upload(&vm, &release_bundle);
    std::thread::sleep(Duration::from_secs(1));

    // Try to connect to DAP proxy — should be rejected (release bundle)
    let dap_port = vm.agent_port + 1;
    let result = TcpStream::connect_timeout(
        &format!("127.0.0.1:{dap_port}").parse().unwrap(),
        Duration::from_secs(3),
    );

    match result {
        Ok(stream) => {
            // Connection accepted but should close immediately
            stream.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut buf = [0u8; 1];
            let n = stream.peek(&mut buf).unwrap_or(0);
            assert_eq!(n, 0, "DAP proxy should reject release bundle");
        }
        Err(_) => {
            // Connection refused — also acceptable
        }
    }
}

#[test]
fn e2e_x86_64_remote_debug_update_during_session() {
    if !qemu_enabled() { eprintln!("Skipping (set ST_E2E_QEMU=1)"); return; }

    let vm = boot_vm("x86_64");
    deploy_agent(&vm);

    // Upload v1 and start debug session
    let bundle_v1 = create_test_bundle("test-project");
    agent_upload(&vm, &bundle_v1);
    std::thread::sleep(Duration::from_secs(1));

    let dap_port = vm.agent_port + 1;
    let stream = TcpStream::connect(format!("127.0.0.1:{dap_port}"))
        .expect("Cannot connect to DAP proxy");
    stream.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
    let reader_stream = stream.try_clone().unwrap();
    let mut reader = BufReader::new(reader_stream);
    let mut writer = stream;

    // Initialize + Launch v1
    send_dap(&mut writer, 1, "initialize", serde_json::json!({ "adapterID": "st" }));
    let _ = read_dap_until(&mut reader, |m| m["type"] == "response" && m["command"] == "initialize", 15000);
    send_dap(&mut writer, 2, "launch", serde_json::json!({ "stopOnEntry": true }));
    let _ = read_dap_until(&mut reader, |m| m["type"] == "response" && m["command"] == "launch", 15000);
    let _ = read_dap_until(&mut reader, |m| m["type"] == "event" && m["event"] == "stopped", 15000);

    // Verify v1 program info
    let (_, info) = agent_get(&vm, "/api/v1/program/info");
    assert_eq!(info["version"], "1.0.0");

    // Disconnect the debug session
    send_dap(&mut writer, 3, "disconnect", serde_json::json!({ "terminateDebuggee": true }));
    std::thread::sleep(Duration::from_secs(1));

    // Upload v2
    let bundle_v2 = create_test_bundle("test-project-v2");
    let (s, _) = agent_upload(&vm, &bundle_v2);
    assert_eq!(s, 200);

    // Verify v2 info
    let (_, info) = agent_get(&vm, "/api/v1/program/info");
    assert_eq!(info["version"], "2.0.0");

    // Re-connect and debug v2
    std::thread::sleep(Duration::from_secs(1));
    let stream2 = TcpStream::connect(format!("127.0.0.1:{dap_port}"))
        .expect("Cannot reconnect to DAP proxy for v2");
    stream2.set_read_timeout(Some(Duration::from_secs(15))).unwrap();
    let reader_stream2 = stream2.try_clone().unwrap();
    let mut reader2 = BufReader::new(reader_stream2);
    let mut writer2 = stream2;

    send_dap(&mut writer2, 1, "initialize", serde_json::json!({ "adapterID": "st" }));
    let _ = read_dap_until(&mut reader2, |m| m["type"] == "response" && m["command"] == "initialize", 15000);
    send_dap(&mut writer2, 2, "launch", serde_json::json!({ "stopOnEntry": true }));
    let _ = read_dap_until(&mut reader2, |m| m["type"] == "response" && m["command"] == "launch", 15000);
    let stopped = read_dap_until(&mut reader2, |m| m["type"] == "event" && m["event"] == "stopped", 15000);
    assert_eq!(stopped["body"]["reason"], "entry", "V2 should stop on entry");

    send_dap(&mut writer2, 3, "disconnect", serde_json::json!({ "terminateDebuggee": true }));
}
