//! Program management API endpoints.

use crate::error::ApiError;
use crate::runtime_manager::{OnlineChangeReport, RuntimeStatus};
use crate::server::AppState;
use axum::extract::{Multipart, State};
use axum::Json;
use serde::Serialize;
use std::sync::Arc;

#[derive(Serialize)]
pub struct UploadResponse {
    pub success: bool,
    pub program: crate::program_store::ProgramMetadata,
}

/// POST /api/v1/program/upload — upload a program bundle.
pub async fn upload(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<UploadResponse>, ApiError> {
    // Extract the first field as bundle bytes
    let field = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::invalid_bundle(format!("Multipart read error: {e}")))?
        .ok_or_else(|| ApiError::invalid_bundle("No file field in upload"))?;

    let data = field
        .bytes()
        .await
        .map_err(|e| ApiError::invalid_bundle(format!("Cannot read upload data: {e}")))?;

    let mut store = state.program_store.write().unwrap();
    let metadata = store.store_bundle(&data)?;

    Ok(Json(UploadResponse {
        success: true,
        program: metadata,
    }))
}

/// GET /api/v1/program/info — current program metadata.
pub async fn info(
    State(state): State<Arc<AppState>>,
) -> Result<Json<crate::program_store::ProgramMetadata>, ApiError> {
    let store = state.program_store.read().unwrap();
    let meta = store
        .current_program()
        .ok_or_else(|| ApiError::not_found("No program deployed"))?;
    Ok(Json(meta.clone()))
}

/// POST /api/v1/program/start — start the PLC runtime.
pub async fn start(State(state): State<Arc<AppState>>) -> Result<Json<serde_json::Value>, ApiError> {
    let (module, program_name) = {
        let store = state.program_store.read().unwrap();
        store.load_module()?
    };

    let program_meta = {
        let store = state.program_store.read().unwrap();
        store.current_program().cloned().ok_or_else(|| {
            ApiError::not_found("No program deployed")
        })?
    };

    // Parse cycle_time from the bundle's plc-project.yaml, default 10ms.
    let cycle_time = {
        let yaml_path = state.program_store.read().unwrap().project_yaml_path();
        let from_project = std::fs::read_to_string(&yaml_path)
            .ok()
            .and_then(|yaml| {
                let cfg =
                    st_comm_api::config::EngineProjectConfig::from_project_yaml(&yaml).ok()?;
                tracing::info!("cycle_time from plc-project.yaml: {:?}", cfg.cycle_time);
                cfg.cycle_time
            });
        if from_project.is_none() {
            tracing::info!(
                "No cycle_time in plc-project.yaml (path={}), using default 10ms",
                yaml_path.display()
            );
        }
        Some(from_project.unwrap_or(std::time::Duration::from_millis(10)))
    };

    // Build native FB registry from device profiles persisted in the bundle.
    // This enables NativeFb::execute() to run on the target, bridging device
    // I/O between the simulated web UI and the FB instance fields.
    let native_fbs = {
        let profiles_dir = state.program_store.read().unwrap().profiles_dir();
        build_native_fb_registry(&profiles_dir)
    };
    if let Some(ref reg) = native_fbs {
        tracing::info!("Native FB registry: {} type(s) from bundled profiles", reg.len());
    }

    state
        .runtime_manager
        .start(module, program_name, cycle_time, program_meta, native_fbs.map(std::sync::Arc::new))
        .await?;

    Ok(Json(serde_json::json!({ "success": true, "status": "starting" })))
}

