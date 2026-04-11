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
    let state = match st_target_agent::server::build_app_state(config) {
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
