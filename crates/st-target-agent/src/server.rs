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
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};
use std::time::Instant;

/// Shared application state accessible by all handlers.
pub struct AppState {
    pub config: AgentConfig,
    pub program_store: RwLock<ProgramStore>,
    pub runtime_manager: RuntimeManager,
    pub start_time: Instant,
    pub log_level_handle: Option<LogLevelHandle>,
    /// True when a DAP debug session is active (single-session enforcement).
    pub active_debug_session: AtomicBool,
}

/// Build the axum Router with all API routes.
pub fn build_router(state: Arc<AppState>) -> Router {
    Router::new()
        .route("/api/v1/program/upload", post(api::program::upload))
        .route("/api/v1/program/update", post(api::program::update))
        .route("/api/v1/program/info", get(api::program::info))
        .route("/api/v1/program/start", post(api::program::start))
        .route("/api/v1/program/stop", post(api::program::stop))
        .route("/api/v1/program/restart", post(api::program::restart))
        .route("/api/v1/program", delete(api::program::remove))
        .route("/api/v1/monitor/ws", get(api::monitor_ws::ws_upgrade))
        .route("/api/v1/variables/catalog", get(api::variables::catalog))
        .route("/api/v1/variables", get(api::variables::variables))
        .route("/api/v1/variables/force", post(api::variables::force))
        .route("/api/v1/variables/force/{name}", delete(api::variables::unforce))
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

    let state = Arc::new(AppState {
        config,
        program_store: RwLock::new(program_store),
        runtime_manager,
        start_time: Instant::now(),
        log_level_handle,
        active_debug_session: AtomicBool::new(false),
    });

    // Start OPC-UA server if enabled
    #[cfg(feature = "opcua")]
    if state.config.opcua_server.enabled {
        start_opcua_server(&state);
    }

    Ok(state)
}

/// Spawn the OPC-UA server as a background tokio task.
#[cfg(feature = "opcua")]
fn start_opcua_server(state: &Arc<AppState>) {
    let opcua_cfg = &state.config.opcua_server;
    let bind = opcua_cfg
        .bind
        .clone()
        .unwrap_or_else(|| state.config.network.bind.clone());

    // Use the agent's retain directory for PKI storage (persistent across restarts).
    let pki_dir = state.config.storage.retain_dir.join("opcua-pki");

    let server_config = st_opcua_server::OpcuaServerConfig {
        enabled: true,
        port: opcua_cfg.port,
        bind,
        security_policy: opcua_cfg.security_policy.clone(),
        anonymous_access: opcua_cfg.anonymous_access,
        sampling_interval_ms: opcua_cfg.sampling_interval_ms,
        pki_dir: Some(pki_dir),
        ..Default::default()
    };

    let provider = Arc::new(crate::opcua_bridge::AgentDataProvider::new(
        Arc::clone(state),
    ));

    tokio::spawn(async move {
        match st_opcua_server::run_opcua_server(server_config, provider).await {
            Ok(_handle) => {
                // Server is running — handle will be dropped when the
                // tokio runtime shuts down, which cancels the server.
                tracing::info!("OPC-UA server started successfully");
            }
            Err(e) => {
                tracing::error!("OPC-UA server failed to start: {e}");
            }
        }
    });
}
