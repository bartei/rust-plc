//! st-runtime: Unified PLC runtime binary for target deployment.
//!
//! Single statically-linked binary combining agent + debugger + compiler.
//! Deployed to target devices via `st-cli target install`.
//!
//! Subcommands:
//!   agent   — Run as HTTP agent daemon (systemd starts this)
//!   debug   — DAP debug server (agent spawns this internally for remote debug)
//!   run     — Direct program execution (for testing)
//!   check   — Syntax and semantic analysis
//!   version — Print version info

use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "st-runtime")]
#[command(about = "PLC runtime for IEC 61131-3 Structured Text")]
#[command(version)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run as agent daemon (HTTP API + DAP proxy)
    Agent {
        /// Path to agent configuration file
        #[arg(short, long, default_value = "/etc/st-plc/agent.yaml")]
        config: PathBuf,
    },

    /// Start DAP debug server (stdin/stdout, spawned by the agent)
    Debug {
        /// Path to .st file or project directory
        path: String,
    },

    /// Compile and execute a program
    Run {
        /// Path to .st file or project directory
        path: Option<String>,
        /// Number of scan cycles to execute (0 = unlimited)
        #[arg(short, long, default_value = "1")]
        cycles: u64,
    },

    /// Parse and analyze, report diagnostics
    Check {
        /// Path to .st file or project directory
        path: Option<String>,
    },

    /// Print version information
    Version,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Agent { config } => {
            run_agent(config).await;
        }
        Commands::Debug { path } => {
            run_debug(&path);
        }
        Commands::Run { path, cycles } => {
            run_program(path.as_deref(), cycles);
        }
        Commands::Check { path } => {
            run_check(path.as_deref());
        }
        Commands::Version => {
            println!("st-runtime {}", env!("CARGO_PKG_VERSION"));
            println!("Target: {}/{}", std::env::consts::OS, std::env::consts::ARCH);
        }
    }
}

/// Run the agent daemon (HTTP API server + DAP proxy).
async fn run_agent(config_path: PathBuf) {
    // Load config FIRST so we can read the log level before initializing logging
    let config = if config_path.exists() {
        match st_target_agent::config::load_config(&config_path) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("Config error: {e}");
                std::process::exit(1);
            }
        }
    } else {
        eprintln!("No config at {}, using defaults", config_path.display());
        st_target_agent::config::AgentConfig::default()
    };

    // SAFETY: Acquire singleton lock BEFORE any I/O binding or runtime startup.
    // Two instances controlling the same physical I/O can cause machinery damage.
    let pid_path = std::path::Path::new("/run/st-runtime/st-runtime.pid");
    let _singleton = match st_target_agent::singleton::SingletonGuard::acquire(pid_path) {
        Ok(guard) => Some(guard),
        Err(st_target_agent::singleton::SingletonError::AlreadyRunning { pid }) => {
            eprintln!("FATAL: Another st-runtime instance is already running (PID {pid}).");
            eprintln!("Two instances controlling the same I/O can cause physical harm.");
            eprintln!("Stop the existing instance first: sudo systemctl stop st-runtime");
            std::process::exit(1);
        }
        Err(st_target_agent::singleton::SingletonError::IoError(_)) => {
            // PID lock may fail in dev environments (no /run/ directory).
            // Port binding below provides fallback singleton enforcement.
            None
        }
    };

    // Initialize logging: journald on systemd Linux, stderr fallback otherwise.
    // The log level from agent.yaml is used as the initial filter.
    let log_handle = st_target_agent::logging::init_logging(&config.logging.level);

    let bind_addr = format!("{}:{}", config.network.bind, config.network.port);
    let dap_port = config.network.dap_port();
    let dap_bind = config.network.bind.clone();
    tracing::info!(
        "Starting {} on {} (DAP: {}:{})",
        config.agent.name, bind_addr, dap_bind, dap_port
    );

    let state = match st_target_agent::server::build_app_state(config, Some(log_handle)) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Startup error: {e}");
            std::process::exit(1);
        }
    };

    let router = st_target_agent::server::build_router(state.clone());

    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .unwrap_or_else(|e| {
            eprintln!("Cannot bind to {bind_addr}: {e}");
            std::process::exit(1);
        });

    // Spawn DAP proxy — uses current_exe() to spawn self with "debug" subcommand
    let dap_state = state.clone();
    let st_cli_path = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("st-runtime"));
    tokio::spawn(st_target_agent::dap_proxy::run_dap_proxy(
        dap_bind,
        dap_port,
        dap_state,
        st_cli_path,
    ));

    tracing::info!("Agent ready, listening on {bind_addr}");

    // Auto-start the deployed program if configured
    if state.config.runtime.auto_start {
        let auto_state = state.clone();
        tokio::spawn(async move {
            // Brief delay to let the server fully initialize
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            let has_program = {
                let store = auto_state.program_store.read().unwrap();
                store.current_program().is_some()
            }; // lock released here
            if has_program {
                tracing::info!("Auto-starting deployed program...");
                match auto_start_program(&auto_state).await {
                    Ok(()) => tracing::info!("Auto-start: program running"),
                    Err(e) => tracing::warn!("Auto-start failed: {e}"),
                }
            } else {
                tracing::info!("No program deployed — skipping auto-start");
            }
        });
    }

    let shutdown_state = state.clone();
    let shutdown_signal = async move {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
        tracing::info!("Shutdown signal received");
        shutdown_state.runtime_manager.shutdown().await;
    };

    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown_signal)
        .await
        .unwrap_or_else(|e| {
            eprintln!("Server error: {e}");
            std::process::exit(1);
        });
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
    let cycle_time = Some(std::time::Duration::from_millis(10));
    state.runtime_manager
        .start(module, program_name, cycle_time, program_meta)
        .await
        .map_err(|e| format!("{e}"))
}

