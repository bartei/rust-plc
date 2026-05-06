//! PLC runtime lifecycle management with a dedicated engine thread.

use crate::config::RuntimeConfig;
use crate::error::ApiError;
use crate::program_store::ProgramMetadata;
use serde::{Deserialize, Serialize};
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
    pub target_cycle_us: u64,
    pub last_period_us: u64,
    pub min_period_us: u64,
    pub max_period_us: u64,
    pub jitter_max_us: u64,
}

// ── Monitor types (HTTP variable monitoring) ────────────────────────

/// A variable in the monitorable catalog (schema only, no values).
/// Carries the IEC 61131-3 RETAIN / PERSISTENT qualifiers so the monitor
/// UI can render badges next to retained variables.
#[derive(Debug, Clone, Default, Serialize)]
pub struct CatalogEntry {
    pub name: String,
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub retain: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub persistent: bool,
}

/// A watched variable's current value.
#[derive(Debug, Clone, Default, Serialize)]
pub struct VariableValue {
    pub name: String,
    pub value: String,
    #[serde(rename = "type")]
    pub ty: String,
    pub forced: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub retain: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub persistent: bool,
}

#[inline]
fn is_false(b: &bool) -> bool { !*b }

/// Body for POST /api/v1/variables/force.
#[derive(Debug, Deserialize)]
pub struct ForceRequest {
    pub name: String,
    pub value: String,
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

    // ── Monitor snapshot (written by engine thread, read by WS/HTTP) ──
    /// Variable catalog (names + types). Set once when the engine starts.
    #[serde(skip)]
    pub variable_catalog: Vec<CatalogEntry>,
    /// ALL monitorable variable values. Updated every cycle.
    #[serde(skip)]
    pub all_variables: Vec<VariableValue>,
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
            variable_catalog: Vec::new(),
            all_variables: Vec::new(),
        }
    }
}

/// Summary of an online change applied to the running engine.
/// Returned from `online_change` so the HTTP API can report what
/// the engine did without re-deriving it from `ChangeAnalysis`.
#[derive(Debug, Clone, Serialize)]
pub struct OnlineChangeReport {
    pub preserved_vars: Vec<String>,
    pub new_vars: Vec<String>,
    pub removed_vars: Vec<String>,
}

/// Command sent to the runtime thread.
pub enum RuntimeCommand {
    Start(Box<StartParams>),
    Stop,
    Shutdown,
    /// Apply an online change with the given new module. Engine analyzes
    /// compatibility and either swaps state or rejects.
    OnlineChange {
        new_module: Box<st_ir::Module>,
        new_program_meta: ProgramMetadata,
        reply: tokio::sync::oneshot::Sender<Result<OnlineChangeReport, String>>,
    },
    /// Attach a debug session to the running engine.
    DebugAttach {
        /// Channel for the engine to send events/responses to the debug session.
        event_tx: std::sync::mpsc::Sender<st_engine::DebugResponse>,
        /// Channel for the debug session to send commands to the engine.
        cmd_rx: std::sync::mpsc::Receiver<st_engine::DebugCommand>,
    },
    /// Detach the debug session (resume normal cycling).
    DebugDetach,
    /// Force a variable to a constant value (HTTP monitor API).
    ForceVariable {
        name: String,
        value: String,
        reply: tokio::sync::oneshot::Sender<Result<String, String>>,
    },
    /// Remove a force override from a variable (HTTP monitor API).
    UnforceVariable {
        name: String,
        reply: tokio::sync::oneshot::Sender<Result<(), String>>,
    },
    /// Reset cycle min/max/jitter statistics.
    ResetStats,
}

pub struct StartParams {
    pub module: st_ir::Module,
    pub program_name: String,
    pub cycle_time: Option<Duration>,
    pub program_meta: ProgramMetadata,
    /// Native FB registry for Rust-backed function blocks (device I/O).
    /// If None, native FB execute() is not called (fields stay at defaults).
    pub native_fbs: Option<std::sync::Arc<st_comm_api::NativeFbRegistry>>,
}

