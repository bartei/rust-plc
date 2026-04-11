//! One-command target installer.
//!
//! `st-cli target install user@host` automates the entire target setup:
//! SSH → detect platform → upload static binary → create directories →
//! write config → install systemd service → start → verify health.

use crate::ssh::{SshError, SshErrorKind, SshTarget};
use std::path::{Path, PathBuf};

/// Options for the install command.
#[derive(Debug, Clone)]
pub struct InstallOptions {
    /// Agent HTTP port on the target.
    pub agent_port: u16,
    /// Agent name (shown in health endpoint).
    pub agent_name: String,
    /// Whether this is an upgrade of an existing installation.
    pub upgrade: bool,
}

impl Default for InstallOptions {
    fn default() -> Self {
        InstallOptions {
            agent_port: 4840,
            agent_name: "st-plc-runtime".to_string(),
            upgrade: false,
        }
    }
}

/// Result of a successful installation.
#[derive(Debug)]
pub struct InstallResult {
    pub os: String,
    pub arch: String,
    pub agent_port: u16,
    pub dap_port: u16,
    pub version: String,
}

// Target paths on the remote device
const BINARY_DIR: &str = "/opt/st-plc";
const BINARY_PATH: &str = "/opt/st-plc/st-plc-runtime";
const BACKUP_PATH: &str = "/opt/st-plc/st-plc-runtime.backup";
const CONFIG_DIR: &str = "/etc/st-plc";
const CONFIG_PATH: &str = "/etc/st-plc/agent.yaml";
const DATA_DIR: &str = "/var/lib/st-plc/programs";
const LOG_DIR: &str = "/var/log/st-plc";
const SERVICE_NAME: &str = "st-plc-runtime";
const SERVICE_PATH: &str = "/etc/systemd/system/st-plc-runtime.service";

/// Install the PLC runtime on a remote target.
///
/// This is the main entry point for `st-cli target install user@host`.
pub fn install(
    target: &SshTarget,
    binary_path: &Path,
    options: &InstallOptions,
    progress: &mut dyn FnMut(&str),
) -> Result<InstallResult, SshError> {
    // 1. Test SSH connection
    progress("Connecting...");
    target.test_connection()?;

    // 2. Check sudo access
    progress("Checking sudo access...");
    target.check_sudo()?;

    // 3. Detect platform
    progress("Detecting target platform...");
    let (os, arch) = target.detect_platform()?;
    progress(&format!("  Target: {os} {arch}"));

    if os != "linux" {
        return Err(SshError {
            message: format!("Unsupported OS: '{os}'. Only Linux targets are supported."),
            kind: SshErrorKind::CommandFailed,
        });
    }

    // 4. Handle upgrade vs fresh install
    if options.upgrade {
        progress("Upgrading existing installation...");
        // Backup current binary
        let _ = target.sudo_exec(&format!("cp {BINARY_PATH} {BACKUP_PATH}"));
        // Stop service
        let _ = target.sudo_exec(&format!("systemctl stop {SERVICE_NAME}"));
    }

    // 5. Create directory structure
    progress("Creating directories...");
    target.sudo_exec(&format!(
        "mkdir -p {BINARY_DIR} {CONFIG_DIR} {DATA_DIR} {LOG_DIR}"
    ))?;
    // Ensure the runtime user can write to data/log dirs
    target.sudo_exec(&format!(
        "chown -R {user}:{user} {DATA_DIR} {LOG_DIR}",
        user = target.user
    ))?;

    // 6. Upload binary
    progress(&format!(
        "Uploading st-plc-runtime ({})...",
        format_file_size(binary_path)
    ));
    let tmp_path = "/tmp/st-plc-runtime.upload";
    target.upload(binary_path, tmp_path)?;
    target.sudo_exec(&format!("mv {tmp_path} {BINARY_PATH}"))?;
    target.sudo_exec(&format!("chmod +x {BINARY_PATH}"))?;

    // 7. Verify binary runs on target
    progress("Verifying binary...");
    let version_output = target.exec(&format!("{BINARY_PATH} version"))?;
    let version = version_output
        .lines()
        .next()
        .unwrap_or("unknown")
        .replace("st-plc-runtime ", "");
    progress(&format!("  Version: {version}"));

    // 8. Write config (only on fresh install, preserve on upgrade)
    if !options.upgrade {
        progress("Writing configuration...");
        let config_yaml = generate_agent_config(&options.agent_name, options.agent_port);
        target.exec(&format!(
            "echo '{config_yaml}' | sudo tee {CONFIG_PATH} > /dev/null"
        ))?;
    }

    // 9. Install systemd service
    progress("Installing systemd service...");
    let service_unit = generate_systemd_unit(options.agent_port);
    target.exec(&format!(
        "echo '{service_unit}' | sudo tee {SERVICE_PATH} > /dev/null"
    ))?;
    target.sudo_exec("systemctl daemon-reload")?;
    target.sudo_exec(&format!("systemctl enable {SERVICE_NAME}"))?;

    // 10. Start service
    progress("Starting agent...");
    target.sudo_exec(&format!("systemctl start {SERVICE_NAME}"))?;

    // 11. Wait for health check
    progress("Waiting for agent to become healthy...");
    let health_ok = wait_for_health(target, options.agent_port)?;
    if !health_ok {
        // Upgrade rollback
        if options.upgrade {
            progress("Health check failed — rolling back...");
            let _ = target.sudo_exec(&format!("systemctl stop {SERVICE_NAME}"));
            let _ = target.sudo_exec(&format!("mv {BACKUP_PATH} {BINARY_PATH}"));
            let _ = target.sudo_exec(&format!("systemctl start {SERVICE_NAME}"));
            return Err(SshError {
                message: "Upgrade failed: agent not healthy after update. Rolled back to previous version.".to_string(),
                kind: SshErrorKind::CommandFailed,
            });
        }
        return Err(SshError {
            message: "Agent failed to start. Check logs: journalctl -u st-plc-runtime".to_string(),
            kind: SshErrorKind::CommandFailed,
        });
    }

    let dap_port = options.agent_port + 1;

    // Clean up backup on successful upgrade
    if options.upgrade {
        let _ = target.sudo_exec(&format!("rm -f {BACKUP_PATH}"));
    }

    Ok(InstallResult {
        os,
        arch,
        agent_port: options.agent_port,
        dap_port,
        version,
    })
}