/// Build a [`NativeFbRegistry`] from YAML profiles in the given directory.
///
/// Dispatches on each profile's `protocol:` field, matching the
/// pattern used by `st-cli` (see `crates/st-cli/src/comm_setup.rs`).
/// The agent intentionally supports a smaller set than the CLI —
/// the agent runs on production targets, so we only wire in
/// protocols that have both a runtime FB **and** a published profile
/// schema:
///
/// - `simulated` — in-memory dummy device (development / tests)
/// - `upp` — Universal Pyrometer Protocol over RS485 (Impac IGAR 6
///   etc.)
///
/// All UPP devices on the same `link` (i.e. same serial port) share a
/// single [`BusManager`](st_comm_serial::BusManager) so writes /
/// reads are serialised on the bus. The `SerialLink` FB is
/// auto-registered when at least one serial-line device is present.
///
/// Profiles whose `protocol:` we don't support log a warning and are
/// skipped — the program still loads, just without that device.
fn build_native_fb_registry(
    profiles_dir: &std::path::Path,
) -> Option<st_comm_api::NativeFbRegistry> {
    if !profiles_dir.is_dir() {
        return None;
    }
    let entries = std::fs::read_dir(profiles_dir).ok()?;

    let transport_map = st_comm_serial::new_transport_map();
    let bus_manager = std::sync::Arc::new(st_comm_serial::BusManager::new(
        std::sync::Arc::clone(&transport_map),
    ));

    let mut registry = st_comm_api::NativeFbRegistry::new();
    let mut has_serial_protocol = false;

    for entry in entries.flatten() {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext != "yaml" && ext != "yml" {
            continue;
        }
        let Ok(profile) = st_comm_api::DeviceProfile::from_file(&path) else {
            tracing::warn!("Failed to parse device profile {}", path.display());
            continue;
        };
        let name = profile.name.clone();
        let protocol = profile.protocol.as_deref().unwrap_or("simulated");
        match protocol {
            "simulated" => {
                registry.register(Box::new(
                    st_comm_sim::SimulatedNativeFb::new(&name, profile),
                ));
            }
            "upp" => {
                registry.register(Box::new(st_comm_upp::UppDeviceNativeFb::new(
                    profile,
                    std::sync::Arc::clone(&bus_manager),
                )));
                has_serial_protocol = true;
            }
            other => {
                tracing::warn!(
                    "Profile {name:?} uses unsupported protocol {other:?}, skipping"
                );
            }
        }
    }

    // Auto-register the SerialLink FB when at least one serial-line
    // device was loaded. SerialLink opens / configures the port at
    // FB-init time and registers the transport in `transport_map`;
    // the BusManager then drives polling for every device on it.
    if has_serial_protocol {
        registry.register(Box::new(
            st_comm_serial::SerialLinkNativeFb::with_transport_map(
                std::sync::Arc::clone(&transport_map),
            ),
        ));
    }

    if registry.is_empty() { None } else { Some(registry) }
}

/// POST /api/v1/program/stop — stop the PLC runtime.
pub async fn stop(State(state): State<Arc<AppState>>) -> Result<Json<serde_json::Value>, ApiError> {
    state.runtime_manager.stop().await?;
    Ok(Json(serde_json::json!({ "success": true, "status": "stopping" })))
}

/// POST /api/v1/program/restart — restart the PLC runtime.
pub async fn restart(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let current_status = state.runtime_manager.state().status;
    if current_status == RuntimeStatus::Running {
        state.runtime_manager.stop().await?;
        // Wait for the runtime to actually stop
        for _ in 0..50 {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            if state.runtime_manager.state().status == RuntimeStatus::Idle {
                break;
            }
        }
    }

    // Re-start
    start(State(state)).await
}

/// Method used to apply a `program/update` request.
#[derive(Serialize, Debug, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum UpdateMethod {
    /// Hot-swapped without stopping the engine. Variables migrated.
    OnlineChange,
    /// Engine stopped, new program loaded, engine restarted.
    Restart,
    /// Runtime was idle when update arrived; bundle stored, no start.
    ColdReplace,
    /// First deployment — no previous program existed.
    InitialDeploy,
}

#[derive(Serialize)]
pub struct UpdateResponse {
    pub success: bool,
    pub method: UpdateMethod,
    pub downtime_ms: u64,
    pub program: crate::program_store::ProgramMetadata,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub online_change: Option<OnlineChangeReport>,
}

