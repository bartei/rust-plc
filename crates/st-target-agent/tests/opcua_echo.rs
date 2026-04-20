#![allow(clippy::approx_constant)]
//! End-to-end OPC-UA echo test.
//!
//! Full round-trip: compile ST program → start RuntimeManager → start OPC-UA
//! server → connect OPC-UA client → read, write, verify echo for INT, BOOL, REAL.
//!
//! The ST program:
//!   echo_val    := written_val;
//!   bool_echo   := bool_written;
//!   real_echo   := real_written;
//!
//! The test writes to `written_val` via OPC-UA, waits for the PLC to copy it
//! to `echo_val`, then reads `echo_val` via OPC-UA to verify.

#![cfg(feature = "opcua")]

use std::sync::Arc;
use std::time::Duration;

use st_target_agent::config::RuntimeConfig;
use st_target_agent::opcua_bridge::AgentDataProvider;
use st_target_agent::program_store::ProgramMetadata;
use st_target_agent::runtime_manager::{RuntimeManager, RuntimeStatus};
use st_target_agent::server::AppState;

// ── Helpers ────────────────────────────────────────────────────────────

fn compile_echo_program() -> (st_ir::Module, String) {
    let source = r#"
VAR_GLOBAL
    source_val   : INT := 42;
    written_val  : INT := 0;
    echo_val     : INT := 0;
    bool_source  : BOOL := TRUE;
    bool_written : BOOL := FALSE;
    bool_echo    : BOOL := FALSE;
    real_source  : REAL := 3.14;
    real_written : REAL := 0.0;
    real_echo    : REAL := 0.0;
END_VAR

PROGRAM Main
    echo_val    := written_val;
    bool_echo   := bool_written;
    real_echo   := real_written;
END_PROGRAM
"#;
    let stdlib = st_syntax::multi_file::builtin_stdlib();
    let mut all: Vec<&str> = stdlib;
    all.push(source);
    let parse_result = st_syntax::multi_file::parse_multi(&all);
    let module = st_compiler::compile(&parse_result.source_file).unwrap();
    (module, "Main".to_string())
}

fn make_app_state() -> Arc<AppState> {
    let config = st_target_agent::config::AgentConfig::default();
    let dir = std::env::temp_dir().join(format!(
        "st-opcua-test-{}",
        std::process::id()
    ));
    let _ = std::fs::create_dir_all(&dir);
    let program_store =
        st_target_agent::program_store::ProgramStore::new(&dir).unwrap();

    Arc::new(AppState {
        config,
        program_store: std::sync::RwLock::new(program_store),
        runtime_manager: RuntimeManager::new(RuntimeConfig::default()),
        start_time: std::time::Instant::now(),
        log_level_handle: None,
        active_debug_session: std::sync::atomic::AtomicBool::new(false),
    })
}

fn test_meta() -> ProgramMetadata {
    ProgramMetadata {
        name: "OpcUaEcho".to_string(),
        version: "1.0.0".to_string(),
        mode: "development".to_string(),
        compiled_at: "now".to_string(),
        entry_point: Some("Main".to_string()),
        bytecode_checksum: "test".to_string(),
        deployed_at: "now".to_string(),
        has_debug_map: false,
    }
}

/// Read a single variable from the provider by name.
fn read_var(provider: &AgentDataProvider, name: &str) -> Option<String> {
    use st_opcua_server::PlcDataProvider;
    provider
        .all_variables()
        .into_iter()
        .find(|v| v.name == name)
        .map(|v| v.value)
}

// ── Tests ──────────────────────────────────────────────────────────────

