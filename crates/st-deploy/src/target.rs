//! Deployment target configuration parsed from `plc-project.yaml`.
//!
//! The `targets:` section defines remote devices that programs can be deployed to.
//! Each target specifies SSH connection details, OS/architecture, and agent settings.

use serde::Deserialize;

/// Authentication mode for connecting to a target.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AuthMode {
    /// SSH key authentication (default). Uses the developer's SSH agent or key file.
    #[default]
    Key,
    /// SSH password authentication. Password is prompted at deploy time.
    Password,
    /// Direct agent API connection (no SSH). Requires TLS + token auth on the agent.
    Agent,
}

/// A single deployment target — a remote device that can receive program bundles.
#[derive(Debug, Clone, Deserialize)]
pub struct Target {
    /// Human-readable name (used in `--target <name>` and VS Code picker).
    pub name: String,
    /// Hostname or IP address.
    pub host: String,
    /// SSH username.
    #[serde(default = "default_user")]
    pub user: String,
    /// Authentication mode.
    #[serde(default)]
    pub auth: AuthMode,
    /// Target operating system.
    #[serde(default = "default_os")]
    pub os: String,
    /// Target CPU architecture (x86_64, aarch64, armv7, etc.).
    #[serde(default = "default_arch")]
    pub arch: String,
    /// Agent API port on the target.
    #[serde(default = "default_agent_port")]
    pub agent_port: u16,
    /// Path on the target where programs are stored.
    #[serde(default)]
    pub deploy_path: Option<String>,
}

fn default_user() -> String {
    "plc".to_string()
}

fn default_os() -> String {
    "linux".to_string()
}

fn default_arch() -> String {
    "x86_64".to_string()
}

fn default_agent_port() -> u16 {
    4840
}

/// Top-level target configuration from `plc-project.yaml`.
#[derive(Debug, Clone, Default)]
pub struct TargetConfig {
    /// All configured deployment targets.
    pub targets: Vec<Target>,
    /// Default target name (used when `--target` is omitted).
    pub default_target: Option<String>,
}

impl TargetConfig {
    /// Parse the `targets:` and `default_target:` sections from a plc-project.yaml string.
    /// Returns an empty config if neither section is present.
    pub fn from_project_yaml(yaml: &str) -> Result<Self, String> {
        let value: serde_yaml::Value = serde_yaml::from_str(yaml)
            .map_err(|e| format!("Invalid YAML: {e}"))?;

        let targets = if let Some(targets_val) = value.get("targets") {
            serde_yaml::from_value(targets_val.clone())
                .map_err(|e| format!("Invalid targets configuration: {e}"))?
        } else {
            Vec::new()
        };

        let default_target = value
            .get("default_target")
            .and_then(|v| v.as_str())
            .map(String::from);

        // Validate: default_target must reference an existing target
        if let Some(ref dt) = default_target {
            if !targets.iter().any(|t: &Target| t.name == *dt) {
                return Err(format!(
                    "default_target '{dt}' does not match any configured target"
                ));
            }
        }

        // Validate: no duplicate target names
        let mut seen = std::collections::HashSet::new();
        for t in &targets {
            if !seen.insert(&t.name) {
                return Err(format!("Duplicate target name: '{}'", t.name));
            }
        }

        Ok(TargetConfig {
            targets,
            default_target,
        })
    }

    /// Find a target by name.
    pub fn find_target(&self, name: &str) -> Option<&Target> {
        self.targets.iter().find(|t| t.name == name)
    }

