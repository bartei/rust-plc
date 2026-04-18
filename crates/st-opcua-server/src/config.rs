//! OPC-UA server configuration types.

use serde::Deserialize;

/// OPC-UA server configuration.
///
/// All fields have sensible defaults so existing `agent.yaml` files that
/// don't include an `opcua_server:` section automatically get OPC-UA
/// enabled on port 4842.
#[derive(Debug, Clone, Deserialize)]
pub struct OpcuaServerConfig {
    /// Whether the OPC-UA server is enabled. Default: `true`.
    #[serde(default = "default_enabled")]
    pub enabled: bool,

    /// TCP port for the OPC-UA server. Default: `4842`.
    /// Chosen to avoid conflict with the HTTP API (4840) and DAP (4841).
    #[serde(default = "default_port")]
    pub port: u16,

    /// Bind address. Default: `"0.0.0.0"`.
    #[serde(default = "default_bind")]
    pub bind: String,

    /// OPC-UA security policy. Default: `"None"`.
    /// Supported: `"None"`, `"Basic256Sha256"`, `"Aes256Sha256RsaPss"`.
    #[serde(default = "default_security_policy")]
    pub security_policy: String,

    /// OPC-UA message security mode. Default: `"None"`.
    /// Supported: `"None"`, `"Sign"`, `"SignAndEncrypt"`.
    #[serde(default = "default_message_security_mode")]
    pub message_security_mode: String,

    /// Allow anonymous access. Default: `true`.
    #[serde(default = "default_true")]
    pub anonymous_access: bool,

    /// Application name shown to OPC-UA clients. Default: `"ST-PLC OPC-UA Server"`.
    #[serde(default = "default_application_name")]
    pub application_name: String,

    /// How often (in ms) to sync variable values from the engine to OPC-UA nodes.
    /// Lower values mean faster updates but slightly more CPU usage.
    /// Default: `100` (100ms).
    #[serde(default = "default_sampling_interval")]
    pub sampling_interval_ms: u64,

    /// Directory for OPC-UA PKI (certificates, private keys, trusted/rejected certs).
    /// A self-signed application certificate is auto-generated on first startup.
    /// Default: `None` (resolved by the host: `/var/lib/st-plc/pki` for the agent,
    /// `./pki` for local development).
    #[serde(default)]
    pub pki_dir: Option<std::path::PathBuf>,
}

impl Default for OpcuaServerConfig {
    fn default() -> Self {
        OpcuaServerConfig {
            enabled: true,
            port: default_port(),
            bind: default_bind(),
            security_policy: default_security_policy(),
            message_security_mode: default_message_security_mode(),
            anonymous_access: true,
            application_name: default_application_name(),
            sampling_interval_ms: default_sampling_interval(),
            pki_dir: None,
        }
    }
}

fn default_enabled() -> bool {
    true
}

fn default_port() -> u16 {
    4842
}

fn default_bind() -> String {
    "0.0.0.0".to_string()
}

fn default_security_policy() -> String {
    "None".to_string()
}

fn default_message_security_mode() -> String {
    "None".to_string()
}

fn default_true() -> bool {
    true
}

fn default_application_name() -> String {
    "ST-PLC OPC-UA Server".to_string()
}

fn default_sampling_interval() -> u64 {
    100
}

impl OpcuaServerConfig {
    /// The OPC-UA endpoint URL derived from bind address and port.
    pub fn endpoint_url(&self) -> String {
        format!("opc.tcp://{}:{}", self.bind, self.port)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config() {
        let config = OpcuaServerConfig::default();
        assert!(config.enabled);
        assert_eq!(config.port, 4842);
        assert_eq!(config.bind, "0.0.0.0");
        assert_eq!(config.security_policy, "None");
        assert!(config.anonymous_access);
        assert_eq!(config.sampling_interval_ms, 100);
    }

    #[test]
    fn endpoint_url() {
        let config = OpcuaServerConfig::default();
        assert_eq!(config.endpoint_url(), "opc.tcp://0.0.0.0:4842");
    }

    #[test]
    fn deserialize_minimal() {
        let yaml = "enabled: true\n";
        let config: OpcuaServerConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.enabled);
        assert_eq!(config.port, 4842);
        assert!(config.anonymous_access);
    }

    #[test]
    fn deserialize_full() {
        let yaml = r#"
enabled: true
port: 4850
bind: 127.0.0.1
security_policy: Basic256Sha256
message_security_mode: SignAndEncrypt
anonymous_access: false
application_name: "My PLC"
sampling_interval_ms: 50
"#;
        let config: OpcuaServerConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.enabled);
        assert_eq!(config.port, 4850);
        assert_eq!(config.bind, "127.0.0.1");
        assert_eq!(config.security_policy, "Basic256Sha256");
        assert!(!config.anonymous_access);
        assert_eq!(config.application_name, "My PLC");
        assert_eq!(config.sampling_interval_ms, 50);
    }

    #[test]
    fn deserialize_empty_uses_defaults() {
        let yaml = "{}";
        let config: OpcuaServerConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.enabled);
        assert_eq!(config.port, 4842);
    }
}
