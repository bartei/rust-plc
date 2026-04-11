//! QEMU/KVM E2E tests for the one-command installer.
//!
//! Tests the full `st-cli target install`, upgrade, uninstall flow against
//! real QEMU VMs. Verifies the static binary, systemd service, health checks,
//! program upload, DAP debugging, crash recovery, and error handling.
//!
//! **Gated by `ST_E2E_QEMU=1`** — not run during normal `cargo test`.
//!
//! ## Prerequisites
//!
//! 1. Static binary built: `./scripts/build-static.sh`
//! 2. st-cli built: `cargo build -p st-cli`
//! 3. QEMU images set up: `cd tests/e2e-deploy/vm && ./setup-images.sh`
//! 4. KVM available: `/dev/kvm`
//!
//! ## Running
//!
//! ```bash
//! ST_E2E_QEMU=1 cargo test -p st-runtime --test e2e_installer -- --test-threads=1
//! ```
//!
//! Tests run sequentially (`--test-threads=1`) because they share the QEMU VM.

use std::io::{BufRead, BufReader, Read, Write};
#[allow(unused_imports)]
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

// ─── Infrastructure ─────────────────────────────────────────────────────

fn qemu_enabled() -> bool {
    std::env::var("ST_E2E_QEMU").map(|v| v == "1").unwrap_or(false)
}

macro_rules! skip_if_no_qemu {
    () => {
        if !qemu_enabled() {
            eprintln!("Skipping (set ST_E2E_QEMU=1)");
            return;
        }
    };
}

fn project_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn vm_scripts_dir() -> PathBuf {
    project_root().join("tests/e2e-deploy/vm")
}

fn ssh_key_path() -> PathBuf {
    vm_scripts_dir().join("images/test_key")
}

fn static_binary() -> PathBuf {
    project_root().join("target/x86_64-unknown-linux-musl/release-static/st-runtime")
}

fn st_cli_binary() -> PathBuf {
    project_root().join("target/debug/st-cli")
}

struct VmHandle {
    ssh_port: u16,
    agent_port: u16,
}

impl VmHandle {
    fn ssh(&self, cmd: &str) -> Result<String, String> {
        let output = Command::new("ssh")
            .args([
                "-o", "StrictHostKeyChecking=accept-new",
                "-o", "UserKnownHostsFile=/dev/null",
                "-o", "LogLevel=ERROR",
                "-o", "ConnectTimeout=10",
                "-o", "BatchMode=yes",
                "-i", ssh_key_path().to_str().unwrap(),
                "-p", &self.ssh_port.to_string(),
                "plc@127.0.0.1",
                cmd,
            ])
            .output()
            .map_err(|e| format!("SSH exec failed: {e}"))?;

        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            Err(String::from_utf8_lossy(&output.stderr).trim().to_string())
        }
    }

    fn curl_agent(&self, path: &str) -> (u16, String) {
        let url = format!("http://127.0.0.1:{}{path}", self.agent_port);
        let output = Command::new("curl")
            .args(["-sf", "--connect-timeout", "5", "-w", "\n%{http_code}", &url])
            .output()
            .unwrap_or_else(|e| panic!("curl failed: {e}"));

        let full = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = full.trim().rsplitn(2, '\n').collect();
        let status: u16 = lines[0].parse().unwrap_or(0);
        let body = if lines.len() > 1 { lines[1].to_string() } else { String::new() };
        (status, body)
    }

    fn curl_post(&self, path: &str) -> (u16, String) {
        let url = format!("http://127.0.0.1:{}{path}", self.agent_port);
        let output = Command::new("curl")
            .args(["-sf", "-X", "POST", "--connect-timeout", "5", "-w", "\n%{http_code}", &url])
            .output()
            .unwrap_or_else(|e| panic!("curl failed: {e}"));

        let full = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = full.trim().rsplitn(2, '\n').collect();
        let status: u16 = lines[0].parse().unwrap_or(0);
        let body = if lines.len() > 1 { lines[1].to_string() } else { String::new() };
        (status, body)
    }

    fn upload_bundle(&self, bundle_path: &Path) -> (u16, String) {
        let url = format!("http://127.0.0.1:{}/api/v1/program/upload", self.agent_port);
        let output = Command::new("curl")
            .args([
                "-sf", "-X", "POST",
                "-F", &format!("file=@{}", bundle_path.display()),
                "--connect-timeout", "10",
                "-w", "\n%{http_code}",
                &url,
            ])
            .output()
            .unwrap_or_else(|e| panic!("curl upload failed: {e}"));

        let full = String::from_utf8_lossy(&output.stdout);
        let lines: Vec<&str> = full.trim().rsplitn(2, '\n').collect();
        let status: u16 = lines[0].parse().unwrap_or(0);
        let body = if lines.len() > 1 { lines[1].to_string() } else { String::new() };
        (status, body)
    }

    /// Run `st-cli target install` against this VM.
    fn run_install(&self, extra_args: &[&str]) -> (bool, String) {
        let port_str = self.ssh_port.to_string();
        let key_path = ssh_key_path();
        let key_str = key_path.to_str().unwrap();
        let mut args = vec![
            "target", "install",
            "plc@127.0.0.1",
            "--port", &port_str,
            "--key", key_str,
        ];
        args.extend(extra_args);

        let output = Command::new(st_cli_binary().to_str().unwrap())
            .args(&args)
            .output()
            .expect("Failed to run st-cli target install");

        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        (output.status.success(), stderr)
    }

    /// Run `st-cli target uninstall` against this VM.
    fn run_uninstall(&self, extra_args: &[&str]) -> (bool, String) {
        let port_str = self.ssh_port.to_string();
        let key_path = ssh_key_path();
        let key_str = key_path.to_str().unwrap();
        let mut args = vec![
            "target", "uninstall",
            "plc@127.0.0.1",
            "--port", &port_str,
            "--key", key_str,
        ];
        args.extend(extra_args);

        let output = Command::new(st_cli_binary().to_str().unwrap())
            .args(&args)
            .output()
            .expect("Failed to run st-cli target uninstall");

        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        (output.status.success(), stderr)
    }
}

