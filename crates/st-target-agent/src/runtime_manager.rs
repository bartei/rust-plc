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
    /// Engine is paused at a breakpoint by an attached debugger.
    DebugPaused,
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
    /// Attach a debug session to the running engine.
    DebugAttach {
        /// Channel for the engine to send events/responses to the debug session.
        event_tx: std::sync::mpsc::Sender<st_engine::DebugResponse>,
        /// Channel for the debug session to send commands to the engine.
        cmd_rx: std::sync::mpsc::Receiver<st_engine::DebugCommand>,
    },
    /// Detach the debug session (resume normal cycling).
    DebugDetach,
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

    /// Stop the runtime. Works from both Running and DebugPaused states.
    pub async fn stop(&self) -> Result<(), ApiError> {
        let current_status = self.state.read().unwrap().status;
        if current_status != RuntimeStatus::Running
            && current_status != RuntimeStatus::DebugPaused
        {
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

    /// Attach a debug session to the running engine.
    ///
    /// Returns channels for bidirectional communication:
    /// - `cmd_tx`: send DebugCommands to the engine
    /// - `event_rx`: receive DebugResponses from the engine
    ///
    /// The engine keeps running normally until a breakpoint hits or Pause
    /// is sent. The debug session is automatically detached if the command
    /// channel is dropped.
    pub async fn debug_attach(
        &self,
    ) -> Result<
        (
            std::sync::mpsc::Sender<st_engine::DebugCommand>,
            std::sync::mpsc::Receiver<st_engine::DebugResponse>,
        ),
        ApiError,
    > {
        let current_status = self.state.read().unwrap().status;
        if current_status != RuntimeStatus::Running
            && current_status != RuntimeStatus::DebugPaused
        {
            return Err(ApiError::not_running());
        }

        // Create std::sync::mpsc channels (used by the runtime thread which
        // is a plain OS thread, not a tokio task).
        let (event_tx, event_rx) = std::sync::mpsc::channel();
        let (cmd_tx, cmd_rx) = std::sync::mpsc::channel();

        self.cmd_tx
            .send(RuntimeCommand::DebugAttach { event_tx, cmd_rx })
            .await
            .map_err(|_| ApiError::internal("Runtime thread not responding"))?;

        Ok((cmd_tx, event_rx))
    }

    /// Detach the debug session (resume normal cycling).
    pub async fn debug_detach(&self) -> Result<(), ApiError> {
        self.cmd_tx
            .send(RuntimeCommand::DebugDetach)
            .await
            .map_err(|_| ApiError::internal("Runtime thread not responding"))?;
        Ok(())
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

                // Build engine config with retain persistence
                let retain_dir = std::path::PathBuf::from("/var/lib/st-plc/retain");
                let retain_path = retain_dir.join(format!("{program_name}.retain"));
                let engine_config = st_engine::EngineConfig {
                    max_cycles: 0, // unlimited
                    cycle_time,
                    retain: Some(st_engine::RetainConfig {
                        path: retain_path,
                        checkpoint_cycles: 10_000,
                    }),
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

                // Save retained variables before dropping the engine
                if let Err(e) = engine.save_retain() {
                    tracing::warn!("Retain save on stop: {e}");
                }

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
            RuntimeCommand::DebugAttach { .. } | RuntimeCommand::DebugDetach => {
                // Debug commands while idle — ignore (no engine to attach to)
            }
        }
    }
}

enum StopReason {
    Commanded,
    Shutdown,
}

/// Active debug session state (held by the runtime thread).
struct DebugSession {
    event_tx: std::sync::mpsc::Sender<st_engine::DebugResponse>,
    cmd_rx: std::sync::mpsc::Receiver<st_engine::DebugCommand>,
}

/// What to do after handling debug commands.
enum DebugAction {
    /// Resume VM execution (continue, step completed).
    Resume,
    /// Detach debug session, resume normal cycling.
    Detach,
    /// Stop command received while debugging.
    Stop,
    /// Shutdown command received while debugging.
    Shutdown,
}

/// Run the scan cycle loop until stop/shutdown/error.
fn run_cycle_loop(
    engine: &mut st_engine::Engine,
    state: &Arc<RwLock<RuntimeState>>,
    cmd_rx: &mut tokio::sync::mpsc::Receiver<RuntimeCommand>,
    cycle_time: Option<Duration>,
) -> Result<StopReason, String> {
    let mut debug_session: Option<DebugSession> = None;

    loop {
        // Execute one scan cycle
        match engine.run_one_cycle() {
            Ok(cycle_elapsed) => {
                // Normal cycle completed — update stats and sleep
                update_cycle_stats(engine, state);

                if let Some(target) = cycle_time {
                    if let Some(remaining) = target.checked_sub(cycle_elapsed) {
                        std::thread::sleep(remaining);
                    }
                } else {
                    std::thread::sleep(Duration::from_millis(1));
                }
            }
            Err(st_engine::VmError::Halt) => {
                // Debug breakpoint/pause hit — NOT a fatal error.
                if debug_session.is_some() {
                    // Notify the debugger that we stopped
                    let reason = engine.vm().debug_state().pause_reason;
                    if let Some(ref session) = debug_session {
                        let _ = session.event_tx.send(
                            st_engine::DebugResponse::Stopped { reason },
                        );
                    }

                    state.write().unwrap().status = RuntimeStatus::DebugPaused;

                    // Serve debug commands until resume/detach/stop
                    match handle_debug_commands(engine, &mut debug_session, cmd_rx) {
                        DebugAction::Resume => {
                            state.write().unwrap().status = RuntimeStatus::Running;
                            continue;
                        }
                        DebugAction::Detach => {
                            debug_session = None;
                            // Fully clean up debug state: clear breakpoints,
                            // reset step mode, and clear the call stack so the
                            // next scan_cycle starts from a clean state.
                            engine.vm_mut().debug_mut().clear_breakpoints();
                            engine.vm_mut().debug_mut().resume(
                                st_engine::debug::StepMode::Continue, 0,
                            );
                            engine.vm_mut().clear_call_stack();
                            state.write().unwrap().status = RuntimeStatus::Running;
                            tracing::info!("Debug detached — engine resuming normal cycling");
                            continue;
                        }
                        DebugAction::Stop => return Ok(StopReason::Commanded),
                        DebugAction::Shutdown => return Ok(StopReason::Shutdown),
                    }
                } else {
                    // No debug session — clear pause, call stack, and resume
                    engine.vm_mut().debug_mut().clear_breakpoints();
                    engine.vm_mut().debug_mut().resume(
                        st_engine::debug::StepMode::Continue, 0,
                    );
                    engine.vm_mut().clear_call_stack();
                }
            }
            Err(e) => {
                // True runtime error (division by zero, stack overflow, etc.)
                return Err(format!("Runtime error: {e}"));
            }
        }

        // Check for commands (non-blocking)
        match cmd_rx.try_recv() {
            Ok(RuntimeCommand::Stop) => return Ok(StopReason::Commanded),
            Ok(RuntimeCommand::Shutdown) => return Ok(StopReason::Shutdown),
            Ok(RuntimeCommand::DebugAttach { event_tx, cmd_rx: dbg_rx }) => {
                tracing::info!("Debug session attached to running engine");
                debug_session = Some(DebugSession {
                    event_tx,
                    cmd_rx: dbg_rx,
                });
                // Engine keeps cycling — breakpoints will trigger Halt
            }
            Ok(RuntimeCommand::DebugDetach) => {
                if let Some(session) = debug_session.take() {
                    let _ = session.event_tx.send(st_engine::DebugResponse::Detached);
                }
                engine.vm_mut().debug_mut().clear_breakpoints();
                engine.vm_mut().debug_mut().resume(
                    st_engine::debug::StepMode::Continue, 0,
                );
                engine.vm_mut().clear_call_stack();
                tracing::info!("Debug session detached");
            }
            Ok(RuntimeCommand::Start { .. }) => {
                // Ignore start while already running
            }
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => {}
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                return Ok(StopReason::Shutdown);
            }
        }

        // Also check for debug commands between cycles (non-blocking)
        // — handles Pause requests and breakpoint updates while running.
        if debug_session.is_some() {
            let mut should_disconnect = false;
            if let Some(ref session) = debug_session {
                while let Ok(cmd) = session.cmd_rx.try_recv() {
                    match cmd {
                        st_engine::DebugCommand::Pause => {
                            engine.vm_mut().debug_mut().pause();
                        }
                        st_engine::DebugCommand::SetBreakpoints {
                            source_path: _,
                            source,
                            lines,
                        } => {
                            let module = engine.vm().module().clone();
                            engine.vm_mut().debug_mut().clear_breakpoints();
                            let results = engine.vm_mut().debug_mut().set_line_breakpoints(
                                &module, &source, &lines, 0,
                            );
                            let verified = results.iter().map(|r| r.is_some()).collect();
                            let _ = session.event_tx.send(
                                st_engine::DebugResponse::BreakpointsSet { verified },
                            );
                        }
                        st_engine::DebugCommand::Disconnect => {
                            let _ = session.event_tx.send(
                                st_engine::DebugResponse::Detached,
                            );
                            should_disconnect = true;
                            break;
                        }
                        _ => {} // Other commands only valid when paused
                    }
                }
            }
            if should_disconnect {
                debug_session = None;
                engine.vm_mut().debug_mut().clear_breakpoints();
                engine.vm_mut().debug_mut().resume(
                    st_engine::debug::StepMode::Continue, 0,
                );
                engine.vm_mut().clear_call_stack();
                tracing::info!("Debug session disconnected");
            }
        }
    }
}

