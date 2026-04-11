//! Status and health API endpoints.

use crate::error::ApiError;
use crate::runtime_manager::RuntimeStatus;
use crate::server::AppState;
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use serde::Serialize;
use std::sync::Arc;

/// GET /api/v1/status — runtime state + cycle stats.
pub async fn status(
    State(state): State<Arc<AppState>>,
) -> Json<crate::runtime_manager::RuntimeState> {
    Json(state.runtime_manager.state())
}

#[derive(Serialize)]
pub struct HealthResponse {
    pub healthy: bool,
    pub agent: String,
    pub version: String,
}

/// GET /api/v1/health — agent health check (200 or 503).
pub async fn health(
    State(state): State<Arc<AppState>>,
) -> (StatusCode, Json<HealthResponse>) {
    let runtime_state = state.runtime_manager.state();
    let healthy = runtime_state.status != RuntimeStatus::Error
        || runtime_state.restart_count < state.config.runtime.max_restarts;

    let status_code = if healthy {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };

    (
        status_code,
        Json(HealthResponse {
            healthy,
            agent: state.config.agent.name.clone(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }),
    )
}

#[derive(Serialize)]
pub struct TargetInfoResponse {
    pub os: String,
    pub arch: String,
    pub agent_version: String,
    pub agent_name: String,
    pub uptime_secs: u64,
}

/// GET /api/v1/target-info — system information.
pub async fn target_info(
    State(state): State<Arc<AppState>>,
) -> Result<Json<TargetInfoResponse>, ApiError> {
    Ok(Json(TargetInfoResponse {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        agent_version: env!("CARGO_PKG_VERSION").to_string(),
        agent_name: state.config.agent.name.clone(),
        uptime_secs: state.start_time.elapsed().as_secs(),
    }))
}
