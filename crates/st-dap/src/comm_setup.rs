//! Communication setup helpers for the DAP server.
//!
//! Mirrors `st-cli`'s `comm_setup` module: reads `plc-project.yaml`, loads
//! device profiles, generates ST source for the device globals, and starts
//! a web UI per simulated device.

use st_comm_api::{write_io_map_file, CommConfig, DeviceProfile, EngineProjectConfig, IoValue};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub struct CommSetup {
    pub config: CommConfig,
    pub profiles: HashMap<String, DeviceProfile>,
    pub io_map_path: PathBuf,
    pub device_states: Vec<DeviceState>,
    pub engine: EngineProjectConfig,
}

/// Standalone loader for the optional `engine:` section. Used when there's no
/// comm config (or no devices) but we still want project-wide engine settings
/// like `cycle_time` to apply.
pub fn load_engine_config(project_root: &Path) -> EngineProjectConfig {
    let Some(yaml_path) = find_project_yaml(project_root) else {
        return EngineProjectConfig::default();
    };
    let Ok(yaml_text) = std::fs::read_to_string(&yaml_path) else {
        return EngineProjectConfig::default();
    };
    EngineProjectConfig::from_project_yaml(&yaml_text).unwrap_or_default()
}

pub struct DeviceState {
    pub name: String,
    pub profile: DeviceProfile,
    pub state: Arc<Mutex<HashMap<String, IoValue>>>,
}

pub fn load_for_project(project_root: &Path) -> Result<Option<CommSetup>, String> {
    let yaml_path = find_project_yaml(project_root);
    let Some(yaml_path) = yaml_path else {
        return Ok(None);
    };

    let yaml_text = std::fs::read_to_string(&yaml_path)
        .map_err(|e| format!("Cannot read {}: {e}", yaml_path.display()))?;

    let config = CommConfig::from_project_yaml(&yaml_text)?;
    let engine = EngineProjectConfig::from_project_yaml(&yaml_text).unwrap_or_default();
    if config.devices.is_empty() {
        return Ok(None);
    }

    let profile_dirs = parse_profile_dirs(&yaml_text, project_root);

    let mut profiles = HashMap::new();
    for dev in &config.devices {
        if profiles.contains_key(&dev.device_profile) {
            continue;
        }
        let profile = load_profile(&dev.device_profile, &profile_dirs)?;
        profiles.insert(dev.device_profile.clone(), profile);
    }

    let io_map_path = write_io_map_file(project_root, &profiles, &config.devices)?;

    Ok(Some(CommSetup {
        config,
        profiles,
        io_map_path,
        device_states: Vec::new(),
        engine,
    }))
}

fn find_project_yaml(root: &Path) -> Option<PathBuf> {
    for name in ["plc-project.yaml", "plc-project.yml"] {
        let p = root.join(name);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn parse_profile_dirs(yaml_text: &str, project_root: &Path) -> Vec<PathBuf> {
    let value: serde_yaml::Value = match serde_yaml::from_str(yaml_text) {
        Ok(v) => v,
        Err(_) => return default_profile_dirs(project_root),
    };

    let dirs_val = value
        .get("profile_dirs")
        .or_else(|| value.get("comm").and_then(|c| c.get("profile_dirs")));

    if let Some(serde_yaml::Value::Sequence(seq)) = dirs_val {
        let mut dirs: Vec<PathBuf> = seq
            .iter()
            .filter_map(|v| v.as_str())
            .map(|s| {
                let p = Path::new(s);
                if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    project_root.join(p)
                }
            })
            .collect();
        dirs.extend(default_profile_dirs(project_root));
        return dirs;
    }

    default_profile_dirs(project_root)
}

fn default_profile_dirs(project_root: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![project_root.join("profiles")];
    let mut cur = project_root.to_path_buf();
    for _ in 0..6 {
        if let Some(parent) = cur.parent() {
            let candidate = parent.join("profiles");
            if candidate.is_dir() {
                dirs.push(candidate);
            }
            cur = parent.to_path_buf();
        } else {
            break;
        }
    }
    dirs
}

fn load_profile(name: &str, search_dirs: &[PathBuf]) -> Result<DeviceProfile, String> {
    for dir in search_dirs {
        for ext in ["yaml", "yml"] {
            let candidate = dir.join(format!("{name}.{ext}"));
            if candidate.exists() {
                return DeviceProfile::from_file(&candidate);
            }
        }
    }
    Err(format!(
        "Device profile '{name}' not found in any of: {}",
        search_dirs
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    ))
}

/// Spawn the web UIs for every device on a background tokio runtime thread.
pub fn start_web_uis(setup: &CommSetup, base_port: u16) {
    if setup.device_states.is_empty() {
        return;
    }

    let states: Vec<(String, DeviceProfile, Arc<Mutex<HashMap<String, IoValue>>>, u16)> = setup
        .device_states
        .iter()
        .enumerate()
        .map(|(i, ds)| {
            (
                ds.name.clone(),
                ds.profile.clone(),
                Arc::clone(&ds.state),
                base_port + i as u16,
            )
        })
        .collect();

    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("[DAP-COMM] Failed to start web UI runtime: {e}");
                return;
            }
        };

        rt.block_on(async {
            for (name, profile, state, port) in states {
                tokio::spawn(st_comm_sim::web::start_web_ui(name, profile, state, port));
            }
            std::future::pending::<()>().await;
        });
    });

    std::thread::sleep(std::time::Duration::from_millis(100));
}
