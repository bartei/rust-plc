//! Variable monitoring API endpoints (HTTP-based monitor panel).

use crate::error::ApiError;
use crate::runtime_manager::{CatalogEntry, ForceRequest, RuntimeStatus, VariableValue};
use crate::server::AppState;
use axum::extract::{Path, Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

// ── GET /api/v1/variables/catalog ───────────────────────────────────

#[derive(Serialize)]
pub struct CatalogResponse {
    pub variables: Vec<CatalogEntry>,
}

/// Returns the list of all monitorable variables (names + types).
/// Empty when no program is running.
pub async fn catalog(
    State(state): State<Arc<AppState>>,
) -> Json<CatalogResponse> {
    let catalog = state.runtime_manager.variable_catalog();
    Json(CatalogResponse { variables: catalog })
}

// ── GET /api/v1/variables?watch=Main.counter,Main.x ────────────────

#[derive(Deserialize)]
pub struct WatchQuery {
    /// Comma-separated variable names to watch.
    #[serde(default)]
    pub watch: String,
}

#[derive(Serialize)]
pub struct VariablesResponse {
    pub variables: Vec<VariableValue>,
}

/// Returns current values of watched variables (filtered from the full
/// snapshot). If no `watch` parameter, returns all variables.
pub async fn variables(
    State(state): State<Arc<AppState>>,
    Query(query): Query<WatchQuery>,
) -> Json<VariablesResponse> {
    let all = state.runtime_manager.all_variables();
    let values = if query.watch.is_empty() {
        all
    } else {
        let names: Vec<String> = query.watch.split(',').map(|s| s.trim().to_string()).collect();
        all.into_iter()
            .filter(|v| {
                names.iter().any(|n| {
                    // Exact match
                    n.eq_ignore_ascii_case(&v.name)
                    // Prefix match for compound types: "Main.arr" matches "Main.arr[1]"
                    || v.name.to_uppercase().starts_with(&format!("{}.", n.to_uppercase()))
                    || v.name.to_uppercase().starts_with(&format!("{}[", n.to_uppercase()))
                })
            })
            .collect()
    };
    Json(VariablesResponse { variables: values })
}

// ── POST /api/v1/variables/force ────────────────────────────────────

#[derive(Serialize)]
pub struct ForceResponse {
    pub result: String,
}

/// Force a variable to a constant value.
pub async fn force(
    State(state): State<Arc<AppState>>,
    Json(body): Json<ForceRequest>,
) -> Result<Json<ForceResponse>, ApiError> {
    let current_status = state.runtime_manager.state().status;
    if current_status != RuntimeStatus::Running
        && current_status != RuntimeStatus::DebugPaused
    {
        return Err(ApiError::not_running());
    }
    let result = state
        .runtime_manager
        .force_variable(body.name, body.value)
        .await?;
    Ok(Json(ForceResponse { result }))
}

// ── DELETE /api/v1/variables/force/:name ─────────────────────────────

/// Remove a force override from a variable.
pub async fn unforce(
    State(state): State<Arc<AppState>>,
    Path(name): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let current_status = state.runtime_manager.state().status;
    if current_status != RuntimeStatus::Running
        && current_status != RuntimeStatus::DebugPaused
    {
        return Err(ApiError::not_running());
    }
    state.runtime_manager.unforce_variable(name).await?;
    Ok(Json(serde_json::json!({ "ok": true })))
}