    /// Resolve which target to use: explicit name > default_target > error.
    pub fn resolve_target(&self, name: Option<&str>) -> Result<&Target, String> {
        let target_name = match name {
            Some(n) => n,
            None => self.default_target.as_deref().ok_or_else(|| {
                "No target specified and no default_target configured".to_string()
            })?,
        };

        self.find_target(target_name).ok_or_else(|| {
            let available: Vec<&str> = self.targets.iter().map(|t| t.name.as_str()).collect();
            if available.is_empty() {
                format!("Target '{target_name}' not found (no targets configured)")
            } else {
                format!(
                    "Target '{target_name}' not found. Available: {}",
                    available.join(", ")
                )
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_full_target_config() {
        let yaml = r#"
name: TestProject
targets:
  - name: line1-plc
    host: 192.168.1.50
    user: plc
    auth: key
    os: linux
    arch: x86_64
    agent_port: 4840
    deploy_path: /var/lib/st-agent/programs
  - name: test-bench
    host: 10.0.0.100
    user: admin
    auth: password
    os: windows
    arch: x86_64
default_target: line1-plc
"#;
        let config = TargetConfig::from_project_yaml(yaml).unwrap();
        assert_eq!(config.targets.len(), 2);
        assert_eq!(config.default_target, Some("line1-plc".to_string()));

        let t1 = &config.targets[0];
        assert_eq!(t1.name, "line1-plc");
        assert_eq!(t1.host, "192.168.1.50");
        assert_eq!(t1.user, "plc");
        assert_eq!(t1.auth, AuthMode::Key);
        assert_eq!(t1.os, "linux");
        assert_eq!(t1.arch, "x86_64");
        assert_eq!(t1.agent_port, 4840);
        assert_eq!(t1.deploy_path, Some("/var/lib/st-agent/programs".to_string()));

        let t2 = &config.targets[1];
        assert_eq!(t2.name, "test-bench");
        assert_eq!(t2.auth, AuthMode::Password);
        assert_eq!(t2.os, "windows");
    }

    #[test]
    fn parse_minimal_target() {
        let yaml = r#"
targets:
  - name: dev
    host: 192.168.1.10
"#;
        let config = TargetConfig::from_project_yaml(yaml).unwrap();
        assert_eq!(config.targets.len(), 1);
        let t = &config.targets[0];
        assert_eq!(t.user, "plc"); // default
        assert_eq!(t.auth, AuthMode::Key); // default
        assert_eq!(t.os, "linux"); // default
        assert_eq!(t.arch, "x86_64"); // default
        assert_eq!(t.agent_port, 4840); // default
        assert!(t.deploy_path.is_none());
    }

    #[test]
    fn no_targets_section() {
        let yaml = "name: NoTargets\nversion: '1.0'\n";
        let config = TargetConfig::from_project_yaml(yaml).unwrap();
        assert!(config.targets.is_empty());
        assert!(config.default_target.is_none());
    }

    #[test]
    fn default_target_must_exist() {
        let yaml = r#"
targets:
  - name: dev
    host: 192.168.1.10
default_target: production
"#;
        let err = TargetConfig::from_project_yaml(yaml).unwrap_err();
        assert!(err.contains("does not match any configured target"));
    }

    #[test]
    fn duplicate_target_names_rejected() {
        let yaml = r#"
targets:
  - name: dev
    host: 192.168.1.10
  - name: dev
    host: 192.168.1.20
"#;
        let err = TargetConfig::from_project_yaml(yaml).unwrap_err();
        assert!(err.contains("Duplicate target name"));
    }

    #[test]
    fn resolve_explicit_target() {
        let yaml = r#"
targets:
  - name: alpha
    host: 10.0.0.1
  - name: beta
    host: 10.0.0.2
default_target: alpha
"#;
        let config = TargetConfig::from_project_yaml(yaml).unwrap();
        let t = config.resolve_target(Some("beta")).unwrap();
        assert_eq!(t.host, "10.0.0.2");
    }

    #[test]
    fn resolve_default_target() {
        let yaml = r#"
targets:
  - name: alpha
    host: 10.0.0.1
  - name: beta
    host: 10.0.0.2
default_target: alpha
"#;
        let config = TargetConfig::from_project_yaml(yaml).unwrap();
        let t = config.resolve_target(None).unwrap();
        assert_eq!(t.name, "alpha");
    }

    #[test]
    fn resolve_no_default_errors() {
        let yaml = r#"
targets:
  - name: alpha
    host: 10.0.0.1
"#;
        let config = TargetConfig::from_project_yaml(yaml).unwrap();
        let err = config.resolve_target(None).unwrap_err();
        assert!(err.contains("no default_target configured"));
    }

    #[test]
    fn resolve_nonexistent_target_lists_available() {
        let yaml = r#"
targets:
  - name: alpha
    host: 10.0.0.1
  - name: beta
    host: 10.0.0.2
"#;
        let config = TargetConfig::from_project_yaml(yaml).unwrap();
        let err = config.resolve_target(Some("gamma")).unwrap_err();
        assert!(err.contains("gamma"));
        assert!(err.contains("alpha"));
        assert!(err.contains("beta"));
    }

    #[test]
    fn auth_mode_agent() {
        let yaml = r#"
targets:
  - name: direct
    host: 192.168.1.10
    auth: agent
"#;
        let config = TargetConfig::from_project_yaml(yaml).unwrap();
        assert_eq!(config.targets[0].auth, AuthMode::Agent);
    }
}