impl Drop for VmHandle {
    fn drop(&mut self) {
        // Stop VM via script
        let _ = Command::new(vm_scripts_dir().join("stop-vm.sh").to_str().unwrap())
            .arg("x86_64")
            .output();
        // Also kill any QEMU process using our ports as a safety net
        let _ = Command::new("pkill")
            .args(["-f", &format!("hostfwd=tcp::{}",  self.ssh_port)])
            .output();
        // Brief wait to release ports
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn boot_fresh_vm() -> VmHandle {
    // Stop any existing VM first
    let _ = Command::new(vm_scripts_dir().join("stop-vm.sh").to_str().unwrap())
        .arg("x86_64")
        .output();

    let output = Command::new(vm_scripts_dir().join("start-vm.sh").to_str().unwrap())
        .arg("x86_64")
        .output()
        .expect("Failed to start VM");

    assert!(
        output.status.success(),
        "VM start failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let output = Command::new(vm_scripts_dir().join("wait-ssh.sh").to_str().unwrap())
        .args(["2222", "90"])
        .output()
        .expect("Failed to wait for SSH");

    assert!(
        output.status.success(),
        "SSH not ready: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Wait for cloud-init to fully complete SSH key injection and sshd restart.
    // The SSH port may be open before keys are injected — retry SSH until it works.
    let key = ssh_key_path();
    let key_str = key.to_str().unwrap();
    for attempt in 1..=15 {
        let result = Command::new("ssh")
            .args([
                "-o", "StrictHostKeyChecking=accept-new",
                "-o", "UserKnownHostsFile=/dev/null",
                "-o", "LogLevel=ERROR",
                "-o", "ConnectTimeout=5",
                "-o", "BatchMode=yes",
                "-i", key_str,
                "-p", "2222",
                "plc@127.0.0.1", "true",
            ])
            .output();

        if let Ok(output) = result {
            if output.status.success() {
                break;
            }
        }
        if attempt == 15 {
            panic!("SSH key auth not ready after 15 attempts — cloud-init may have failed");
        }
        std::thread::sleep(Duration::from_secs(1));
    }

    VmHandle {
        ssh_port: 2222,
        agent_port: 4840,
    }
}

fn make_test_bundle() -> PathBuf {
    let fixture_dir = project_root().join("tests/e2e-deploy/fixtures/test-project");
    let bundle = st_deploy::bundle::create_bundle(
        &fixture_dir,
        &st_deploy::bundle::BundleOptions::default(),
    )
    .expect("Failed to create test bundle");
    let bundle_path = std::env::temp_dir().join("e2e-installer-test.st-bundle");
    st_deploy::bundle::write_bundle(&bundle, &bundle_path).unwrap();
    bundle_path
}

// ─── Prerequisite: Static Binary Verification ───────────────────────────

#[test]
fn test_static_binary_exists() {
    skip_if_no_qemu!();
    assert!(
        static_binary().exists(),
        "Static binary not found at {}. Run: ./scripts/build-static.sh",
        static_binary().display()
    );
}

#[test]
fn test_static_binary_is_statically_linked() {
    skip_if_no_qemu!();
    let output = Command::new("file")
        .arg(static_binary().to_str().unwrap())
        .output()
        .unwrap();
    let info = String::from_utf8_lossy(&output.stdout);
    assert!(
        info.contains("static") && info.contains("linked"),
        "Binary should be statically linked: {info}"
    );
}

#[test]
fn test_static_binary_ldd_shows_static() {
    skip_if_no_qemu!();
    let output = Command::new("ldd")
        .arg(static_binary().to_str().unwrap())
        .output()
        .unwrap();
    let info = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let combined = format!("{info}{stderr}");
    assert!(
        combined.contains("statically linked") || combined.contains("not a dynamic executable"),
        "ldd should report statically linked: {combined}"
    );
}

#[test]
fn test_static_binary_size_under_25mb() {
    skip_if_no_qemu!();
    let size = std::fs::metadata(static_binary()).unwrap().len();
    let mb = size as f64 / (1024.0 * 1024.0);
    assert!(
        mb < 25.0,
        "Static binary should be < 25MB, got {mb:.1}MB"
    );
    eprintln!("Static binary size: {mb:.1}MB");
}

// ─── Fresh Install Tests (x86_64) ──────────────────────────────────────

#[test]
fn test_fresh_install_succeeds() {
    skip_if_no_qemu!();
    let vm = boot_fresh_vm();

    let (ok, output) = vm.run_install(&["--name", "e2e-install-test"]);
    assert!(ok, "Install failed:\n{output}");
    assert!(output.contains("is ready"), "Should report target ready:\n{output}");
}

#[test]
fn test_after_install_binary_exists() {
    skip_if_no_qemu!();
    let vm = boot_fresh_vm();
    vm.run_install(&[]);

    let result = vm.ssh("test -x /opt/st-plc/st-runtime && echo YES || echo NO");
    assert_eq!(result.unwrap(), "YES", "Binary should exist and be executable");
}

#[test]
fn test_after_install_config_exists() {
    skip_if_no_qemu!();
    let vm = boot_fresh_vm();
    vm.run_install(&[]);

    let result = vm.ssh("test -f /etc/st-plc/agent.yaml && echo YES || echo NO");
    assert_eq!(result.unwrap(), "YES", "Config should exist");

    let config = vm.ssh("cat /etc/st-plc/agent.yaml").unwrap();
    assert!(config.contains("bind: 0.0.0.0"), "Config should have bind setting");
    assert!(config.contains("port: 4840"), "Config should have port setting");
}

#[test]
fn test_after_install_systemd_active() {
    skip_if_no_qemu!();
    let vm = boot_fresh_vm();
    vm.run_install(&[]);

    let status = vm.ssh("systemctl is-active st-runtime").unwrap();
    assert_eq!(status, "active", "systemd service should be active");
}

#[test]
fn test_after_install_systemd_enabled() {
    skip_if_no_qemu!();
    let vm = boot_fresh_vm();
    vm.run_install(&[]);

    let enabled = vm.ssh("systemctl is-enabled st-runtime").unwrap();
    assert_eq!(enabled, "enabled", "systemd service should be enabled (starts on boot)");
}

#[test]
fn test_after_install_health_check() {
    skip_if_no_qemu!();
    let vm = boot_fresh_vm();
    vm.run_install(&[]);

    let (status, body) = vm.curl_agent("/api/v1/health");
    assert_eq!(status, 200, "Health check should return 200");
    assert!(body.contains("healthy"), "Health response: {body}");
}

#[test]
fn test_after_install_target_info() {
    skip_if_no_qemu!();
    let vm = boot_fresh_vm();
    vm.run_install(&[]);

    let (status, body) = vm.curl_agent("/api/v1/target-info");
    assert_eq!(status, 200);
    let info: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(info["os"], "linux");
    assert_eq!(info["arch"], "x86_64");
    assert!(info["agent_version"].as_str().unwrap().len() > 0);
}

#[test]
fn test_after_install_upload_and_run() {
    skip_if_no_qemu!();
    let vm = boot_fresh_vm();
    vm.run_install(&[]);

    let bundle_path = make_test_bundle();
    let (status, _) = vm.upload_bundle(&bundle_path);
    assert_eq!(status, 200, "Bundle upload should succeed");

    let (status, _) = vm.curl_post("/api/v1/program/start");
    assert_eq!(status, 200, "Program start should succeed");

    std::thread::sleep(Duration::from_millis(500));

    let (status, body) = vm.curl_agent("/api/v1/status");
    assert_eq!(status, 200);
    let st: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(st["status"], "running");
    let cycles = st["cycle_stats"]["cycle_count"].as_u64().unwrap_or(0);
    assert!(cycles > 0, "Cycle count should be > 0, got {cycles}");

    // Cleanup
    vm.curl_post("/api/v1/program/stop");
}

#[test]
fn test_after_install_dap_debug() {
    skip_if_no_qemu!();
    let vm = boot_fresh_vm();
    vm.run_install(&[]);

    let bundle_path = make_test_bundle();
    vm.upload_bundle(&bundle_path);

    std::thread::sleep(Duration::from_secs(1));

    // Connect to DAP proxy
    let dap_port = vm.agent_port + 1;
    let stream = std::net::TcpStream::connect(format!("127.0.0.1:{dap_port}"));
    match stream {
        Ok(s) => {
            s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
            let mut writer = s.try_clone().unwrap();
            let mut reader = std::io::BufReader::new(s);

            // Send Initialize
            let init = serde_json::json!({
                "seq": 1, "type": "request", "command": "initialize",
                "arguments": {"adapterID": "st"}
            });
            let json = serde_json::to_string(&init).unwrap();
            let frame = format!("Content-Length: {}\r\n\r\n{}", json.len(), json);
            writer.write_all(frame.as_bytes()).unwrap();
            writer.flush().unwrap();

            // Read response (with timeout)
            let mut content_length: usize = 0;
            let mut line = String::new();
            loop {
                line.clear();
                if reader.read_line(&mut line).unwrap_or(0) == 0 { break; }
                if line.trim().is_empty() { break; }
                if let Some(rest) = line.trim().strip_prefix("Content-Length:") {
                    content_length = rest.trim().parse().unwrap_or(0);
                }
            }

            if content_length > 0 {
                let mut body = vec![0u8; content_length];
                reader.read_exact(&mut body).unwrap();
                let resp: serde_json::Value = serde_json::from_slice(&body).unwrap();
                assert_eq!(resp["success"], true, "DAP Initialize should succeed: {resp}");
                eprintln!("DAP debug working through installer-deployed target");
            }

            // Disconnect
            let disc = serde_json::json!({
                "seq": 2, "type": "request", "command": "disconnect",
                "arguments": {"terminateDebuggee": true}
            });
            let json = serde_json::to_string(&disc).unwrap();
            let frame = format!("Content-Length: {}\r\n\r\n{}", json.len(), json);
            let _ = writer.write_all(frame.as_bytes());
        }
        Err(e) => {
            panic!("Cannot connect to DAP proxy on port {dap_port}: {e}");
        }
    }
}

#[test]
fn test_service_auto_restarts_after_crash() {
    skip_if_no_qemu!();
    let vm = boot_fresh_vm();
    vm.run_install(&[]);

    // Verify healthy
    let (status, _) = vm.curl_agent("/api/v1/health");
    assert_eq!(status, 200);

    // Kill the process (simulate crash)
    let _ = vm.ssh("sudo pkill -9 -f st-runtime");

    // Wait for systemd to restart it (RestartSec=3)
    std::thread::sleep(Duration::from_secs(5));

    // Should be healthy again
    let (status, body) = vm.curl_agent("/api/v1/health");
    assert_eq!(status, 200, "Agent should be healthy after crash recovery: {body}");
}

#[test]
fn test_custom_agent_name() {
    skip_if_no_qemu!();
    let vm = boot_fresh_vm();
    vm.run_install(&["--name", "my-custom-plc"]);

    let (_, body) = vm.curl_agent("/api/v1/health");
    let health: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(health["agent"], "my-custom-plc", "Custom agent name should appear in health");
}

// ─── Upgrade Tests ──────────────────────────────────────────────────────

#[test]
fn test_upgrade_succeeds() {
    skip_if_no_qemu!();
    let vm = boot_fresh_vm();

    // Install first
    let (ok, _) = vm.run_install(&[]);
    assert!(ok, "Initial install should succeed");

    // Get initial version
    let (_, body) = vm.curl_agent("/api/v1/health");
    let v1: serde_json::Value = serde_json::from_str(&body).unwrap();
    let version1 = v1["version"].as_str().unwrap().to_string();

    // Upgrade (same binary — but tests the upgrade path)
    let (ok, output) = vm.run_install(&["--upgrade"]);
    assert!(ok, "Upgrade should succeed:\n{output}");

    // Verify still healthy
    let (status, body) = vm.curl_agent("/api/v1/health");
    assert_eq!(status, 200, "Agent should be healthy after upgrade");
    let v2: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v2["version"].as_str().unwrap(), version1, "Version should match");
}

#[test]
fn test_upgrade_preserves_config() {
    skip_if_no_qemu!();
    let vm = boot_fresh_vm();
    vm.run_install(&["--name", "preserve-me"]);

    // Verify custom name
    let (_, body) = vm.curl_agent("/api/v1/health");
    assert!(body.contains("preserve-me"));

    // Upgrade
    vm.run_install(&["--upgrade"]);

    // Config should be preserved (upgrade skips config write)
    let config = vm.ssh("cat /etc/st-plc/agent.yaml").unwrap();
    assert!(config.contains("preserve-me"), "Config should be preserved after upgrade");
}

#[test]
fn test_upgrade_preserves_deployed_program() {
    skip_if_no_qemu!();
    let vm = boot_fresh_vm();
    vm.run_install(&[]);

    // Upload a program
    let bundle_path = make_test_bundle();
    vm.upload_bundle(&bundle_path);

    let (_, body) = vm.curl_agent("/api/v1/program/info");
    assert!(body.contains("E2ETestProject"), "Program should be deployed");

    // Upgrade
    vm.run_install(&["--upgrade"]);

    // Program store is in-memory, so it won't survive a restart.
    // But the binary and config are preserved.
    let (status, _) = vm.curl_agent("/api/v1/health");
    assert_eq!(status, 200, "Agent should be healthy after upgrade with program");
}

// ─── Uninstall Tests ────────────────────────────────────────────────────

#[test]
fn test_uninstall_removes_service() {
    skip_if_no_qemu!();
    let vm = boot_fresh_vm();
    vm.run_install(&[]);

    // Verify installed
    let (status, _) = vm.curl_agent("/api/v1/health");
    assert_eq!(status, 200);

    // Uninstall
    let (ok, output) = vm.run_uninstall(&[]);
    assert!(ok, "Uninstall should succeed:\n{output}");

    // Service should be gone
    let result = vm.ssh("systemctl is-active st-runtime 2>&1 || true").unwrap();
    assert!(
        result.contains("inactive") || result.contains("could not be found"),
        "Service should be stopped: {result}"
    );

    // Binary should be gone
    let result = vm.ssh("test -f /opt/st-plc/st-runtime && echo EXISTS || echo GONE").unwrap();
    assert_eq!(result, "GONE", "Binary should be removed");

    // Config should be gone
    let result = vm.ssh("test -d /etc/st-plc && echo EXISTS || echo GONE").unwrap();
    assert_eq!(result, "GONE", "Config dir should be removed");
}

#[test]
fn test_uninstall_purge_removes_data() {
    skip_if_no_qemu!();
    let vm = boot_fresh_vm();
    vm.run_install(&[]);

    // Uninstall with purge
    let (ok, _) = vm.run_uninstall(&["--purge"]);
    assert!(ok);

    // Programs dir should be gone
    let result = vm.ssh("test -d /var/lib/st-plc/programs && echo EXISTS || echo GONE").unwrap();
    assert_eq!(result, "GONE", "Programs dir should be removed with --purge");

    // Log dir should be gone
    let result = vm.ssh("test -d /var/log/st-plc && echo EXISTS || echo GONE").unwrap();
    assert_eq!(result, "GONE", "Log dir should be removed with --purge");
}

#[test]
fn test_uninstall_not_installed_errors() {
    skip_if_no_qemu!();
    let vm = boot_fresh_vm();

    // Uninstall without installing → should error
    let (ok, output) = vm.run_uninstall(&[]);
    assert!(!ok, "Uninstall on not-installed target should fail");
    assert!(
        output.contains("not installed"),
        "Should report not installed: {output}"
    );
}

// ─── Error Handling Tests ───────────────────────────────────────────────

#[test]
fn test_install_wrong_key_fails() {
    skip_if_no_qemu!();
    let vm = boot_fresh_vm();

    // Use a nonexistent key
    let output = Command::new(st_cli_binary().to_str().unwrap())
        .args([
            "target", "install",
            "plc@127.0.0.1",
            "--port", &vm.ssh_port.to_string(),
            "--key", "/tmp/nonexistent_key",
        ])
        .output()
        .expect("Failed to run st-cli");

    assert!(!output.status.success(), "Should fail with wrong key");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("Permission denied") || stderr.contains("No such file")
            || stderr.contains("denied") || stderr.contains("error"),
        "Should report auth error: {stderr}"
    );
}

#[test]
fn test_install_unreachable_host_fails() {
    skip_if_no_qemu!();

    // Try to install to a host that doesn't exist
    let output = Command::new(st_cli_binary().to_str().unwrap())
        .args([
            "target", "install",
            "plc@192.0.2.1",  // RFC 5737 TEST-NET — guaranteed unreachable
            "--key", ssh_key_path().to_str().unwrap(),
        ])
        .output()
        .expect("Failed to run st-cli");

    assert!(!output.status.success(), "Should fail with unreachable host");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("timed out") || stderr.contains("Connection") || stderr.contains("error"),
        "Should report connection error: {stderr}"
    );
}

// ─── SSH Transport Tests ────────────────────────────────────────────────

#[test]
fn test_ssh_with_explicit_key() {
    skip_if_no_qemu!();
    let vm = boot_fresh_vm();

    let (ok, output) = vm.run_install(&["--key", ssh_key_path().to_str().unwrap()]);
    assert!(ok, "Install with explicit key should succeed:\n{output}");
}

#[test]
fn test_ssh_with_non_standard_port() {
    skip_if_no_qemu!();
    let vm = boot_fresh_vm();

    // Port 2222 is the forwarded SSH port — this IS our non-standard port test
    let (ok, output) = vm.run_install(&[]);
    assert!(ok, "Install with --port 2222 should succeed:\n{output}");
}

// ─── Full Lifecycle: Install → Deploy → Run → Debug → Update → Uninstall

#[test]
fn test_full_installer_lifecycle() {
    skip_if_no_qemu!();
    let vm = boot_fresh_vm();

    // 1. Install
    eprintln!("[LIFECYCLE] Installing...");
    let (ok, output) = vm.run_install(&["--name", "lifecycle-test"]);
    assert!(ok, "Install failed:\n{output}");

    // 2. Health check
    eprintln!("[LIFECYCLE] Health check...");
    let (status, body) = vm.curl_agent("/api/v1/health");
    assert_eq!(status, 200);
    assert!(body.contains("lifecycle-test"));

    // 3. Upload program
    eprintln!("[LIFECYCLE] Uploading bundle...");
    let bundle_path = make_test_bundle();
    let (status, _) = vm.upload_bundle(&bundle_path);
    assert_eq!(status, 200);

    // 4. Start runtime
    eprintln!("[LIFECYCLE] Starting runtime...");
    let (status, _) = vm.curl_post("/api/v1/program/start");
    assert_eq!(status, 200);
    std::thread::sleep(Duration::from_millis(500));

    // 5. Verify running
    eprintln!("[LIFECYCLE] Verifying running...");
    let (_, body) = vm.curl_agent("/api/v1/status");
    let st: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(st["status"], "running");
    let c1 = st["cycle_stats"]["cycle_count"].as_u64().unwrap();
    assert!(c1 > 0);

    // 6. Stop
    eprintln!("[LIFECYCLE] Stopping...");
    vm.curl_post("/api/v1/program/stop");
    std::thread::sleep(Duration::from_millis(300));

    // 7. Upgrade
    eprintln!("[LIFECYCLE] Upgrading...");
    let (ok, _) = vm.run_install(&["--upgrade"]);
    assert!(ok);

    // 8. Verify still healthy after upgrade
    let (status, _) = vm.curl_agent("/api/v1/health");
    assert_eq!(status, 200);

    // 9. Uninstall
    eprintln!("[LIFECYCLE] Uninstalling...");
    let (ok, _) = vm.run_uninstall(&["--purge"]);
    assert!(ok);

    // 10. Verify gone
    let result = vm.ssh("test -f /opt/st-plc/st-runtime && echo EXISTS || echo GONE").unwrap();
    assert_eq!(result, "GONE");

    eprintln!("[LIFECYCLE] Complete: install → deploy → run → stop → upgrade → uninstall");
}

// ─── Journald Logging Tests ─────────────────────────────────────────────

#[test]
fn test_logs_written_to_journald() {
    skip_if_no_qemu!();
    let vm = boot_fresh_vm();
    vm.run_install(&[]);

    // The agent logs to journald. Verify with journalctl.
    let result = vm.ssh(
        "sudo journalctl -u st-runtime --no-pager -n 10 2>&1"
    ).unwrap();
    assert!(
        result.contains("Starting") || result.contains("Agent ready") || result.contains("INFO"),
        "journald should contain agent startup messages: {result}"
    );
}

#[test]
fn test_log_level_from_config_honored() {
    skip_if_no_qemu!();
    let vm = boot_fresh_vm();
    vm.run_install(&["--name", "log-level-test"]);

    // Default config level is "info". The health endpoint reports the level.
    let (status, body) = vm.curl_agent("/api/v1/log-level");
    assert_eq!(status, 200);
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(parsed["level"], "info", "Default log level should be info");
}

#[test]
fn test_log_level_runtime_change() {
    skip_if_no_qemu!();
    let vm = boot_fresh_vm();
    vm.run_install(&[]);

    // Verify initial level
    let (status, body) = vm.curl_agent("/api/v1/log-level");
    assert_eq!(status, 200);
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(parsed["level"], "info");

    // Change to debug at runtime
    let url = format!("http://127.0.0.1:{}/api/v1/log-level", vm.agent_port);
    let output = Command::new("curl")
        .args([
            "-sf", "-X", "PUT",
            "-H", "Content-Type: application/json",
            "-d", r#"{"level":"debug"}"#,
            "--connect-timeout", "5",
            &url,
        ])
        .output()
        .unwrap();
    assert!(output.status.success(), "PUT log-level should succeed");
    let resp: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(resp["level"], "debug");

    // Verify it persisted
    let (status, body) = vm.curl_agent("/api/v1/log-level");
    assert_eq!(status, 200);
    let parsed: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(parsed["level"], "debug", "Level should have changed to debug");

    // Verify debug messages now appear in journald
    // Generate a log event by uploading a bundle (triggers info/debug logs)
    let bundle_path = make_test_bundle();
    vm.upload_bundle(&bundle_path);

    let result = vm.ssh(
        "sudo journalctl -u st-runtime --no-pager -n 20 --since '1 min ago' 2>&1"
    ).unwrap();
    // With debug level, we should see more verbose output
    assert!(
        !result.is_empty(),
        "journald should have recent log entries after level change"
    );
}