/// Run the DAP debug server on stdin/stdout.
fn run_debug(source_path: &str) {
    st_dap::run_dap(std::io::stdin(), std::io::stdout(), source_path);
}

/// Compile and execute a program.
fn run_program(path: Option<&str>, cycles: u64) {
    let target = path.map(std::path::Path::new);

    let project = match st_syntax::project::discover_project(target) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Project error: {e}");
            std::process::exit(1);
        }
    };

    let sources = match st_syntax::project::load_project_sources(&project) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    };

    let stdlib = st_syntax::multi_file::builtin_stdlib();
    let mut all: Vec<&str> = stdlib;
    let source_strs: Vec<&str> = sources.iter().map(|(_, s)| s.as_str()).collect();
    all.extend(&source_strs);
    let parse_result = st_syntax::multi_file::parse_multi(&all);

    if !parse_result.errors.is_empty() {
        for err in &parse_result.errors {
            eprintln!("error: {}", err.message);
        }
        std::process::exit(1);
    }

    let analysis = st_semantics::analyze::analyze(&parse_result.source_file);
    let has_errors = analysis.diagnostics.iter().any(|d| {
        d.severity == st_semantics::diagnostic::Severity::Error
    });
    if has_errors {
        for d in &analysis.diagnostics {
            if d.severity == st_semantics::diagnostic::Severity::Error {
                eprintln!("error: {}", d.message);
            }
        }
        std::process::exit(1);
    }

    let module = match st_compiler::compile(&parse_result.source_file) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Compilation error: {e}");
            std::process::exit(1);
        }
    };

    let program_name = project.entry_point.unwrap_or_else(|| {
        module
            .functions
            .iter()
            .find(|f| f.kind == st_ir::PouKind::Program)
            .map(|f| f.name.clone())
            .unwrap_or_else(|| {
                eprintln!("No PROGRAM found");
                std::process::exit(1);
            })
    });

    let config = st_engine::EngineConfig {
        max_cycles: cycles,
        ..Default::default()
    };
    let mut engine = st_engine::Engine::new(module, program_name, config);
    match engine.run() {
        Ok(()) => {
            let stats = engine.stats();
            eprintln!(
                "Executed {} cycle(s) in {:?} ({} instructions)",
                stats.cycle_count,
                stats.total_time,
                engine.vm().instruction_count(),
            );
        }
        Err(e) => {
            eprintln!("Runtime error: {e}");
            std::process::exit(1);
        }
    }
}

/// Parse and analyze, report diagnostics.
fn run_check(path: Option<&str>) {
    let target = path.map(std::path::Path::new);

    let project = match st_syntax::project::discover_project(target) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("Project error: {e}");
            std::process::exit(1);
        }
    };

    let sources = match st_syntax::project::load_project_sources(&project) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    };

    let stdlib = st_syntax::multi_file::builtin_stdlib();
    let mut all: Vec<&str> = stdlib;
    let source_strs: Vec<&str> = sources.iter().map(|(_, s)| s.as_str()).collect();
    all.extend(&source_strs);
    let parse_result = st_syntax::multi_file::parse_multi(&all);

    let analysis = st_semantics::analyze::analyze(&parse_result.source_file);

    let mut error_count = 0;
    for err in &parse_result.errors {
        eprintln!("error: {}", err.message);
        error_count += 1;
    }
    for d in &analysis.diagnostics {
        if d.severity == st_semantics::diagnostic::Severity::Error {
            eprintln!("error: {}", d.message);
            error_count += 1;
        }
    }

    if error_count > 0 {
        eprintln!("{error_count} error(s) found");
        std::process::exit(1);
    }

    eprintln!("Project '{}': OK ({} file(s))", project.name, project.source_files.len());
}
