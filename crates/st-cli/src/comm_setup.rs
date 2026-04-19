//! Communication setup helpers for `st-cli`.
//!
//! Discovers device profiles from the project's search paths, builds a
//! NativeFbRegistry, and starts web UIs for simulated devices.

use st_comm_api::{DeviceProfile, EngineProjectConfig, NativeFbRegistry};
use st_comm_modbus::device_fb::ModbusRtuDeviceNativeFb;
use st_comm_serial::SerialLinkNativeFb;
use st_comm_sim::SimulatedNativeFb;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// Shared device state for the simulated device web UI.
pub struct DeviceState {
    pub name: String,
    pub profile: DeviceProfile,
    pub state: Arc<Mutex<HashMap<String, st_comm_api::IoValue>>>,
}

/// Result of loading native FB comm setup for a project.
pub struct NativeCommSetup {
    /// Native FB registry containing all device types from discovered profiles.
    pub registry: NativeFbRegistry,
    /// Device states for web UIs (one per simulated device profile).
    pub device_states: Vec<DeviceState>,
}

/// Load the optional `engine:` section from `plc-project.yaml`.
pub fn load_engine_config(project_root: &Path) -> EngineProjectConfig {
    let Some(yaml_path) = find_project_yaml(project_root) else {
        return EngineProjectConfig::default();
    };
    let Ok(yaml_text) = std::fs::read_to_string(&yaml_path) else {
        return EngineProjectConfig::default();
    };
    EngineProjectConfig::from_project_yaml(&yaml_text).unwrap_or_default()
}

/// Build a [`NativeFbRegistry`] from all device profiles discovered in the
/// project's profile search paths. Each profile becomes a native FB type.
///
/// Returns `Ok(None)` if no profiles exist.
pub fn load_native_fbs_for_project(project_root: &Path) -> Result<Option<NativeCommSetup>, String> {
    let yaml_path = find_project_yaml(project_root);
    let yaml_text = yaml_path
        .as_ref()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .unwrap_or_default();

    let profile_dirs = if yaml_text.is_empty() {
        default_profile_dirs(project_root)
    } else {
        parse_profile_dirs(&yaml_text, project_root)
    };

    let profiles = discover_all_profiles(&profile_dirs);
    if profiles.is_empty() {
        return Ok(None);
    }

    let mut registry = NativeFbRegistry::new();
    let mut device_states = Vec::new();
    let mut has_modbus_rtu = false;

    // Shared transport map for serial link-device binding
    let transport_map = st_comm_serial::new_transport_map();

    for profile in profiles {
        let protocol = profile.protocol.as_deref().unwrap_or("simulated");
        match protocol {
            "simulated" => {
                let sim_fb = SimulatedNativeFb::new(&profile.name, profile.clone());
                let state_handle = sim_fb.state_handle();
                device_states.push(DeviceState {
                    name: profile.name.clone(),
                    profile: profile.clone(),
                    state: state_handle,
                });
                registry.register(Box::new(sim_fb));
            }
            "modbus-rtu" => {
                let modbus_fb = ModbusRtuDeviceNativeFb::new(
                    profile.clone(),
                    Arc::clone(&transport_map),
                );
                registry.register(Box::new(modbus_fb));
                has_modbus_rtu = true;
            }
            other => {
                eprintln!(
                    "[COMM] Profile '{}' uses unsupported protocol '{}', skipping",
                    profile.name, other
                );
            }
        }
    }

    // Auto-register SerialLink if any Modbus RTU devices were found
    if has_modbus_rtu {
        registry.register(Box::new(SerialLinkNativeFb::with_transport_map(
            Arc::clone(&transport_map),
        )));
    }

    if registry.is_empty() {
        return Ok(None);
    }

    eprintln!(
        "[COMM] Loaded {} native FB type(s) from profiles",
        registry.len()
    );

    Ok(Some(NativeCommSetup {
        registry,
        device_states,
    }))
}

/// Discover all device profile YAML files in the given search directories.
fn discover_all_profiles(search_dirs: &[PathBuf]) -> Vec<DeviceProfile> {
    let mut profiles = Vec::new();
    let mut seen_names = std::collections::HashSet::new();

    for dir in search_dirs {
        if !dir.is_dir() {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext != "yaml" && ext != "yml" {
                continue;
            }
            if let Ok(profile) = DeviceProfile::from_file(&path) {
                if seen_names.insert(profile.name.clone()) {
                    profiles.push(profile);
                }
            }
        }
    }
    profiles
}

/// Start web UIs for native FB device states.
pub fn start_native_web_uis(setup: &NativeCommSetup, base_port: u16) {
    if setup.device_states.is_empty() {
        return;
    }

    #[allow(clippy::type_complexity)]
    let states: Vec<(
        String,
        DeviceProfile,
        Arc<Mutex<HashMap<String, st_comm_api::IoValue>>>,
        u16,
    )> = setup
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
                eprintln!("[COMM] Failed to start web UI runtime: {e}");
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

// ── Internal helpers ──────────────────────────────────────────────────

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
