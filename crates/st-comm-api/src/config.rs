//! Project YAML config parser for `links:` and `devices:` sections.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

/// Parse a human duration string of the form `<integer><unit>` where unit is
/// `ns`, `us`, `µs`, `ms`, or `s`. Whitespace tolerated. Returned as a
/// `Duration`. Used by both `engine.cycle_time` and `devices[].cycle_time`.
pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err("empty duration string".into());
    }
    // Find the boundary between digits and the unit suffix.
    let split_at = trimmed
        .find(|c: char| !c.is_ascii_digit() && c != '.')
        .unwrap_or(trimmed.len());
    let (num_str, unit) = trimmed.split_at(split_at);
    let num: f64 = num_str
        .parse()
        .map_err(|_| format!("invalid number in duration '{s}'"))?;
    let unit = unit.trim();
    let nanos = match unit {
        "ns" => num,
        "us" | "µs" => num * 1_000.0,
        "ms" => num * 1_000_000.0,
        "s" | "" => num * 1_000_000_000.0,
        other => return Err(format!("unknown duration unit '{other}' in '{s}'")),
    };
    if nanos < 0.0 {
        return Err(format!("duration cannot be negative: '{s}'"));
    }
    Ok(Duration::from_nanos(nanos as u64))
}

/// Engine-level configuration parsed from the optional `engine:` section of
/// `plc-project.yaml`.
///
/// ```yaml
/// engine:
///   cycle_time: 10ms     # optional; absent ⇒ run as fast as possible
/// ```
#[derive(Debug, Clone, Default)]
pub struct EngineProjectConfig {
    /// Target scan cycle time. `None` means "run as fast as the CPU allows".
    pub cycle_time: Option<Duration>,
}

impl EngineProjectConfig {
    /// Parse the optional `engine:` section out of a full project YAML.
    /// Returns the default (no cycle time) if the section is absent.
    pub fn from_project_yaml(yaml: &str) -> Result<Self, String> {
        let value: serde_yaml::Value =
            serde_yaml::from_str(yaml).map_err(|e| format!("Invalid YAML: {e}"))?;

        let Some(engine) = value.get("engine") else {
            return Ok(Self::default());
        };

        let cycle_time = match engine.get("cycle_time") {
            Some(serde_yaml::Value::String(s)) => Some(parse_duration(s)?),
            Some(serde_yaml::Value::Number(n)) => {
                // Bare number → treat as milliseconds (convenient default)
                let ms = n
                    .as_u64()
                    .ok_or_else(|| "engine.cycle_time must be a positive number".to_string())?;
                Some(Duration::from_millis(ms))
            }
            Some(_) => {
                return Err(
                    "engine.cycle_time must be a duration string (e.g. '10ms') or a number of milliseconds"
                        .into(),
                );
            }
            None => None,
        };

        Ok(Self { cycle_time })
    }
}

/// Communication configuration from plc-project.yaml.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CommConfig {
    /// Communication links (physical transport channels).
    #[serde(default)]
    pub links: Vec<LinkConfig>,

    /// Communication devices (addressable units on links).
    #[serde(default)]
    pub devices: Vec<DeviceConfig>,
}

/// Configuration for a communication link.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkConfig {
    /// Unique link name — referenced by devices.
    pub name: String,

    /// Link type: "tcp", "serial", "simulated".
    #[serde(rename = "type")]
    pub link_type: String,

    /// TCP/UDP: remote host.
    #[serde(default)]
    pub host: Option<String>,

    /// TCP/UDP: port number. Serial: device path.
    #[serde(default)]
    pub port: Option<serde_yaml::Value>,

    /// Response timeout (e.g., "500ms").
    #[serde(default)]
    pub timeout: Option<String>,

    /// Serial: baud rate.
    #[serde(default)]
    pub baud: Option<u32>,

    /// Serial: parity ("none", "even", "odd").
    #[serde(default)]
    pub parity: Option<String>,

    /// Serial: data bits (7, 8).
    #[serde(default)]
    pub data_bits: Option<u8>,

    /// Serial: stop bits (1, 2).
    #[serde(default)]
    pub stop_bits: Option<u8>,
}

/// Configuration for a communication device.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceConfig {
    /// Global variable name in ST code.
    pub name: String,

    /// Link name this device communicates over.
    pub link: String,

    /// Application-layer protocol: "modbus-tcp", "modbus-rtu", "simulated".
    pub protocol: String,

    /// Protocol-specific address (e.g., Modbus unit ID).
    #[serde(default)]
    pub unit_id: Option<u16>,

    /// I/O mode: "cyclic" or "acyclic".
    #[serde(default = "default_mode")]
    pub mode: String,

    /// Minimum interval between I/O updates (e.g., "10ms").
    #[serde(default)]
    pub cycle_time: Option<String>,

    /// Device profile name (without .yaml extension).
    pub device_profile: String,

    /// Any extra protocol-specific config.
    #[serde(flatten)]
    pub extra: HashMap<String, serde_yaml::Value>,
}

fn default_mode() -> String {
    "cyclic".to_string()
}

