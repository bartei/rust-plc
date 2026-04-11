//! HTTP server setup with axum.

use crate::api;
use crate::auth;
use crate::config::AgentConfig;
use crate::logging::LogLevelHandle;
use crate::program_store::ProgramStore;
use crate::runtime_manager::RuntimeManager;
use axum::middleware;
use axum::routing::{delete, get, post, put};
use axum::Router;
use std::sync::{Arc, RwLock};
use std::time::Instant;

/// Shared application state accessible by all handlers.
pub struct AppState {
    pub config: AgentConfig,
    pub program_store: RwLock<ProgramStore>,
    pub runtime_manager: RuntimeManager,
    pub start_time: Instant,
    pub log_level_handle: Option<LogLevelHandle>,
}

/// Build the axum Router with all API routes.
pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/v1/program/upload", post(api::program::upload))
        .route("/api/v1/program/info", get(api::program::info))
        .route("/api/v1/program/start", post(api::program::start))
        .route("/api/v1/program/stop", post(api::program::stop))
        .route("/api/v1/program/restart", post(api::program::restart))
        .route("/api/v1/program", delete(api::program::remove))
        .route("/api/v1/status", get(api::status::status))
        .route("/api/v1/health", get(api::status::health))
        .route("/api/v1/target-info", get(api::status::target_info))
        .route("/api/v1/logs", get(api::logs::query_logs))
        .route("/api/v1/log-level", get(api::logs::get_log_level))
        .route("/api/v1/log-level", put(api::logs::set_log_level))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::auth_middleware,
        ))
        .with_state(state)
}

/// Create the AppState. Pass `log_level_handle` from `init_logging()` for
/// runtime log level control, or `None` for tests.
pub fn build_app_state(
    config: AgentConfig,
    log_level_handle: Option<LogLevelHandle>,
) -> Result<Arc<AppState>, String> {
    let program_store = ProgramStore::new(&config.storage.program_dir)?;
    let runtime_manager = RuntimeManager::new(config.runtime.clone());

    Ok(Arc::new(AppState {
        config,
        program_store: RwLock::new(program_store),
        runtime_manager,
        start_time: Instant::now(),
        log_level_handle,
    }))
}
