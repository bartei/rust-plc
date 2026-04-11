//! PLC runtime lifecycle management with a dedicated engine thread.

use crate::config::RuntimeConfig;
use crate::error::ApiError;
use crate::program_store::ProgramMetadata;
use serde::Serialize;
use std::sync::{Arc, RwLock};
use std::time::Duration;

/// Runtime status (state machine).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeStatus {
    Idle,
    Starting,
    Running,
    Stopping,
    Error,
}

/// Snapshot of cycle statistics for API responses.
#[derive(Debug, Clone, Default, Serialize)]
pub struct CycleStatsSnapshot {
    pub cycle_count: u64,
    pub last_cycle_time_us: u64,
    pub min_cycle_time_us: u64,
    pub max_cycle_time_us: u64,
    pub avg_cycle_time_us: u64,
}

/// Shared runtime state visible to the HTTP API.
#[derive(Debug, Clone, Serialize)]
pub struct RuntimeState {
    pub status: RuntimeStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cycle_stats: Option<CycleStatsSnapshot>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub program: Option<ProgramMetadata>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    pub restart_count: u32,
}

impl Default for RuntimeState {
    fn default() -> Self {
        RuntimeState {
            status: RuntimeStatus::Idle,
            cycle_stats: None,
            program: None,
            error: None,
            started_at: None,
            restart_count: 0,
        }
    }
}

/// Command sent to the runtime thread.
pub enum RuntimeCommand {
    Start(Box<StartParams>),
    Stop,
    Shutdown,
}

pub struct StartParams {
    pub module: st_ir::Module,
    pub program_name: String,
    pub cycle_time: Option<Duration>,
    pub program_meta: ProgramMetadata,
}

/// Manages the PLC runtime lifecycle in a dedicated thread.
pub struct RuntimeManager {
    state: Arc<RwLock<RuntimeState>>,
    cmd_tx: tokio::sync::mpsc::Sender<RuntimeCommand>,
    _config: RuntimeConfig,
}

impl RuntimeManager {
    /// Create a new RuntimeManager and spawn the runtime thread.
    pub fn new(config: RuntimeConfig) -> Self {
        let state = Arc::new(RwLock::new(RuntimeState::default()));
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel(16);

        let thread_state = Arc::clone(&state);
        std::thread::Builder::new()
            .name("plc-runtime".to_string())
            .spawn(move || runtime_thread(thread_state, cmd_rx))
            .expect("Failed to spawn runtime thread");

        RuntimeManager {
            state,
            cmd_tx,
            _config: config,
        }
    }

    /// Get the current runtime state.
    pub fn state(&self) -> RuntimeState {
        self.state.read().unwrap().clone()
    }

    /// Get a reference to the shared state lock (for watchdog).
    pub fn shared_state(&self) -> Arc<RwLock<RuntimeState>> {
        Arc::clone(&self.state)
    }

    /// Start the runtime with the given module.
    pub async fn start(
        &self,
        module: st_ir::Module,
        program_name: String,
        cycle_time: Option<Duration>,
        program_meta: ProgramMetadata,
    ) -> Result<(), ApiError> {
        let current_status = self.state.read().unwrap().status;
        if current_status == RuntimeStatus::Running || current_status == RuntimeStatus::Starting {
            return Err(ApiError::already_running());
        }

        self.cmd_tx
            .send(RuntimeCommand::Start(Box::new(StartParams {
                module,
                program_name,
                cycle_time,
                program_meta,
            })))
            .await
            .map_err(|_| ApiError::internal("Runtime thread not responding"))?;

        Ok(())
    }

    /// Stop the runtime.
    pub async fn stop(&self) -> Result<(), ApiError> {
        let current_status = self.state.read().unwrap().status;
        if current_status != RuntimeStatus::Running {
            return Err(ApiError::not_running());
        }

        self.cmd_tx
            .send(RuntimeCommand::Stop)
            .await
            .map_err(|_| ApiError::internal("Runtime thread not responding"))?;

        Ok(())
    }