/// Manages the PLC runtime lifecycle in a dedicated thread.
pub struct RuntimeManager {
    state: Arc<RwLock<RuntimeState>>,
    cmd_tx: tokio::sync::mpsc::Sender<RuntimeCommand>,
    /// Broadcast channel — engine thread sends `()` after every cycle so
    /// WebSocket clients wake up and push variable updates.
    cycle_notify: tokio::sync::broadcast::Sender<()>,
    _config: RuntimeConfig,
}

impl RuntimeManager {
    /// Create a new RuntimeManager with the default retain directory
    /// (`/var/lib/st-plc/retain`).
    pub fn new(config: RuntimeConfig) -> Self {
        Self::new_with_retain_dir(
            config,
            std::path::PathBuf::from("/var/lib/st-plc/retain"),
        )
    }

    /// Create a new RuntimeManager and spawn the runtime thread, using
    /// `retain_dir` as the parent directory for `.retain` snapshot files.
    /// Tests use this to override the default `/var/lib/st-plc/retain`
    /// path so they can run without root privileges.
    pub fn new_with_retain_dir(config: RuntimeConfig, retain_dir: std::path::PathBuf) -> Self {
        let state = Arc::new(RwLock::new(RuntimeState::default()));
        let (cycle_notify, _) = tokio::sync::broadcast::channel(64);
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::channel(16);

        let thread_state = Arc::clone(&state);
        let thread_notify = cycle_notify.clone();
        std::thread::Builder::new()
            .name("plc-runtime".to_string())
            .spawn(move || runtime_thread(thread_state, thread_notify, cmd_rx, retain_dir))
            .expect("Failed to spawn runtime thread");

        RuntimeManager {
            state,
            cmd_tx,
            cycle_notify,
            _config: config,
        }
    }

    /// Get the current runtime state.
    pub fn state(&self) -> RuntimeState {
        self.state.read().unwrap().clone()
    }

    // ── Monitor API methods ─────────────────────────────────────────

    /// Get the variable catalog (names + types). Empty if engine not running.
    pub fn variable_catalog(&self) -> Vec<CatalogEntry> {
        self.state.read().unwrap().variable_catalog.clone()
    }

    /// Get all current variable values. Used by HTTP GET /api/v1/variables.
    pub fn all_variables(&self) -> Vec<VariableValue> {
        self.state.read().unwrap().all_variables.clone()
    }

    /// Subscribe to cycle notifications (for WebSocket push tasks).
    pub fn subscribe_cycles(&self) -> tokio::sync::broadcast::Receiver<()> {
        self.cycle_notify.subscribe()
    }

    /// Reset cycle min/max/jitter statistics.
    pub async fn reset_stats(&self) -> Result<(), ApiError> {
        self.cmd_tx
            .send(RuntimeCommand::ResetStats)
            .await
            .map_err(|_| ApiError::internal("Runtime thread not responding"))
    }