/// POST /api/v1/program/update — receive a new bundle and apply it.
///
/// If the runtime is running and the new bundle is layout-compatible with
/// the running one, the engine performs an online change (zero downtime,
/// variable state preserved). Otherwise the runtime is stopped, the new
/// bundle is stored, and the runtime is restarted from it.
pub async fn update(
    State(state): State<Arc<AppState>>,
    mut multipart: Multipart,
) -> Result<Json<UpdateResponse>, ApiError> {
    // 1. Read the uploaded bundle bytes.
    let field = multipart
        .next_field()
        .await
        .map_err(|e| ApiError::invalid_bundle(format!("Multipart read error: {e}")))?
        .ok_or_else(|| ApiError::invalid_bundle("No file field in upload"))?;
    let data = field
        .bytes()
        .await
        .map_err(|e| ApiError::invalid_bundle(format!("Cannot read upload data: {e}")))?;

    let initial_status = state.runtime_manager.state().status;
    let had_program_before = state.program_store.read().unwrap().current_program().is_some();

    // 2. Persist the new bundle (extract + verify + write to disk). This
    //    overwrites the previously deployed program. The running engine
    //    keeps cycling on its in-memory module copy until we either hot-
    //    swap or stop/start.
    let new_meta = {
        let mut store = state.program_store.write().unwrap();
        store.store_bundle(&data)?
    };

    // 3. Decide path based on prior runtime state.
    match initial_status {
        RuntimeStatus::Running | RuntimeStatus::DebugPaused => {
            // Try online change first (zero downtime).
            let (new_module, _name) = {
                let store = state.program_store.read().unwrap();
                store.load_module()?
            };

            let started = std::time::Instant::now();
            match state
                .runtime_manager
                .online_change(new_module, new_meta.clone())
                .await
            {
                Ok(report) => {
                    let downtime_ms = started.elapsed().as_millis() as u64;
                    Ok(Json(UpdateResponse {
                        success: true,
                        method: UpdateMethod::OnlineChange,
                        downtime_ms,
                        program: new_meta,
                        online_change: Some(report),
                    }))
                }
                Err(e) if e.code == "online_change_incompatible" => {
                    // Layout changed — fall back to a full restart.
                    let downtime_ms = restart_runtime(&state).await?;
                    Ok(Json(UpdateResponse {
                        success: true,
                        method: UpdateMethod::Restart,
                        downtime_ms,
                        program: new_meta,
                        online_change: None,
                    }))
                }
                Err(e) => Err(e),
            }
        }
        RuntimeStatus::Idle | RuntimeStatus::Error => {
            // No engine to swap — the new bundle is stored, but we don't
            // auto-start. The caller can POST /program/start when ready.
            Ok(Json(UpdateResponse {
                success: true,
                method: if had_program_before {
                    UpdateMethod::ColdReplace
                } else {
                    UpdateMethod::InitialDeploy
                },
                downtime_ms: 0,
                program: new_meta,
                online_change: None,
            }))
        }
        RuntimeStatus::Starting | RuntimeStatus::Stopping => {
            Err(ApiError::internal(
                "Runtime is transitioning state; retry update in a moment",
            ))
        }
    }
}

/// Stop the engine, then start it again with whatever bundle is stored.
/// Returns the wall-clock downtime in milliseconds.
async fn restart_runtime(state: &Arc<AppState>) -> Result<u64, ApiError> {
    let started = std::time::Instant::now();

    // Stop the engine if it's still running. Tolerate already-stopped.
    let _ = state.runtime_manager.stop().await;
    for _ in 0..100 {
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        if state.runtime_manager.state().status == RuntimeStatus::Idle {
            break;
        }
    }

    if state.runtime_manager.state().status != RuntimeStatus::Idle {
        return Err(ApiError::internal(
            "Engine did not become idle within the restart deadline",
        ));
    }

    // Re-start using the freshly stored bundle. Reuse the existing start
    // handler so it picks up cycle_time + native FBs the same way as upload.
    let _ = start(State(Arc::clone(state))).await?;

    Ok(started.elapsed().as_millis() as u64)
}

/// DELETE /api/v1/program — remove the deployed program.
pub async fn remove(
    State(state): State<Arc<AppState>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Stop runtime if running
    let current_status = state.runtime_manager.state().status;
    if current_status == RuntimeStatus::Running {
        state.runtime_manager.stop().await?;
        for _ in 0..50 {
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            if state.runtime_manager.state().status == RuntimeStatus::Idle {
                break;
            }
        }
    }

    let mut store = state.program_store.write().unwrap();
    store.remove_current()?;

    Ok(Json(serde_json::json!({ "success": true })))
}