    /// Send shutdown command (for graceful exit).
    pub async fn shutdown(&self) {
        let _ = self.cmd_tx.send(RuntimeCommand::Shutdown).await;
    }

    /// Get the command sender (for watchdog restart).
    pub fn cmd_sender(&self) -> tokio::sync::mpsc::Sender<RuntimeCommand> {
        self.cmd_tx.clone()
    }
}

/// The runtime thread loop. Owns the Engine and executes scan cycles.
fn runtime_thread(
    state: Arc<RwLock<RuntimeState>>,
    mut cmd_rx: tokio::sync::mpsc::Receiver<RuntimeCommand>,
) {
    while let Some(cmd) = cmd_rx.blocking_recv() {
        match cmd {
            RuntimeCommand::Shutdown => break,
            RuntimeCommand::Stop => {
                // Already idle, ignore
            }
            RuntimeCommand::Start(params) => {
                let StartParams { module, program_name, cycle_time, program_meta } = *params;
                // Update state to Starting
                {
                    let mut s = state.write().unwrap();
                    s.status = RuntimeStatus::Starting;
                    s.program = Some(program_meta);
                    s.error = None;
                    s.started_at = Some(chrono::Utc::now().to_rfc3339());
                }

                // Build engine config
                let engine_config = st_engine::EngineConfig {
                    max_cycles: 0, // unlimited
                    cycle_time,
                    ..Default::default()
                };

                // Construct engine inside this thread (avoids Send issues)
                let mut engine =
                    st_engine::Engine::new(module, program_name, engine_config);

                // Set state to Running
                {
                    let mut s = state.write().unwrap();
                    s.status = RuntimeStatus::Running;
                }

                // Scan cycle loop
                let run_result = run_cycle_loop(&mut engine, &state, &mut cmd_rx, cycle_time);

                // Update state based on result
                {
                    let mut s = state.write().unwrap();
                    match run_result {
                        Ok(StopReason::Commanded) => {
                            s.status = RuntimeStatus::Idle;
                        }
                        Ok(StopReason::Shutdown) => {
                            s.status = RuntimeStatus::Idle;
                            break; // exit the thread
                        }
                        Err(e) => {
                            s.status = RuntimeStatus::Error;
                            s.error = Some(e);
                        }
                    }
                }
            }
        }
    }
}

enum StopReason {
    Commanded,
    Shutdown,
}