/// Uninstall the PLC runtime from a remote target.
pub fn uninstall(
    target: &SshTarget,
    purge: bool,
    progress: &mut dyn FnMut(&str),
) -> Result<(), SshError> {
    progress("Connecting...");
    target.test_connection()?;
    target.check_sudo()?;

    // Check if installed
    let installed = target.exec(&format!("test -f {BINARY_PATH} && echo yes || echo no"))?;
    if installed.trim() != "yes" {
        return Err(SshError {
            message: format!("PLC runtime is not installed on {}@{}", target.user, target.host),
            kind: SshErrorKind::CommandFailed,
        });
    }

    progress("Stopping service...");
    let _ = target.sudo_exec(&format!("systemctl stop {SERVICE_NAME}"));
    let _ = target.sudo_exec(&format!("systemctl disable {SERVICE_NAME}"));

    progress("Removing files...");
    target.sudo_exec(&format!("rm -f {SERVICE_PATH}"))?;
    target.sudo_exec("systemctl daemon-reload")?;
    target.sudo_exec(&format!("rm -rf {BINARY_DIR}"))?;
    target.sudo_exec(&format!("rm -rf {CONFIG_DIR}"))?;

    if purge {
        progress("Purging data and logs...");
        target.sudo_exec(&format!("rm -rf {DATA_DIR}"))?;
        target.sudo_exec(&format!("rm -rf {LOG_DIR}"))?;
    }

    Ok(())
}

/// Find the static binary for the given architecture.
pub fn find_static_binary(arch: &str) -> Result<PathBuf, String> {
    let target_triple = match arch {
        "x86_64" => "x86_64-unknown-linux-musl",
        "aarch64" => "aarch64-unknown-linux-musl",
        _ => return Err(format!("Unsupported architecture: {arch}")),
    };

    let suffixes = [
        format!("target/{target_triple}/release-static/st-plc-runtime"),
        format!("target/{target_triple}/release/st-plc-runtime"),
    ];

    // Search from: CWD, then walk up to find workspace root (has Cargo.toml with [workspace])
    let mut search_roots: Vec<PathBuf> = vec![std::env::current_dir().unwrap_or_default()];

    // Also try relative to the st-cli binary itself
    if let Ok(exe) = std::env::current_exe() {
        if let Some(bin_dir) = exe.parent() {
            // Binary is in target/debug/ — workspace root is ../../
            if let Some(target_dir) = bin_dir.parent() {
                if let Some(root) = target_dir.parent() {
                    search_roots.push(root.to_path_buf());
                }
            }
        }
    }

    // Walk up from CWD looking for workspace Cargo.toml
    let mut cur = std::env::current_dir().unwrap_or_default();
    for _ in 0..10 {
        if cur.join("Cargo.toml").exists() && cur.join("crates").is_dir() {
            search_roots.push(cur.clone());
        }
        if !cur.pop() {
            break;
        }
    }

    for root in &search_roots {
        for suffix in &suffixes {
            let path = root.join(suffix);
            if path.exists() {
                return Ok(path);
            }
        }
    }

    Err(format!(
        "Static binary not found for {arch}. Build with:\n  ./scripts/build-static.sh {arch}"
    ))
}

// ── Internal helpers ────────────────────────────────────────────────────

fn generate_agent_config(name: &str, port: u16) -> String {
    format!(
        r#"agent:
  name: {name}
network:
  bind: 0.0.0.0
  port: {port}
runtime:
  auto_start: true
  restart_on_crash: true
  restart_delay_ms: 1000
  max_restarts: 10
storage:
  program_dir: {DATA_DIR}
  log_dir: {LOG_DIR}"#
    )
}

fn generate_systemd_unit(_agent_port: u16) -> String {
    format!(
        r#"[Unit]
Description=ST PLC Runtime Agent
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart={BINARY_PATH} agent --config {CONFIG_PATH}
Restart=on-failure
RestartSec=3
StandardOutput=journal
StandardError=journal
Environment=RUST_LOG=info

[Install]
WantedBy=multi-user.target"#
    )
}

fn wait_for_health(target: &SshTarget, port: u16) -> Result<bool, SshError> {
    for attempt in 1..=15 {
        let result = target.exec(&format!(
            "curl -sf http://127.0.0.1:{port}/api/v1/health 2>/dev/null || echo FAIL"
        ))?;
        if result.contains("healthy") {
            return Ok(true);
        }
        if attempt < 15 {
            std::thread::sleep(std::time::Duration::from_secs(1));
        }
    }
    Ok(false)
}

fn format_file_size(path: &Path) -> String {
    match std::fs::metadata(path) {
        Ok(m) => {
            let bytes = m.len();
            if bytes < 1024 * 1024 {
                format!("{:.1} KB", bytes as f64 / 1024.0)
            } else {
                format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
            }
        }
        Err(_) => "? bytes".to_string(),
    }
}
