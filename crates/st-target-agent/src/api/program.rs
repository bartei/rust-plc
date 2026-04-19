//! Program management API endpoints.

use crate::error::ApiError;
use crate::runtime_manager::RuntimeStatus;
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
fn build_native_fb_registry(
    profiles_dir: &std::path::Path,
) -> Option<st_comm_api::NativeFbRegistry> {
    if !profiles_dir.is_dir() {
        return None;
    }
    let entries = std::fs::read_dir(profiles_dir).ok()?;
    let mut registry = st_comm_api::NativeFbRegistry::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext != "yaml" && ext != "yml" {
            continue;
        }
        if let Ok(profile) = st_comm_api::DeviceProfile::from_file(&path) {
            let name = profile.name.clone();
            registry.register(Box::new(
                st_comm_sim::SimulatedNativeFb::new(&name, profile),
            ));
        }
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