/// Run the scan cycle loop until stop/shutdown/error.
fn run_cycle_loop(
    engine: &mut st_engine::Engine,
    state: &Arc<RwLock<RuntimeState>>,
    cmd_rx: &mut tokio::sync::mpsc::Receiver<RuntimeCommand>,
    cycle_time: Option<Duration>,
) -> Result<StopReason, String> {
    loop {
        // Execute one scan cycle
        let cycle_elapsed = engine
            .run_one_cycle()
            .map_err(|e| format!("Runtime error: {e}"))?;

        // Update shared state with latest stats
        {
            let stats = engine.stats();
            let mut s = state.write().unwrap();
            s.cycle_stats = Some(CycleStatsSnapshot {
                cycle_count: stats.cycle_count,
                last_cycle_time_us: stats.last_cycle_time.as_micros() as u64,
                min_cycle_time_us: stats.min_cycle_time.as_micros() as u64,
                max_cycle_time_us: stats.max_cycle_time.as_micros() as u64,
                avg_cycle_time_us: stats.avg_cycle_time().as_micros() as u64,
            });
        }

        // Sleep for remaining cycle time
        if let Some(target) = cycle_time {
            if let Some(remaining) = target.checked_sub(cycle_elapsed) {
                std::thread::sleep(remaining);
            }
        } else {
            // No cycle time configured — yield briefly to allow commands through
            std::thread::sleep(Duration::from_millis(1));
        }

        // Check for commands (non-blocking)
        match cmd_rx.try_recv() {
            Ok(RuntimeCommand::Stop) => return Ok(StopReason::Commanded),
            Ok(RuntimeCommand::Shutdown) => return Ok(StopReason::Shutdown),
            Ok(RuntimeCommand::Start { .. }) => {
                // Ignore start while already running
            }
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {
                // No command, continue cycling
            }
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                return Ok(StopReason::Shutdown);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compile_test_module() -> (st_ir::Module, String) {
        let source = "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := x + 1;\nEND_PROGRAM\n";
        let stdlib = st_syntax::multi_file::builtin_stdlib();
        let mut all: Vec<&str> = stdlib;
        all.push(source);
        let parse_result = st_syntax::multi_file::parse_multi(&all);
        let module = st_compiler::compile(&parse_result.source_file).unwrap();
        (module, "Main".to_string())
    }

    #[tokio::test]
    async fn initial_state_is_idle() {
        let mgr = RuntimeManager::new(RuntimeConfig::default());
        let state = mgr.state();
        assert_eq!(state.status, RuntimeStatus::Idle);
        assert!(state.cycle_stats.is_none());
        mgr.shutdown().await;
    }

    #[tokio::test]
    async fn start_transitions_to_running() {
        let mgr = RuntimeManager::new(RuntimeConfig::default());
        let (module, name) = compile_test_module();
        let meta = ProgramMetadata {
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            mode: "development".to_string(),
            compiled_at: "now".to_string(),
            entry_point: Some("Main".to_string()),
            bytecode_checksum: "abc".to_string(),
            deployed_at: "now".to_string(),
            has_debug_map: false,
        };

        mgr.start(module, name, Some(Duration::from_millis(10)), meta)
            .await
            .unwrap();

        // Give the runtime thread time to start
        tokio::time::sleep(Duration::from_millis(50)).await;

        let state = mgr.state();
        assert_eq!(state.status, RuntimeStatus::Running);
        assert!(state.cycle_stats.is_some());
        assert!(state.cycle_stats.as_ref().unwrap().cycle_count > 0);

        mgr.stop().await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;

        let state = mgr.state();
        assert_eq!(state.status, RuntimeStatus::Idle);

        mgr.shutdown().await;
    }

    #[tokio::test]
    async fn stop_when_idle_errors() {
        let mgr = RuntimeManager::new(RuntimeConfig::default());
        let result = mgr.stop().await;
        assert!(result.is_err());
        mgr.shutdown().await;
    }

    #[tokio::test]
    async fn double_start_errors() {
        let mgr = RuntimeManager::new(RuntimeConfig::default());
        let (module, name) = compile_test_module();
        let meta = ProgramMetadata {
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            mode: "development".to_string(),
            compiled_at: "now".to_string(),
            entry_point: Some("Main".to_string()),
            bytecode_checksum: "abc".to_string(),
            deployed_at: "now".to_string(),
            has_debug_map: false,
        };

        mgr.start(module.clone(), name.clone(), Some(Duration::from_millis(10)), meta.clone())
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;

        let result = mgr.start(module, name, Some(Duration::from_millis(10)), meta).await;
        assert!(result.is_err());

        mgr.stop().await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        mgr.shutdown().await;
    }

    #[tokio::test]
    async fn cycle_stats_advance() {
        let mgr = RuntimeManager::new(RuntimeConfig::default());
        let (module, name) = compile_test_module();
        let meta = ProgramMetadata {
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            mode: "development".to_string(),
            compiled_at: "now".to_string(),
            entry_point: Some("Main".to_string()),
            bytecode_checksum: "abc".to_string(),
            deployed_at: "now".to_string(),
            has_debug_map: false,
        };

        mgr.start(module, name, Some(Duration::from_millis(5)), meta)
            .await
            .unwrap();

        tokio::time::sleep(Duration::from_millis(100)).await;
        let stats1 = mgr.state().cycle_stats.unwrap().cycle_count;

        tokio::time::sleep(Duration::from_millis(100)).await;
        let stats2 = mgr.state().cycle_stats.unwrap().cycle_count;

        assert!(stats2 > stats1, "Cycle count should advance: {stats1} -> {stats2}");

        mgr.stop().await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        mgr.shutdown().await;
    }
}
