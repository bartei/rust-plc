//! Agent configuration parsed from `agent.yaml`.

use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AgentConfig {
    #[serde(default)]
    pub agent: AgentInfo,
    #[serde(default)]
    pub network: NetworkConfig,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub runtime: RuntimeConfig,
    #[serde(default)]
    pub storage: StorageConfig,
    #[serde(default)]
    pub security: SecurityConfig,
}


#[derive(Debug, Clone, Deserialize)]
pub struct AgentInfo {
    #[serde(default = "default_agent_name")]
    pub name: String,
    #[serde(default)]
    pub description: String,
}

impl Default for AgentInfo {
    fn default() -> Self {
        AgentInfo {
            name: default_agent_name(),
            description: String::new(),
        }
    }
}

fn default_agent_name() -> String {
    "st-agent".to_string()
}

#[derive(Debug, Clone, Deserialize)]
pub struct NetworkConfig {
    #[serde(default = "default_bind")]
    pub bind: String,
    #[serde(default = "default_port")]
    pub port: u16,
    /// TCP port for DAP (Debug Adapter Protocol) proxy connections.
    /// VS Code connects here for remote debugging. Default: port + 1 (4841).
    #[serde(default)]
    pub dap_port: Option<u16>,
}

impl NetworkConfig {
    /// Resolved DAP port (default: HTTP port + 1).
    pub fn dap_port(&self) -> u16 {
        self.dap_port.unwrap_or(self.port + 1)
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        NetworkConfig {
            bind: default_bind(),
            port: default_port(),
            dap_port: None,
        }
    }
}

fn default_bind() -> String {
    "127.0.0.1".to_string()
}

fn default_port() -> u16 {
    4840
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthMode {
    #[default]
    None,
    Token,
}

#[derive(Debug, Clone, Deserialize)]
pub struct AuthConfig {
    #[serde(default)]
    pub mode: AuthMode,
    #[serde(default)]
    pub token: Option<String>,
    #[serde(default)]
    pub read_only: bool,
}

impl Default for AuthConfig {
    fn default() -> Self {
        AuthConfig {
            mode: AuthMode::None,
            token: None,
            read_only: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RuntimeConfig {
    #[serde(default = "default_runtime_mode")]
    pub mode: String,
    #[serde(default)]
    pub auto_start: bool,
    #[serde(default)]
    pub watchdog_ms: Option<u64>,
    #[serde(default = "default_true")]
    pub restart_on_crash: bool,
    #[serde(default = "default_restart_delay")]
    pub restart_delay_ms: u64,
    #[serde(default = "default_max_restarts")]
    pub max_restarts: u32,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        RuntimeConfig {
            mode: default_runtime_mode(),
            auto_start: false,
            watchdog_ms: None,
            restart_on_crash: true,
            restart_delay_ms: default_restart_delay(),
            max_restarts: default_max_restarts(),
        }
    }
}

fn default_runtime_mode() -> String {
    "vm".to_string()
}

fn default_true() -> bool {
    true
}

fn default_restart_delay() -> u64 {
    1000
}

fn default_max_restarts() -> u32 {
    5
}

#[derive(Debug, Clone, Deserialize)]
pub struct StorageConfig {
    #[serde(default = "default_program_dir")]
    pub program_dir: PathBuf,
    #[serde(default = "default_log_dir")]
    pub log_dir: PathBuf,
}

impl Default for StorageConfig {
    fn default() -> Self {
        StorageConfig {
            program_dir: default_program_dir(),
            log_dir: default_log_dir(),
        }
    }
}

fn default_program_dir() -> PathBuf {
    PathBuf::from("/var/lib/st-agent/programs")
}

fn default_log_dir() -> PathBuf {
    PathBuf::from("/var/log/st-agent")
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SecurityConfig {
    #[serde(default)]
    pub require_signed: bool,
    #[serde(default)]
    pub trusted_keys: Vec<PathBuf>,
}

/// Load agent configuration from a YAML file.
pub fn load_config(path: &Path) -> Result<AgentConfig, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Cannot read config {}: {e}", path.display()))?;
    parse_config(&content)
}

/// Parse agent configuration from a YAML string.
pub fn parse_config(yaml: &str) -> Result<AgentConfig, String> {
    serde_yaml::from_str(yaml).map_err(|e| format!("Invalid agent config: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_full_config() {
        let yaml = r#"
agent:
  name: line1-plc
  description: "Bottle filling line"
network:
  bind: 0.0.0.0
  port: 4840
auth:
  mode: token
  token: "secret123"
  read_only: false
runtime:
  mode: vm
  auto_start: true
  watchdog_ms: 100
  restart_on_crash: true
  restart_delay_ms: 2000
  max_restarts: 3
storage:
  program_dir: /opt/programs
  log_dir: /opt/logs
security:
  require_signed: true
  trusted_keys:
    - /etc/keys/deployer.pub
"#;
        let config = parse_config(yaml).unwrap();
        assert_eq!(config.agent.name, "line1-plc");
        assert_eq!(config.network.bind, "0.0.0.0");
        assert_eq!(config.network.port, 4840);
        assert_eq!(config.auth.mode, AuthMode::Token);
        assert_eq!(config.auth.token, Some("secret123".to_string()));
        assert!(config.runtime.auto_start);
        assert_eq!(config.runtime.watchdog_ms, Some(100));
        assert_eq!(config.runtime.max_restarts, 3);
        assert_eq!(config.storage.program_dir, PathBuf::from("/opt/programs"));
        assert!(config.security.require_signed);
        assert_eq!(config.security.trusted_keys.len(), 1);
    }

    #[test]
    fn parse_minimal_config() {
        let yaml = "agent:\n  name: test\n";
        let config = parse_config(yaml).unwrap();
        assert_eq!(config.agent.name, "test");
        assert_eq!(config.network.port, 4840);
        assert_eq!(config.network.bind, "127.0.0.1");
        assert_eq!(config.auth.mode, AuthMode::None);
        assert!(!config.runtime.auto_start);
        assert!(config.runtime.restart_on_crash);
        assert_eq!(config.runtime.max_restarts, 5);
    }

    #[test]
    fn parse_empty_config_uses_defaults() {
        let yaml = "{}";
        let config = parse_config(yaml).unwrap();
        assert_eq!(config.agent.name, "st-agent");
        assert_eq!(config.network.port, 4840);
    }

    #[test]
    fn invalid_yaml_rejected() {
        let result = parse_config("invalid: yaml: [broken");
        assert!(result.is_err());
    }

    #[test]
    fn default_config() {
        let config = AgentConfig::default();
        assert_eq!(config.agent.name, "st-agent");
        assert_eq!(config.network.bind, "127.0.0.1");
        assert_eq!(config.network.port, 4840);
        assert_eq!(config.runtime.restart_delay_ms, 1000);
    }
}
