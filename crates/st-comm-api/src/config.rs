//! Engine configuration parsed from `plc-project.yaml`.

use std::time::Duration;

/// Parse a human duration string of the form `<integer><unit>` where unit is
/// `ns`, `us`, `µs`, `ms`, or `s`. Whitespace tolerated.
pub fn parse_duration(s: &str) -> Result<Duration, String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        return Err("empty duration string".into());
    }
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
#[derive(Debug, Clone, Default)]
pub struct EngineProjectConfig {
    pub cycle_time: Option<Duration>,
    pub retain_checkpoint_cycles: Option<u32>,
}

impl EngineProjectConfig {
    /// Parse the optional `engine:` section out of a full project YAML.
    pub fn from_project_yaml(yaml: &str) -> Result<Self, String> {
        let value: serde_yaml::Value =
            serde_yaml::from_str(yaml).map_err(|e| format!("Invalid YAML: {e}"))?;

        let Some(engine) = value.get("engine") else {
            return Ok(Self::default());
        };

        let cycle_time = match engine.get("cycle_time") {
            Some(serde_yaml::Value::String(s)) => Some(parse_duration(s)?),
            Some(serde_yaml::Value::Number(n)) => {
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

        let retain_checkpoint_cycles = engine
            .get("retain")
            .and_then(|r| r.get("checkpoint_cycles"))
            .and_then(|v| v.as_u64())
            .map(|n| n as u32);

        Ok(Self { cycle_time, retain_checkpoint_cycles })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let yaml = "engine:\n  cycle_time: 10ms\n";
        let cfg = EngineProjectConfig::from_project_yaml(yaml).unwrap();
        assert_eq!(cfg.cycle_time, Some(Duration::from_millis(10)));
    }

    #[test]
    fn parse_engine_config_bare_number_is_ms() {
        let yaml = "engine:\n  cycle_time: 5\n";
        let cfg = EngineProjectConfig::from_project_yaml(yaml).unwrap();
        assert_eq!(cfg.cycle_time, Some(Duration::from_millis(5)));
    }

    #[test]
    fn parse_engine_config_absent() {
        let yaml = "name: NoEngine\n";
        let cfg = EngineProjectConfig::from_project_yaml(yaml).unwrap();
        assert_eq!(cfg.cycle_time, None);
        assert_eq!(cfg.retain_checkpoint_cycles, None);
    }

    #[test]
    fn parse_engine_retain_config() {
        let yaml = "engine:\n  cycle_time: 10ms\n  retain:\n    checkpoint_cycles: 500\n";
        let cfg = EngineProjectConfig::from_project_yaml(yaml).unwrap();
        assert_eq!(cfg.cycle_time, Some(Duration::from_millis(10)));
        assert_eq!(cfg.retain_checkpoint_cycles, Some(500));
    }
}