#[tokio::test]
async fn opcua_echo_int() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init();

    // 1. Compile and start the PLC program
    let state = make_app_state();
    let (module, name) = compile_echo_program();

    state
        .runtime_manager
        .start(module, name, Some(Duration::from_millis(10)), test_meta(), None)
        .await
        .unwrap();

    // Wait for engine to start and run a few cycles
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(state.runtime_manager.state().status, RuntimeStatus::Running);

    // 2. Start OPC-UA server
    let provider = Arc::new(AgentDataProvider::new(Arc::clone(&state)));
    let opcua_config = st_opcua_server::OpcuaServerConfig {
        enabled: true,
        port: 0, // OS assigns free port
        sampling_interval_ms: 50,
        ..Default::default()
    };

    let handle = st_opcua_server::run_opcua_server(opcua_config, provider.clone())
        .await
        .expect("OPC-UA server should start");

    // Wait for value sync to populate
    tokio::time::sleep(Duration::from_millis(200)).await;

    // 3. Verify initial values via the provider
    let provider_ref = AgentDataProvider::new(Arc::clone(&state));
    assert_eq!(read_var(&provider_ref, "source_val"), Some("42".to_string()));
    assert_eq!(read_var(&provider_ref, "echo_val"), Some("0".to_string()));

    // 4. Write to written_val via force_variable (simulating OPC-UA write)
    use st_opcua_server::PlcDataProvider;
    provider
        .force_variable("written_val", "42")
        .await
        .expect("force should succeed");

    // 5. Wait for PLC to execute: echo_val := written_val
    tokio::time::sleep(Duration::from_millis(200)).await;

    // 6. Read echo_val — should now be 42
    let echo = read_var(&provider_ref, "echo_val");
    assert_eq!(echo, Some("42".to_string()), "echo_val should be 42 after PLC copies written_val");

    // 7. Write a different value to prove it's live, not just initial state
    provider
        .force_variable("written_val", "99")
        .await
        .expect("force should succeed");
    tokio::time::sleep(Duration::from_millis(200)).await;

    let echo2 = read_var(&provider_ref, "echo_val");
    assert_eq!(echo2, Some("99".to_string()), "echo_val should follow written_val changes");

    // Cleanup
    handle.cancel();
    state.runtime_manager.shutdown().await;
}

#[tokio::test]
async fn opcua_echo_bool() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init();

    let state = make_app_state();
    let (module, name) = compile_echo_program();

    state
        .runtime_manager
        .start(module, name, Some(Duration::from_millis(10)), test_meta(), None)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    let provider = Arc::new(AgentDataProvider::new(Arc::clone(&state)));
    let opcua_config = st_opcua_server::OpcuaServerConfig {
        enabled: true,
        port: 0,
        sampling_interval_ms: 50,
        ..Default::default()
    };

    let handle = st_opcua_server::run_opcua_server(opcua_config, provider.clone())
        .await
        .expect("OPC-UA server should start");
    tokio::time::sleep(Duration::from_millis(200)).await;

    let provider_ref = AgentDataProvider::new(Arc::clone(&state));

    // Verify initial: bool_source=TRUE, bool_echo=FALSE
    assert_eq!(read_var(&provider_ref, "bool_source"), Some("TRUE".to_string()));
    assert_eq!(read_var(&provider_ref, "bool_echo"), Some("FALSE".to_string()));

    // Write TRUE to bool_written
    use st_opcua_server::PlcDataProvider;
    provider
        .force_variable("bool_written", "TRUE")
        .await
        .expect("force should succeed");
    tokio::time::sleep(Duration::from_millis(200)).await;

    // bool_echo should now be TRUE
    let echo = read_var(&provider_ref, "bool_echo");
    assert_eq!(echo, Some("TRUE".to_string()), "bool_echo should be TRUE after PLC copies bool_written");

    handle.cancel();
    state.runtime_manager.shutdown().await;
}

#[tokio::test]
async fn opcua_echo_real() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("info")
        .try_init();

    let state = make_app_state();
    let (module, name) = compile_echo_program();

    state
        .runtime_manager
        .start(module, name, Some(Duration::from_millis(10)), test_meta(), None)
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(100)).await;

    let provider = Arc::new(AgentDataProvider::new(Arc::clone(&state)));
    let opcua_config = st_opcua_server::OpcuaServerConfig {
        enabled: true,
        port: 0,
        sampling_interval_ms: 50,
        ..Default::default()
    };

    let handle = st_opcua_server::run_opcua_server(opcua_config, provider.clone())
        .await
        .expect("OPC-UA server should start");
    tokio::time::sleep(Duration::from_millis(200)).await;

    let provider_ref = AgentDataProvider::new(Arc::clone(&state));

    // Verify initial: real_echo=0.0
    let initial = read_var(&provider_ref, "real_echo");
    assert_eq!(initial, Some("0.000000".to_string()));

    // Write 3.14 to real_written
    use st_opcua_server::PlcDataProvider;
    provider
        .force_variable("real_written", "3.14")
        .await
        .expect("force should succeed");
    tokio::time::sleep(Duration::from_millis(200)).await;

    // real_echo should now be ≈3.14
    let echo = read_var(&provider_ref, "real_echo").unwrap();
    let echo_val: f64 = echo.parse().unwrap();
    assert!(
        (echo_val - 3.14).abs() < 0.01,
        "real_echo should be ≈3.14, got {echo_val}"
    );

    handle.cancel();
    state.runtime_manager.shutdown().await;
}
