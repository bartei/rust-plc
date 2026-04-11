//! Log query and streaming API endpoints.

use crate::error::ApiError;
use crate::server::AppState;
use axum::extract::{Query, State};
use axum::Json;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Deserialize)]
pub struct LogQuery {
    #[serde(default = "default_limit")]
    pub limit: usize,
    pub level: Option<String>,
    pub since: Option<String>,
}

fn default_limit() -> usize {
    100
}

#[derive(Serialize)]
pub struct LogEntry {
    pub timestamp: String,
    pub level: String,
    pub message: String,
}

#[derive(Serialize)]
pub struct LogsResponse {
    pub entries: Vec<LogEntry>,
    pub total: usize,
}

/// GET /api/v1/logs — query recent log entries.
///
/// Currently returns a placeholder response. Full log file tailing will be
/// implemented when the tracing-appender file output is wired up.
pub async fn query_logs(
    State(_state): State<Arc<AppState>>,
    Query(params): Query<LogQuery>,
) -> Result<Json<LogsResponse>, ApiError> {
    // TODO: Read from actual log files in storage.log_dir
    // For now, return agent startup info as a minimal response
    let entries = vec![LogEntry {
        timestamp: chrono::Utc::now().to_rfc3339(),
        level: "info".to_string(),
        message: "Agent is running".to_string(),
    }];

    let total = entries.len().min(params.limit);

    Ok(Json(LogsResponse {
        entries: entries.into_iter().take(params.limit).collect(),
        total,
    }))
}
