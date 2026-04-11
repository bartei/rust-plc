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

    // Parse cycle_time from the bundle's project YAML if available
    let cycle_time = state.config.runtime.watchdog_ms.map(|ms| {
        std::time::Duration::from_millis(ms)
    });
    // Use a default 10ms cycle time if not configured
    let cycle_time = cycle_time.or(Some(std::time::Duration::from_millis(10)));

    state
        .runtime_manager
        .start(module, program_name, cycle_time, program_meta)
        .await?;

    Ok(Json(serde_json::json!({ "success": true, "status": "starting" })))
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
