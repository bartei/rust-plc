//! SSH transport — runs commands and uploads files to remote targets.
//!
//! Uses the system `ssh` and `scp` binaries via subprocess, which means the
//! user's SSH config, agent, and keys work automatically. No Rust SSH library
//! needed.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Error type for SSH operations.
#[derive(Debug)]
pub struct SshError {
    pub message: String,
    pub kind: SshErrorKind,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SshErrorKind {
    ConnectionFailed,
    AuthenticationFailed,
    CommandFailed,
    TransferFailed,
    SudoRequired,
    Timeout,
}

impl std::fmt::Display for SshError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for SshError {}

/// A remote SSH target.
#[derive(Debug, Clone)]
pub struct SshTarget {
    pub user: String,
    pub host: String,
    pub port: u16,
    pub key: Option<PathBuf>,
}

impl SshTarget {
    /// Create a new SSH target from a `user@host` string.
    pub fn parse(user_at_host: &str) -> Result<Self, String> {
        let (user, host) = user_at_host
            .split_once('@')
            .ok_or_else(|| format!("Invalid target format: '{user_at_host}'. Expected user@host"))?;
        Ok(SshTarget {
            user: user.to_string(),
            host: host.to_string(),
            port: 22,
            key: None,
        })
    }

    /// Set a custom SSH port.
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Set an explicit SSH key path.
    pub fn with_key(mut self, key: PathBuf) -> Self {
        self.key = Some(key);
        self
    }

    /// Test if the SSH connection works.
    pub fn test_connection(&self) -> Result<(), SshError> {
        let output = self.run_ssh(&["true"])?;
        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("Permission denied") || stderr.contains("permission denied") {
                Err(SshError {
                    message: format!(
                        "Permission denied connecting to {}@{}. Check your SSH key.",
                        self.user, self.host
                    ),
                    kind: SshErrorKind::AuthenticationFailed,
                })
            } else {
                Err(SshError {
                    message: format!("SSH connection failed: {}", stderr.trim()),
                    kind: SshErrorKind::ConnectionFailed,
                })
            }
        }
    }

    /// Execute a command on the remote target. Returns stdout as String.
    pub fn exec(&self, cmd: &str) -> Result<String, SshError> {
        let output = self.run_ssh(&[cmd])?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(SshError {
                message: format!("Command failed: {}", stderr.trim()),
                kind: SshErrorKind::CommandFailed,
            })
        }
    }

    /// Execute a command with sudo on the remote target.
    pub fn sudo_exec(&self, cmd: &str) -> Result<String, SshError> {
        self.exec(&format!("sudo {cmd}"))
    }

    /// Upload a local file to the remote target via SCP.
    pub fn upload(&self, local: &Path, remote: &str) -> Result<(), SshError> {
        let mut cmd = Command::new("scp");
        self.add_ssh_options(&mut cmd);
        cmd.arg("-P").arg(self.port.to_string());
        cmd.arg(local.to_str().unwrap());
        cmd.arg(format!("{}@{}:{}", self.user, self.host, remote));

        let output = cmd
            .output()
            .map_err(|e| SshError {
                message: format!("Failed to run scp: {e}"),
                kind: SshErrorKind::TransferFailed,
            })?;

        if output.status.success() {
            Ok(())
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(SshError {
                message: format!("SCP upload failed: {}", stderr.trim()),
                kind: SshErrorKind::TransferFailed,
            })
        }
    }

    /// Detect the remote OS and CPU architecture.
    pub fn detect_platform(&self) -> Result<(String, String), SshError> {
        let output = self.exec("uname -s -m")?;
        let parts: Vec<&str> = output.split_whitespace().collect();
        if parts.len() >= 2 {
            Ok((parts[0].to_lowercase(), parts[1].to_string()))
        } else {
            Err(SshError {
                message: format!("Unexpected uname output: '{output}'"),
                kind: SshErrorKind::CommandFailed,
            })
        }
    }

    /// Check if the remote user has passwordless sudo access.
    pub fn check_sudo(&self) -> Result<(), SshError> {
        let output = self.run_ssh(&["sudo -n true"])?;
        if output.status.success() {
            Ok(())
        } else {
            Err(SshError {
                message: format!(
                    "User '{}' does not have passwordless sudo on {}. \
                     Add to sudoers: {} ALL=(ALL) NOPASSWD:ALL",
                    self.user, self.host, self.user
                ),
                kind: SshErrorKind::SudoRequired,
            })
        }
    }

    // ── Internal helpers ──────────────────────────────────────────────

    fn ssh_args(&self) -> Vec<String> {
        let mut args = vec![
            "-o".to_string(), "StrictHostKeyChecking=accept-new".to_string(),
            "-o".to_string(), "UserKnownHostsFile=/dev/null".to_string(),
            "-o".to_string(), "LogLevel=ERROR".to_string(),
            "-o".to_string(), "ConnectTimeout=30".to_string(),
            "-o".to_string(), "BatchMode=yes".to_string(),
            "-p".to_string(), self.port.to_string(),
        ];
        if let Some(ref key) = self.key {
            args.push("-i".to_string());
            args.push(key.to_string_lossy().to_string());
        }
        args
    }

    fn add_ssh_options(&self, cmd: &mut Command) {
        cmd.args(["-o", "StrictHostKeyChecking=accept-new"]);
        cmd.args(["-o", "UserKnownHostsFile=/dev/null"]);
        cmd.args(["-o", "LogLevel=ERROR"]);
        cmd.args(["-o", "ConnectTimeout=30"]);
        cmd.args(["-o", "BatchMode=yes"]);
        if let Some(ref key) = self.key {
            cmd.arg("-i").arg(key);
        }
    }

    fn run_ssh(&self, remote_cmd: &[&str]) -> Result<Output, SshError> {
        let mut cmd = Command::new("ssh");
        let args = self.ssh_args();
        cmd.args(&args);
        cmd.arg(format!("{}@{}", self.user, self.host));
        cmd.args(remote_cmd);

        cmd.output().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                SshError {
                    message: "ssh binary not found. Install OpenSSH.".to_string(),
                    kind: SshErrorKind::ConnectionFailed,
                }
            } else {
                SshError {
                    message: format!("Failed to run ssh: {e}"),
                    kind: SshErrorKind::ConnectionFailed,
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_user_at_host() {
        let target = SshTarget::parse("plc@192.168.1.50").unwrap();
        assert_eq!(target.user, "plc");
        assert_eq!(target.host, "192.168.1.50");
        assert_eq!(target.port, 22);
        assert!(target.key.is_none());
    }

    #[test]
    fn parse_with_port_and_key() {
        let target = SshTarget::parse("admin@10.0.0.1")
            .unwrap()
            .with_port(2222)
            .with_key(PathBuf::from("/home/user/.ssh/plc_key"));
        assert_eq!(target.port, 2222);
        assert_eq!(target.key, Some(PathBuf::from("/home/user/.ssh/plc_key")));
    }

    #[test]
    fn parse_invalid_format() {
        assert!(SshTarget::parse("nousername").is_err());
    }

    #[test]
    fn ssh_args_include_key_when_set() {
        let target = SshTarget::parse("plc@host")
            .unwrap()
            .with_key(PathBuf::from("/tmp/key"));
        let args = target.ssh_args();
        assert!(args.contains(&"-i".to_string()));
        assert!(args.contains(&"/tmp/key".to_string()));
    }

    #[test]
    fn ssh_args_no_key_by_default() {
        let target = SshTarget::parse("plc@host").unwrap();
        let args = target.ssh_args();
        assert!(!args.contains(&"-i".to_string()));
    }
}
