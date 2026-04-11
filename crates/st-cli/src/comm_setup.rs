//! Communication setup helpers used by `st-cli run`.
//!
//! Reads the comm section of a `plc-project.yaml`, loads referenced device
//! profiles, generates ST source code for the device globals, instantiates
//! simulated devices, and starts a web UI for each one.

use st_comm_api::{write_io_map_file, CommConfig, DeviceProfile, EngineProjectConfig};
use st_comm_sim::SimulatedDevice;
use st_engine::Engine;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Result of loading the comm configuration for a project.
pub struct CommSetup {
    /// Parsed `links:` and `devices:` from the project YAML.
    pub config: CommConfig,
    /// Loaded device profiles, keyed by `device_profile` string.
    pub profiles: HashMap<String, DeviceProfile>,
    /// Path of the on-disk `_io_map.st` we wrote into the project root.
    pub io_map_path: PathBuf,
    /// Per-device shared state handles, in the same order as `config.devices`.
    /// Used to start the web UIs after the engine is built.
    pub device_states: Vec<DeviceState>,
}

pub struct DeviceState {
    pub name: String,
    pub profile: DeviceProfile,
    pub state: Arc<Mutex<HashMap<String, st_comm_api::IoValue>>>,
}

/// Load the optional `engine:` section from `plc-project.yaml`. Returns the
/// default (empty) config if there is no project YAML, no `engine:` section,
/// or the file is unreadable. Used for cycle-time and other project-wide
/// engine settings — independent of whether comm devices are configured.
pub fn load_engine_config(project_root: &Path) -> EngineProjectConfig {
    let Some(yaml_path) = find_project_yaml(project_root) else {
        return EngineProjectConfig::default();
    };
    let Ok(yaml_text) = std::fs::read_to_string(&yaml_path) else {
        return EngineProjectConfig::default();
    };
    EngineProjectConfig::from_project_yaml(&yaml_text).unwrap_or_default()
}

/// Load comm config from `plc-project.yaml` in the given project root and
/// regenerate the on-disk I/O map file. Returns `Ok(None)` if there is no
/// project YAML or no comm section.
pub fn load_for_project(project_root: &Path) -> Result<Option<CommSetup>, String> {
    let yaml_path = find_project_yaml(project_root);
    let Some(yaml_path) = yaml_path else {
        return Ok(None);
    };

    let yaml_text = std::fs::read_to_string(&yaml_path)
        .map_err(|e| format!("Cannot read {}: {e}", yaml_path.display()))?;

    let config = CommConfig::from_project_yaml(&yaml_text)?;
    if config.devices.is_empty() {
        return Ok(None);
    }

    // Resolve profile search paths from the YAML (or use defaults).
    let profile_dirs = parse_profile_dirs(&yaml_text, project_root);

    // Load every referenced profile.
    let mut profiles = HashMap::new();
    for dev in &config.devices {
        if profiles.contains_key(&dev.device_profile) {
            continue;
        }
        let profile = load_profile(&dev.device_profile, &profile_dirs)?;
        profiles.insert(dev.device_profile.clone(), profile);
    }

    // Write the I/O map file (`_io_map.st`) to disk so the LSP, semantic
    // checker, compiler, and runtime all see the device globals from the
    // same source. The file is only rewritten if its contents differ from
    // what's already there.
    let io_map_path = write_io_map_file(project_root, &profiles, &config.devices)?;

    Ok(Some(CommSetup {
        config,
        profiles,
        io_map_path,
        device_states: Vec::new(),
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
    // Look for `comm.profile_dirs: [..]` or top-level `profile_dirs: [..]`.
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
        // Always also include the defaults as a fallback.
        dirs.extend(default_profile_dirs(project_root));
        return dirs;
    }

    default_profile_dirs(project_root)
}

fn default_profile_dirs(project_root: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![project_root.join("profiles")];
    // Walk up looking for a sibling `profiles/` directory (workspace root).
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

/// Build simulated devices from the configuration and register them with the engine.
/// Populates `setup.device_states` so the web UIs can be started afterwards.
pub fn register_simulated_devices(setup: &mut CommSetup, engine: &mut Engine) {
    for dev_cfg in &setup.config.devices {
        let Some(profile) = setup.profiles.get(&dev_cfg.device_profile) else {
            eprintln!(
                "[COMM] Device '{}' references unknown profile '{}'",
                dev_cfg.name, dev_cfg.device_profile
            );
            continue;
        };

        // Only "simulated" protocol is supported in Phase 13a.
        if dev_cfg.protocol != "simulated" {
            eprintln!(
                "[COMM] Skipping device '{}': protocol '{}' not yet implemented",
                dev_cfg.name, dev_cfg.protocol
            );
            continue;
        }

        let sim_device = SimulatedDevice::new(&dev_cfg.name, profile.clone());
        let state_handle = sim_device.state_handle();

        // Split the borrow: take an immutable VM ref first, then call comm_mut.
        // We need both, so use a method that borrows them disjointly.
        let device_box: Box<dyn st_comm_api::CommDevice> = Box::new(sim_device);
        register_one(engine, device_box, &dev_cfg.name);

        setup.device_states.push(DeviceState {
            name: dev_cfg.name.clone(),
            profile: profile.clone(),
            state: state_handle,
        });
    }

    eprintln!(
        "[COMM] Registered {} simulated device(s)",
        setup.device_states.len()
    );
}

fn register_one(
    engine: &mut Engine,
    device: Box<dyn st_comm_api::CommDevice>,
    instance_name: &str,
) {
    // The borrow checker insists we don't hold &Vm and &mut CommManager at the
    // same time, so split the call by going through the engine's helper.
    engine.register_comm_device(device, instance_name);
}

/// Spawn a tokio runtime on a background thread and start one web UI per device.
/// Returns immediately; the web UIs run for the lifetime of the program.
/// The starting port is `base_port`, incrementing for each device.
pub fn start_web_uis(setup: &CommSetup, base_port: u16) {
    if setup.device_states.is_empty() {
        return;
    }

    #[allow(clippy::type_complexity)]
    let states: Vec<(String, DeviceProfile, Arc<Mutex<HashMap<String, st_comm_api::IoValue>>>, u16)> =
        setup
            .device_states
            .iter()
            .enumerate()
            .map(|(i, ds)| (ds.name.clone(), ds.profile.clone(), Arc::clone(&ds.state), base_port + i as u16))
            .collect();

    std::thread::spawn(move || {
        let rt = match tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
        {
            Ok(rt) => rt,
            Err(e) => {
                eprintln!("[COMM] Failed to start web UI runtime: {e}");
                return;
            }
        };

        rt.block_on(async {
            for (name, profile, state, port) in states {
                tokio::spawn(st_comm_sim::web::start_web_ui(
                    name, profile, state, port,
                ));
            }
            // Keep the runtime alive forever.
            std::future::pending::<()>().await;
        });
    });

    // Brief pause to give web servers a chance to bind before the engine starts.
    std::thread::sleep(std::time::Duration::from_millis(100));
}
