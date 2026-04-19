//! st-target-agent: PLC runtime agent for remote deployment and management.

use clap::Parser;
use std::path::PathBuf;
use tracing::info;

#[derive(Parser)]
#[command(name = "st-target-agent", about = "PLC runtime agent for remote deployment")]
#[command(version)]
struct Args {
    /// Path to agent configuration file.
    #[arg(short, long, default_value = "/etc/st-agent/agent.yaml")]
    config: PathBuf,
}

#[tokio::main]
async fn main() {
    let args = Args::parse();

    // Initialize tracing (stderr for now, file appender later)
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::INFO.into()),
        )
        .init();

    // Load configuration
    let config = if args.config.exists() {
        match st_target_agent::config::load_config(&args.config) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Config error: {e}");
                std::process::exit(1);
            }
        }
    } else {
        info!("No config file at {}, using defaults", args.config.display());
        st_target_agent::config::AgentConfig::default()
    };

    let bind_addr = format!("{}:{}", config.network.bind, config.network.port);
    let dap_port = config.network.dap_port();
    let dap_bind = config.network.bind.clone();
    info!(
        "Starting {} on {} (DAP proxy: {}:{})",
        config.agent.name, bind_addr, dap_bind, dap_port
    );

    // Build application state
    let state = match st_target_agent::server::build_app_state(config, None) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Startup error: {e}");
            std::process::exit(1);
        }
    };

    // Build router
    let router = st_target_agent::server::build_router(state.clone());

    // Bind listener
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .unwrap_or_else(|e| {
            eprintln!("Cannot bind to {bind_addr}: {e}");
            std::process::exit(1);
        });

    info!("Agent ready, listening on {bind_addr}");

    // Auto-start the deployed program if configured
    if state.config.runtime.auto_start {
        let auto_state = state.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let has_program = {
                let store = auto_state.program_store.read().unwrap();
                store.current_program().is_some()
            };
            if has_program {
                info!("Auto-starting deployed program...");
                match auto_start_program(&auto_state).await {
                    Ok(()) => info!("Auto-start: program running"),
                    Err(e) => tracing::warn!("Auto-start failed: {e}"),
                }
            } else {
                info!("No program deployed — skipping auto-start");
            }
        });
    }

    // Spawn DAP proxy on a separate port
    // Find st-cli binary (co-located or in PATH)
    let st_cli_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("st-cli")))
        .filter(|p| p.exists())
        .unwrap_or_else(|| PathBuf::from("st-cli"));
    tokio::spawn(st_target_agent::dap_proxy::run_dap_proxy(
        dap_bind,
        dap_port,
        state.clone(),
        st_cli_path,
    ));

    // Spawn graceful shutdown handler
    let shutdown_state = state.clone();
    let shutdown_signal = async move {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
        info!("Shutdown signal received, stopping...");
        shutdown_state.runtime_manager.shutdown().await;
    };

    // Run server with graceful shutdown
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal)
        .await
        .unwrap_or_else(|e| {
            eprintln!("Server error: {e}");
            std::process::exit(1);
        });

    info!("Agent stopped");
}

/// Auto-start the deployed program (mirrors the /api/v1/program/start logic).
async fn auto_start_program(
    state: &std::sync::Arc<st_target_agent::server::AppState>,
) -> Result<(), String> {
    let (module, program_name) = {
        let store = state.program_store.read().unwrap();
        store.load_module().map_err(|e| format!("{e}"))?
    };
    let program_meta = {
        let store = state.program_store.read().unwrap();
        store.current_program().cloned()
            .ok_or_else(|| "No program deployed".to_string())?
    };

    // Parse cycle_time from the bundled plc-project.yaml
    let cycle_time = {
        let yaml_path = state.program_store.read().unwrap().project_yaml_path();
        let from_project = std::fs::read_to_string(&yaml_path)
            .ok()
            .and_then(|yaml| {
                let cfg = st_comm_api::config::EngineProjectConfig::from_project_yaml(&yaml).ok()?;
                info!("Auto-start: cycle_time from plc-project.yaml: {:?}", cfg.cycle_time);
                cfg.cycle_time
            });
        if from_project.is_none() {
            info!("Auto-start: no cycle_time in plc-project.yaml, using default 10ms");
        }
        Some(from_project.unwrap_or(std::time::Duration::from_millis(10)))
    };

    // Build native FB registry from bundled profiles
    let native_fbs = {
        let profiles_dir = state.program_store.read().unwrap().profiles_dir();
        build_registry(&profiles_dir)
    };
    if let Some(ref reg) = native_fbs {
        info!("Auto-start: native FB registry: {} type(s)", reg.len());
    }

    state.runtime_manager
        .start(module, program_name, cycle_time, program_meta, native_fbs.map(std::sync::Arc::new))
        .await
        .map_err(|e| format!("{e}"))
}

fn build_registry(profiles_dir: &std::path::Path) -> Option<st_comm_api::NativeFbRegistry> {
    if !profiles_dir.is_dir() { return None; }
    let entries = std::fs::read_dir(profiles_dir).ok()?;
    let mut registry = st_comm_api::NativeFbRegistry::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext != "yaml" && ext != "yml" { continue; }
        if let Ok(profile) = st_comm_api::DeviceProfile::from_file(&path) {
            let name = profile.name.clone();
            registry.register(Box::new(st_comm_sim::SimulatedNativeFb::new(&name, profile)));
        }
    }
    if registry.is_empty() { None } else { Some(registry) }
}