impl CommConfig {
    /// Parse communication config from a YAML string (the full project YAML).
    pub fn from_project_yaml(yaml: &str) -> Result<Self, String> {
        // Parse the full project YAML and extract links + devices
        let value: serde_yaml::Value =
            serde_yaml::from_str(yaml).map_err(|e| format!("Invalid YAML: {e}"))?;

        let links: Vec<LinkConfig> = if let Some(links_val) = value.get("links") {
            serde_yaml::from_value(links_val.clone())
                .map_err(|e| format!("Invalid 'links' config: {e}"))?
        } else {
            Vec::new()
        };

        let devices: Vec<DeviceConfig> = if let Some(devices_val) = value.get("devices") {
            serde_yaml::from_value(devices_val.clone())
                .map_err(|e| format!("Invalid 'devices' config: {e}"))?
        } else {
            Vec::new()
        };

        Ok(CommConfig { links, devices })
    }

    /// Find a link config by name.
    pub fn find_link(&self, name: &str) -> Option<&LinkConfig> {
        self.links.iter().find(|l| l.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simulated_config() {
        let yaml = r#"
name: TestProject
entryPoint: Main

links:
  - name: sim_link
    type: simulated

devices:
  - name: io_rack
    link: sim_link
    protocol: simulated
    mode: cyclic
    device_profile: sim_8di_4ai_4do_2ao
"#;
        let config = CommConfig::from_project_yaml(yaml).unwrap();
        assert_eq!(config.links.len(), 1);
        assert_eq!(config.links[0].name, "sim_link");
        assert_eq!(config.links[0].link_type, "simulated");

        assert_eq!(config.devices.len(), 1);
        assert_eq!(config.devices[0].name, "io_rack");
        assert_eq!(config.devices[0].link, "sim_link");
        assert_eq!(config.devices[0].device_profile, "sim_8di_4ai_4do_2ao");
    }

    #[test]
    fn parse_modbus_config() {
        let yaml = r#"
links:
  - name: eth1
    type: tcp
    host: 192.168.1.100
    port: 502
    timeout: 500ms
  - name: rs485
    type: serial
    port: /dev/ttyUSB0
    baud: 19200
    parity: even
    data_bits: 8
    stop_bits: 1

devices:
  - name: rack_left
    link: eth1
    protocol: modbus-tcp
    unit_id: 1
    mode: cyclic
    cycle_time: 10ms
    device_profile: wago_750_352
  - name: pump_vfd
    link: rs485
    protocol: modbus-rtu
    unit_id: 3
    mode: cyclic
    device_profile: abb_acs580
"#;
        let config = CommConfig::from_project_yaml(yaml).unwrap();
        assert_eq!(config.links.len(), 2);
        assert_eq!(config.devices.len(), 2);

        let eth = &config.links[0];
        assert_eq!(eth.host.as_deref(), Some("192.168.1.100"));

        let rs485 = &config.links[1];
        assert_eq!(rs485.baud, Some(19200));
        assert_eq!(rs485.parity.as_deref(), Some("even"));

        let pump = &config.devices[1];
        assert_eq!(pump.unit_id, Some(3));
        assert_eq!(pump.protocol, "modbus-rtu");
    }

    #[test]
    fn parse_no_comm_section() {
        let yaml = r#"
name: SimpleProject
entryPoint: Main
"#;
        let config = CommConfig::from_project_yaml(yaml).unwrap();
        assert!(config.links.is_empty());
        assert!(config.devices.is_empty());
    }

    #[test]
    fn parse_duration_units() {
        assert_eq!(parse_duration("10ms").unwrap(), Duration::from_millis(10));
        assert_eq!(parse_duration("500us").unwrap(), Duration::from_micros(500));
        assert_eq!(parse_duration("500µs").unwrap(), Duration::from_micros(500));
        assert_eq!(parse_duration("1s").unwrap(), Duration::from_secs(1));
        assert_eq!(parse_duration("250ns").unwrap(), Duration::from_nanos(250));
        assert_eq!(parse_duration(" 10ms ").unwrap(), Duration::from_millis(10));
        assert!(parse_duration("").is_err());
        assert!(parse_duration("abc").is_err());
        assert!(parse_duration("10xy").is_err());
    }

    #[test]
    fn parse_engine_config_present() {
        let yaml = r#"
name: TestProject
engine:
  cycle_time: 10ms
"#;
        let cfg = EngineProjectConfig::from_project_yaml(yaml).unwrap();
        assert_eq!(cfg.cycle_time, Some(Duration::from_millis(10)));
    }

    #[test]
    fn parse_engine_config_bare_number_is_ms() {
        let yaml = r#"
engine:
  cycle_time: 5
"#;
        let cfg = EngineProjectConfig::from_project_yaml(yaml).unwrap();
        assert_eq!(cfg.cycle_time, Some(Duration::from_millis(5)));
    }

    #[test]
    fn parse_engine_config_absent() {
        let yaml = r#"
name: NoEngine
"#;
        let cfg = EngineProjectConfig::from_project_yaml(yaml).unwrap();
        assert_eq!(cfg.cycle_time, None);
    }

    #[test]
    fn find_link_by_name() {
        let yaml = r#"
links:
  - name: link_a
    type: tcp
    host: 10.0.0.1
    port: 502
  - name: link_b
    type: serial
    port: /dev/ttyUSB0
    baud: 9600
devices: []
"#;
        let config = CommConfig::from_project_yaml(yaml).unwrap();
        assert!(config.find_link("link_a").is_some());
        assert!(config.find_link("link_b").is_some());
        assert!(config.find_link("nonexistent").is_none());
    }
}