    /// Force a variable to a constant value. Returns the formatted result.
    pub async fn force_variable(&self, name: String, value: String) -> Result<String, ApiError> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.cmd_tx
            .send(RuntimeCommand::ForceVariable { name, value, reply: tx })
            .await
            .map_err(|_| ApiError::internal("Runtime thread not responding"))?;
        rx.await
            .map_err(|_| ApiError::internal("Runtime thread dropped reply"))?
            .map_err(ApiError::internal)
    }

    /// Remove a force override from a variable.
    pub async fn unforce_variable(&self, name: String) -> Result<(), ApiError> {
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.cmd_tx
            .send(RuntimeCommand::UnforceVariable { name, reply: tx })
            .await
            .map_err(|_| ApiError::internal("Runtime thread not responding"))?;
        rx.await
            .map_err(|_| ApiError::internal("Runtime thread dropped reply"))?
            .map_err(ApiError::internal)
    }

    /// Start the runtime with the given module.
    pub async fn start(
        &self,
        module: st_ir::Module,
        program_name: String,
        cycle_time: Option<Duration>,
        program_meta: ProgramMetadata,
        native_fbs: Option<std::sync::Arc<st_comm_api::NativeFbRegistry>>,
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
                native_fbs,
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

    /// Apply an online change to the running engine. Returns a report
    /// of preserved/new/removed variables on success. Returns an
    /// `incompatible` error (HTTP 409) when the new module's variable
    /// layout is not compatible with the running one — the caller is
    /// expected to fall back to a stop+upload+start path.
    pub async fn online_change(
        &self,
        new_module: st_ir::Module,
        new_program_meta: ProgramMetadata,
    ) -> Result<OnlineChangeReport, ApiError> {
        let current_status = self.state.read().unwrap().status;
        if current_status != RuntimeStatus::Running
            && current_status != RuntimeStatus::DebugPaused
        {
            return Err(ApiError::not_running());
        }

        let (tx, rx) = tokio::sync::oneshot::channel();
        self.cmd_tx
            .send(RuntimeCommand::OnlineChange {
                new_module: Box::new(new_module),
                new_program_meta,
                reply: tx,
            })
            .await
            .map_err(|_| ApiError::internal("Runtime thread not responding"))?;

        let result = rx
            .await
            .map_err(|_| ApiError::internal("Runtime thread dropped reply"))?;

        result.map_err(|msg| {
            // Convey "incompatible" as 409 so the API handler can
            // distinguish it from a true internal failure and decide
            // to fall back to a full restart.
            if msg.starts_with("Incompatible change") {
                ApiError::online_change_incompatible(msg)
            } else {
                ApiError::internal(msg)
            }
        })
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

}

/// The runtime thread loop. Owns the Engine and executes scan cycles.
fn runtime_thread(
    state: Arc<RwLock<RuntimeState>>,
    cycle_notify: tokio::sync::broadcast::Sender<()>,
    mut cmd_rx: tokio::sync::mpsc::Receiver<RuntimeCommand>,
    retain_dir: std::path::PathBuf,
) {
    while let Some(cmd) = cmd_rx.blocking_recv() {
        match cmd {
            RuntimeCommand::Shutdown => break,
            RuntimeCommand::Stop => {
                // Already idle, ignore
            }
            RuntimeCommand::Start(params) => {
                let StartParams { module, program_name, cycle_time, program_meta, native_fbs } = *params;
                tracing::info!(
                    "Engine starting: program={}, cycle_time={:?}",
                    program_name,
                    cycle_time
                );
                // Update state to Starting
                {
                    let mut s = state.write().unwrap();
                    s.status = RuntimeStatus::Starting;
                    s.program = Some(program_meta);
                    s.error = None;
                    s.started_at = Some(chrono::Utc::now().to_rfc3339());
                }

                // Build engine config with retain persistence. The retain
                // directory comes from the agent's storage config (or a
                // test override) — see `RuntimeManager::new_with_retain_dir`.
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

                // Construct engine inside this thread (avoids Send issues).
                // Pass native FB registry so device execute() is called each cycle.
                let mut engine = st_engine::Engine::new_with_native_fbs(
                    module, program_name, engine_config, native_fbs,
                );

                // Populate variable catalog in shared state (set once).
                {
                    let catalog: Vec<CatalogEntry> = engine
                        .vm()
                        .monitorable_catalog()
                        .into_iter()
                        .map(|c| CatalogEntry {
                            name: c.name,
                            ty: c.ty,
                            retain: c.retain,
                            persistent: c.persistent,
                        })
                        .collect();
                    tracing::info!(
                        "Engine catalog populated: {} monitorable variables",
                        catalog.len()
                    );
                    if !catalog.is_empty() {
                        let sample: Vec<&str> = catalog.iter().take(5).map(|c| c.name.as_str()).collect();
                        tracing::debug!("Catalog sample: {sample:?}");
                    }
                    let mut s = state.write().unwrap();
                    s.variable_catalog = catalog;
                    s.status = RuntimeStatus::Running;
                }

                // Scan cycle loop
                let run_result = run_cycle_loop(
                    &mut engine, &state, &cycle_notify, &mut cmd_rx, cycle_time,
                );

                // Save retained variables before dropping the engine
                if let Err(e) = engine.save_retain() {
                    tracing::warn!("Retain save on stop: {e}");
                }

                // Update state based on result
                {
                    let mut s = state.write().unwrap();
                    s.variable_catalog.clear();
                    s.all_variables.clear();
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
            RuntimeCommand::OnlineChange { reply, .. } => {
                let _ = reply.send(Err(
                    "Runtime is not running; cannot apply online change".to_string(),
                ));
            }
            RuntimeCommand::ForceVariable { reply, .. } => {
                let _ = reply.send(Err("Runtime is not running".to_string()));
            }
            RuntimeCommand::UnforceVariable { reply, .. } => {
                let _ = reply.send(Err("Runtime is not running".to_string()));
            }
            RuntimeCommand::ResetStats => {} // Idle, nothing to reset
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
    cycle_notify: &tokio::sync::broadcast::Sender<()>,
    cmd_rx: &mut tokio::sync::mpsc::Receiver<RuntimeCommand>,
    cycle_time: Option<Duration>,
) -> Result<StopReason, String> {
    let mut debug_session: Option<DebugSession> = None;
    let mut snapshot_cycle: u64 = 0;
    let target_cycle_us = cycle_time.map(|d| d.as_micros() as u64).unwrap_or(0);

    loop {
        // Execute one scan cycle
        match engine.run_one_cycle() {
            Ok(cycle_elapsed) => {
                // Normal cycle completed — update stats + variable snapshot
                update_cycle_stats(engine, state, target_cycle_us);
                snapshot_all_variables(engine, state, &mut snapshot_cycle);
                // Only broadcast when WS clients are listening
                if cycle_notify.receiver_count() > 0 {
                    let _ = cycle_notify.send(());
                }

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
            Ok(RuntimeCommand::OnlineChange { new_module, new_program_meta, reply }) => {
                let started = std::time::Instant::now();
                match engine.online_change_module(*new_module) {
                    Ok(analysis) => {
                        // Refresh variable catalog (slot count may have grown).
                        let catalog: Vec<CatalogEntry> = engine
                            .vm()
                            .monitorable_catalog()
                            .into_iter()
                            .map(|c| CatalogEntry {
                                name: c.name,
                                ty: c.ty,
                                retain: c.retain,
                                persistent: c.persistent,
                            })
                            .collect();
                        let mut s = state.write().unwrap();
                        s.variable_catalog = catalog;
                        s.program = Some(new_program_meta);
                        drop(s);
                        tracing::info!(
                            "Online change applied in {:?}: {} preserved, {} new, {} removed",
                            started.elapsed(),
                            analysis.preserved_vars.len(),
                            analysis.new_vars.len(),
                            analysis.removed_vars.len(),
                        );
                        let _ = reply.send(Ok(OnlineChangeReport {
                            preserved_vars: analysis.preserved_vars,
                            new_vars: analysis.new_vars,
                            removed_vars: analysis.removed_vars,
                        }));
                    }
                    Err(e) => {
                        tracing::warn!("Online change rejected: {e}");
                        let _ = reply.send(Err(e));
                    }
                }
            }
            Ok(RuntimeCommand::ForceVariable { name, value, reply }) => {
                tracing::info!("Engine: force {name} = {value}");
                let result = handle_force(engine, &name, &value);
                if let Err(ref e) = result {
                    tracing::warn!("Engine: force failed — {e}");
                }
                let _ = reply.send(result);
            }
            Ok(RuntimeCommand::UnforceVariable { name, reply }) => {
                tracing::info!("Engine: unforce {name}");
                engine.vm_mut().unforce_variable(&name);
                let _ = reply.send(Ok(()));
            }
            Ok(RuntimeCommand::ResetStats) => {
                tracing::info!("Engine: resetting cycle stats");
                engine.reset_stats();
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
                            source_offset,
                        } => {
                            let module = engine.vm().module().clone();
                            engine.vm_mut().debug_mut().clear_breakpoints();
                            let results = engine.vm_mut().debug_mut().set_line_breakpoints(
                                &module, &source, &lines, source_offset,
                            );
                            let set_count = results.iter().filter(|r| r.is_some()).count();
                            tracing::info!(
                                "Debug: setBreakpoints (running) — {set_count}/{} verified, source_offset={source_offset}, source_len={}, lines={:?}",
                                results.len(),
                                source.len(),
                                lines,
                            );
                            tracing::debug!(
                                "Debug: active breakpoint count: {}",
                                engine.vm().debug_state().source_breakpoint_count()
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
        // Check for runtime commands (Stop/Shutdown/Force) between debug commands
        match runtime_cmd_rx.try_recv() {
            Ok(RuntimeCommand::Stop) => return DebugAction::Stop,
            Ok(RuntimeCommand::Shutdown) => return DebugAction::Shutdown,
            Ok(RuntimeCommand::DebugDetach) => return DebugAction::Detach,
            Ok(RuntimeCommand::ForceVariable { name, value, reply }) => {
                let result = handle_force(engine, &name, &value);
                let _ = reply.send(result);
            }
            Ok(RuntimeCommand::UnforceVariable { name, reply }) => {
                engine.vm_mut().unforce_variable(&name);
                let _ = reply.send(Ok(()));
            }
            Ok(RuntimeCommand::DebugAttach { event_tx, cmd_rx: new_cmd_rx }) => {
                // A new debug client connected while we're paused.
                // Detach the old session and swap to the new one.
                tracing::info!("Debug: new session attached while paused — swapping");
                if let Some(old) = debug_session.as_ref() {
                    let _ = old.event_tx.send(st_engine::DebugResponse::Detached);
                }
                *debug_session = Some(DebugSession {
                    event_tx,
                    cmd_rx: new_cmd_rx,
                });
                // Re-enter the loop with the new session
                return DebugAction::Resume;
            }
            Ok(RuntimeCommand::Start { .. }) | Ok(RuntimeCommand::ResetStats) => {
                // Ignore non-critical commands while debugging
            }
            Ok(RuntimeCommand::OnlineChange { reply, .. }) => {
                // Refuse online changes while paused at a breakpoint —
                // the VM holds a stack frame that would be invalidated by
                // a module swap. Caller can stop+restart instead.
                let _ = reply.send(Err(
                    "Cannot apply online change while debug session is paused".to_string(),
                ));
            }
            Err(_) => {}
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
                st_engine::DebugCommand::SetBreakpoints { source_path: _, source, lines, source_offset } => {
                    let module = engine.vm().module().clone();
                    engine.vm_mut().debug_mut().clear_breakpoints();
                    let results = engine.vm_mut().debug_mut().set_line_breakpoints(
                        &module, &source, &lines, source_offset,
                    );
                    let set_count = results.iter().filter(|r| r.is_some()).count();
                    tracing::info!(
                        "Debug: setBreakpoints (paused) — {set_count}/{} verified, source_offset={source_offset}, lines={:?}",
                        results.len(),
                        lines,
                    );
                    tracing::debug!(
                        "Debug: active breakpoint count: {}",
                        engine.vm().debug_state().source_breakpoint_count()
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
fn update_cycle_stats(
    engine: &st_engine::Engine,
    state: &Arc<RwLock<RuntimeState>>,
    target_cycle_us: u64,
) {
    let stats = engine.stats();
    let mut s = state.write().unwrap();
    s.cycle_stats = Some(CycleStatsSnapshot {
        cycle_count: stats.cycle_count,
        last_cycle_time_us: stats.last_cycle_time.as_micros() as u64,
        min_cycle_time_us: stats.min_cycle_time.as_micros() as u64,
        max_cycle_time_us: stats.max_cycle_time.as_micros() as u64,
        avg_cycle_time_us: stats.avg_cycle_time().as_micros() as u64,
        target_cycle_us,
        last_period_us: stats.last_cycle_period.as_micros() as u64,
        min_period_us: if stats.min_cycle_period == Duration::MAX {
            0
        } else {
            stats.min_cycle_period.as_micros() as u64
        },
        max_period_us: stats.max_cycle_period.as_micros() as u64,
        jitter_max_us: stats.jitter_max.as_micros() as u64,
    });
}

/// Snapshot ALL monitorable variable values from the engine into shared state.
/// WebSocket clients filter this per their subscription sets.
fn snapshot_all_variables(
    engine: &st_engine::Engine,
    state: &Arc<RwLock<RuntimeState>>,
    cycle_count: &mut u64,
) {
    let forced = engine.vm().forced_variables();
    let all_vars = engine.vm().monitorable_variables();

    let snapshot: Vec<VariableValue> = all_vars
        .into_iter()
        .map(|v| {
            let is_forced = forced.contains_key(&v.name.to_uppercase());
            VariableValue {
                name: v.name,
                value: v.value,
                ty: v.ty,
                forced: is_forced,
                retain: v.retain,
                persistent: v.persistent,
            }
        })
        .collect();

    // Log once at startup, then every 10000 cycles
    *cycle_count += 1;
    if *cycle_count == 1 {
        tracing::info!(
            "First variable snapshot: {} variables",
            snapshot.len()
        );
        if !snapshot.is_empty() {
            let sample: Vec<String> = snapshot
                .iter()
                .take(5)
                .map(|v| format!("{}={}", v.name, v.value))
                .collect();
            tracing::debug!("Snapshot sample: {sample:?}");
        }
    } else if *cycle_count % 10_000 == 0 {
        tracing::debug!(
            "Variable snapshot #{}: {} vars, {} forced",
            cycle_count,
            snapshot.len(),
            forced.len()
        );
    }

    state.write().unwrap().all_variables = snapshot;
}

/// Parse a value string and force a variable. Returns a description on success.
fn handle_force(
    engine: &mut st_engine::Engine,
    name: &str,
    value_str: &str,
) -> Result<String, String> {
    let value = parse_value_string(value_str);
    engine.vm_mut().force_variable(name, value.clone());
    Ok(format!(
        "Forced {} = {}",
        name,
        st_engine::debug::format_value(&value)
    ))
}

/// Parse a user-provided value string into a `Value`.
/// Same logic as the DAP force handler: bool → int → float → string.
fn parse_value_string(s: &str) -> st_ir::Value {
    if s.eq_ignore_ascii_case("true") {
        st_ir::Value::Bool(true)
    } else if s.eq_ignore_ascii_case("false") {
        st_ir::Value::Bool(false)
    } else if let Ok(i) = s.parse::<i64>() {
        st_ir::Value::Int(i)
    } else if let Ok(f) = s.parse::<f64>() {
        st_ir::Value::Real(f)
    } else {
        st_ir::Value::String(s.to_string())
    }
}

// The runtime lifecycle (idle → starting → running → idle, cycle stat
// advancement, stop-when-idle errors) is exercised end-to-end by
// `tests/api_integration.rs::{test_status_idle, test_start_stop,
// test_stop_when_idle, test_cycle_stats_advance, test_full_lifecycle}`.
// The remaining unit test below covers a race that's hard to reproduce
// over HTTP — issuing a second `start` while the runtime thread is still
// in the brief `Starting` window.
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

        mgr.start(module.clone(), name.clone(), Some(Duration::from_millis(10)), meta.clone(), None)
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;

        let result = mgr.start(module, name, Some(Duration::from_millis(10)), meta, None).await;
        assert!(result.is_err());

        mgr.stop().await.unwrap();
        tokio::time::sleep(Duration::from_millis(50)).await;
        mgr.shutdown().await;
    }
}