/// Serve debug commands while the VM is paused at a breakpoint.
/// Blocks until the debugger sends Continue, Step, Disconnect, or the
/// channel closes. Returns what the cycle loop should do next.
fn handle_debug_commands(
    engine: &mut st_engine::Engine,
    debug_session: &mut Option<DebugSession>,
    runtime_cmd_rx: &mut tokio::sync::mpsc::Receiver<RuntimeCommand>,
) -> DebugAction {
    let Some(session) = debug_session.as_ref() else {
        return DebugAction::Detach;
    };

    // Debug pause timeout: 30 minutes. Prevents a forgotten debugger from
    // halting a production system indefinitely.
    let timeout = Duration::from_secs(30 * 60);

    loop {
        // Check for runtime commands (Stop/Shutdown) between debug commands
        match runtime_cmd_rx.try_recv() {
            Ok(RuntimeCommand::Stop) => return DebugAction::Stop,
            Ok(RuntimeCommand::Shutdown) => return DebugAction::Shutdown,
            Ok(RuntimeCommand::DebugDetach) => return DebugAction::Detach,
            _ => {}
        }

        match session.cmd_rx.recv_timeout(timeout) {
            Ok(cmd) => match cmd {
                st_engine::DebugCommand::Continue => {
                    let depth = engine.vm().call_depth();
                    engine.vm_mut().debug_mut().resume(
                        st_engine::debug::StepMode::Continue, depth,
                    );
                    let _ = session.event_tx.send(st_engine::DebugResponse::Resumed);
                    return DebugAction::Resume;
                }
                st_engine::DebugCommand::StepIn => {
                    let depth = engine.vm().call_depth();
                    let offset = engine.vm().stack_frames().first()
                        .map(|f| f.source_offset).unwrap_or(0);
                    engine.vm_mut().debug_mut().resume_with_source(
                        st_engine::debug::StepMode::StepIn, depth, offset,
                    );
                    return DebugAction::Resume;
                }
                st_engine::DebugCommand::StepOver => {
                    let depth = engine.vm().call_depth();
                    let offset = engine.vm().stack_frames().first()
                        .map(|f| f.source_offset).unwrap_or(0);
                    engine.vm_mut().debug_mut().resume_with_source(
                        st_engine::debug::StepMode::StepOver, depth, offset,
                    );
                    return DebugAction::Resume;
                }
                st_engine::DebugCommand::StepOut => {
                    let depth = engine.vm().call_depth();
                    engine.vm_mut().debug_mut().resume(
                        st_engine::debug::StepMode::StepOut, depth,
                    );
                    return DebugAction::Resume;
                }
                st_engine::DebugCommand::GetVariables { scope } => {
                    let vars = match scope {
                        st_engine::DebugScopeKind::Locals => {
                            engine.vm().current_locals_with_fb_fields()
                        }
                        st_engine::DebugScopeKind::Globals => {
                            engine.vm().global_variables()
                        }
                    };
                    let _ = session.event_tx.send(
                        st_engine::DebugResponse::Variables { vars },
                    );
                }
                st_engine::DebugCommand::GetStackTrace => {
                    let frames = engine.vm().stack_frames();
                    let _ = session.event_tx.send(
                        st_engine::DebugResponse::StackTrace { frames },
                    );
                }
                st_engine::DebugCommand::Evaluate { expression } => {
                    // Simple variable lookup
                    let (value, ty) = evaluate_expression(engine, &expression);
                    let _ = session.event_tx.send(
                        st_engine::DebugResponse::EvaluateResult { value, ty },
                    );
                }
                st_engine::DebugCommand::SetBreakpoints { source_path: _, source, lines } => {
                    let module = engine.vm().module().clone();
                    engine.vm_mut().debug_mut().clear_breakpoints();
                    let results = engine.vm_mut().debug_mut().set_line_breakpoints(
                        &module, &source, &lines, 0,
                    );
                    let verified = results.iter().map(|r| r.is_some()).collect();
                    let _ = session.event_tx.send(
                        st_engine::DebugResponse::BreakpointsSet { verified },
                    );
                }
                st_engine::DebugCommand::ClearBreakpoints => {
                    engine.vm_mut().debug_mut().clear_breakpoints();
                }
                st_engine::DebugCommand::Pause => {
                    // Already paused — no-op
                }
                st_engine::DebugCommand::Disconnect => {
                    return DebugAction::Detach;
                }
            },
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                tracing::warn!(
                    "Debug session timeout (30 min) — auto-resuming engine"
                );
                return DebugAction::Detach;
            }
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                tracing::info!("Debug session channel closed — resuming engine");
                return DebugAction::Detach;
            }
        }
    }
}

/// Simple expression evaluation: variable lookup by name.
fn evaluate_expression(engine: &st_engine::Engine, expr: &str) -> (String, String) {
    // Try locals first
    let locals = engine.vm().current_locals();
    if let Some(v) = locals.iter().find(|v| v.name.eq_ignore_ascii_case(expr)) {
        return (v.value.clone(), v.ty.clone());
    }
    // Try globals
    let globals = engine.vm().global_variables();
    if let Some(v) = globals.iter().find(|v| v.name.eq_ignore_ascii_case(expr)) {
        return (v.value.clone(), v.ty.clone());
    }
    // Try dotted FB/struct field path
    if expr.contains('.') {
        if let Some(v) = engine.vm().resolve_fb_field(expr) {
            return (v.value, v.ty);
        }
    }
    ("<unknown>".to_string(), String::new())
}

/// Update shared cycle stats from the engine (factored out for readability).
fn update_cycle_stats(engine: &st_engine::Engine, state: &Arc<RwLock<RuntimeState>>) {
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
