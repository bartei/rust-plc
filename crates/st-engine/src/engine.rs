//! Scan cycle engine: runs PLC programs in a cyclic loop.

use crate::vm::{Vm, VmConfig, VmError};
use st_ir::*;
use std::time::{Duration, Instant};

/// Scan cycle statistics.
///
/// Note the distinction between *cycle time* and *cycle period*:
/// - **cycle time** (`*_cycle_time`) is pure VM execution time per cycle —
///   how long the program took to run.
/// - **cycle period** (`*_cycle_period`) is the wall-clock interval between
///   the start of one cycle and the start of the next — execution time
///   plus any inter-cycle sleep enforced by `EngineConfig.cycle_time`.
///
/// For control loops (PID, position, temperature) the **period** is what
/// matters: a PID that expects samples every 10ms but gets them at
/// 10±0.5ms accumulates integral error. The `jitter_max` field reports
/// the worst absolute deviation of the period from the configured target.
#[derive(Debug, Clone, Default)]
pub struct CycleStats {
    pub cycle_count: u64,
    /// Pure VM execution time of the most recent cycle.
    pub last_cycle_time: Duration,
    pub min_cycle_time: Duration,
    pub max_cycle_time: Duration,
    /// Sum of `last_cycle_time` across all cycles. Does NOT include sleep
    /// time enforced between cycles by `EngineConfig.cycle_time`.
    pub total_time: Duration,

    /// Wall-clock interval between the most recent two cycle starts. Zero
    /// before the second cycle has run.
    pub last_cycle_period: Duration,
    /// Smallest period observed since the engine started.
    pub min_cycle_period: Duration,
    /// Largest period observed since the engine started.
    pub max_cycle_period: Duration,
    /// Maximum absolute deviation of any observed period from the configured
    /// `cycle_time` target. Zero when no `cycle_time` is set (free-run mode
    /// has no meaningful target to deviate from).
    pub jitter_max: Duration,
}

impl CycleStats {
    /// Average cycle execution time computed in u128 nanoseconds so that
    /// long-running PLC sessions never silently lose precision. The previous
    /// `total_time / cycle_count as u32` cast wrapped after 4.29 billion
    /// cycles (~71 minutes at 1µs/cycle), which is well within the
    /// "indefinite debug session" use case.
    pub fn avg_cycle_time(&self) -> Duration {
        if self.cycle_count == 0 {
            Duration::ZERO
        } else {
            let avg_ns = self.total_time.as_nanos() / self.cycle_count as u128;
            // The result fits in u64 nanos for any single cycle that doesn't
            // exceed Duration::MAX itself — i.e., always.
            Duration::from_nanos(avg_ns as u64)
        }
    }
}

/// Configuration for the scan cycle engine.
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Target cycle time. If None, runs as fast as possible.
    pub cycle_time: Option<Duration>,
    /// Maximum number of cycles (0 = unlimited).
    pub max_cycles: u64,
    /// VM configuration.
    pub vm_config: VmConfig,
    /// Watchdog timeout — if a single cycle exceeds this, abort.
    pub watchdog_timeout: Option<Duration>,
    /// Retain/persistent variable storage. None = no persistence.
    pub retain: Option<crate::retain_store::RetainConfig>,
}

#[allow(clippy::derivable_impls)]
impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            cycle_time: None,
            max_cycles: 0,
            vm_config: VmConfig::default(),
            watchdog_timeout: None,
            retain: None,
        }
    }
}

/// The PLC scan cycle engine.
pub struct Engine {
    vm: Vm,
    config: EngineConfig,
    stats: CycleStats,
    program_name: String,
    /// Tracks when the previous scan cycle started (for period calculation).
    previous_cycle_start: Option<Instant>,
    /// Cycle counter for periodic retain checkpoints.
    retain_cycle_counter: u32,
}

impl Engine {
    /// Create a new engine from a compiled module. Runs the synthetic
    /// `__global_init` function (if present) so `VAR_GLOBAL x : T := <expr>;`
    /// initial values are applied before the first scan cycle.
    pub fn new(module: Module, program_name: String, config: EngineConfig) -> Self {
        let mut vm = Vm::new(module, config.vm_config.clone());
        // If global init fails (e.g. division by zero in an initializer)
        // we leave the engine constructible — the VM keeps its default
        // values and the user will see the issue at runtime.
        let _ = vm.run_global_init();

        // Restore retained/persistent variables from disk (warm restart).
        if let Some(ref retain_cfg) = config.retain {
            if retain_cfg.path.exists() {
                match crate::retain_store::load_from_file(&retain_cfg.path) {
                    Ok(snapshot) => {
                        let warnings =
                            crate::retain_store::restore_snapshot(&mut vm, &snapshot, true);
                        for w in &warnings {
                            tracing::warn!("Retain restore: {w}");
                        }
                        tracing::info!(
                            "Restored {} globals, {} programs from {}",
                            snapshot.globals.len(),
                            snapshot.program_locals.len(),
                            retain_cfg.path.display(),
                        );
                    }
                    Err(e) => {
                        tracing::warn!("Failed to load retain file: {e}");
                    }
                }
            }
        }

        Self {
            vm,
            config,
            stats: CycleStats {
                min_cycle_time: Duration::MAX,
                min_cycle_period: Duration::MAX,
                ..Default::default()
            },
            program_name,
            previous_cycle_start: None,
            retain_cycle_counter: 0,
        }
    }

    /// Create a new engine with an optional native FB registry for Rust-backed FBs.
    pub fn new_with_native_fbs(
        module: Module,
        program_name: String,
        config: EngineConfig,
        native_fbs: Option<std::sync::Arc<st_comm_api::NativeFbRegistry>>,
    ) -> Self {
        let mut vm = Vm::new_with_native_fbs(module, config.vm_config.clone(), native_fbs);
        let _ = vm.run_global_init();

        if let Some(ref retain_cfg) = config.retain {
            if retain_cfg.path.exists() {
                match crate::retain_store::load_from_file(&retain_cfg.path) {
                    Ok(snapshot) => {
                        let warnings =
                            crate::retain_store::restore_snapshot(&mut vm, &snapshot, true);
                        for w in &warnings {
                            tracing::warn!("Retain restore: {w}");
                        }
                        tracing::info!(
                            "Restored {} globals, {} programs from {}",
                            snapshot.globals.len(),
                            snapshot.program_locals.len(),
                            retain_cfg.path.display(),
                        );
                    }
                    Err(e) => {
                        tracing::warn!("Failed to load retain file: {e}");
                    }
                }
            }
        }

        Self {
            vm,
            config,
            stats: CycleStats {
                min_cycle_time: Duration::MAX,
                min_cycle_period: Duration::MAX,
                ..Default::default()
            },
            program_name,
            previous_cycle_start: None,
            retain_cycle_counter: 0,
        }
    }

    /// Run the scan cycle loop. Returns after max_cycles or on error.
    /// VM internally so callers don't have to juggle borrows.
    /// Run the scan cycle loop. Returns after max_cycles or on error.
    /// If `EngineConfig.cycle_time` is set, sleeps after each cycle so the
    /// total cycle period (execution + sleep) matches the target. If a single
    /// cycle exceeds the target the next cycle starts immediately (no
    /// catch-up sleep accumulation).
    pub fn run(&mut self) -> Result<(), VmError> {
        loop {
            if self.config.max_cycles > 0 && self.stats.cycle_count >= self.config.max_cycles {
                return Ok(());
            }
            let elapsed = self.run_one_cycle()?;
            if let Some(target) = self.config.cycle_time {
                if let Some(remaining) = target.checked_sub(elapsed) {
                    if !remaining.is_zero() {
                        std::thread::sleep(remaining);
                    }
                }
            }
        }
    }

    /// Run a single scan cycle.
    pub fn run_one_cycle(&mut self) -> Result<Duration, VmError> {
        let start = Instant::now();

        // Period tracking: measure wall-clock interval between cycle starts.
        if let Some(prev) = self.previous_cycle_start {
            let period = start.duration_since(prev);
            self.stats.last_cycle_period = period;
            if period < self.stats.min_cycle_period {
                self.stats.min_cycle_period = period;
            }
            if period > self.stats.max_cycle_period {
                self.stats.max_cycle_period = period;
            }
            // Jitter: deviation from the configured target cycle time.
            if let Some(target) = self.config.cycle_time {
                let dev = period.abs_diff(target);
                if dev > self.stats.jitter_max {
                    self.stats.jitter_max = dev;
                }
            }
        }
        self.previous_cycle_start = Some(start);

        // Update elapsed time for timer FBs (milliseconds since engine start)
        let elapsed_ms = self.stats.total_time.as_millis() as i64;
        self.vm.set_elapsed_time_ms(elapsed_ms);

        self.vm.reset_instruction_count();

        // If the VM has an active call stack (debug resume mid-cycle),
        // continue execution from where it paused. Otherwise start a
        // fresh scan cycle with I/O read → execute → I/O write.
        if self.vm.call_depth() > 0 {
            // Resuming a debug-paused cycle
            self.vm.continue_execution()?;
        } else {
            // Normal fresh cycle — native FBs handle I/O inside execute()
            self.vm.scan_cycle(&self.program_name)?;

            // Apply forced values to PROGRAM locals (retained_locals).
            // Globals are enforced via forced_global_slots in set_global_by_slot.
            self.vm.enforce_retained_locals();
        }

        let elapsed = start.elapsed();

        // Check watchdog
        if let Some(timeout) = self.config.watchdog_timeout {
            if elapsed > timeout {
                return Err(VmError::ExecutionLimit(0));
            }
        }

        // Update stats
        self.stats.cycle_count += 1;
        self.stats.last_cycle_time = elapsed;
        self.stats.total_time += elapsed;
        if elapsed < self.stats.min_cycle_time {
            self.stats.min_cycle_time = elapsed;
        }
        if elapsed > self.stats.max_cycle_time {
            self.stats.max_cycle_time = elapsed;
        }

        // Periodic retain checkpoint
        if let Some(ref retain_cfg) = self.config.retain {
            if retain_cfg.checkpoint_cycles > 0 {
                self.retain_cycle_counter += 1;
                if self.retain_cycle_counter >= retain_cfg.checkpoint_cycles {
                    self.retain_cycle_counter = 0;
                    if let Err(e) = self.save_retain() {
                        tracing::warn!("Retain checkpoint failed: {e}");
                    }
                }
            }
        }

        Ok(elapsed)
    }

    /// Save retained/persistent variables to disk. Called on shutdown,
    /// periodically during execution, and before online change.
    pub fn save_retain(&self) -> Result<(), String> {
        let Some(ref retain_cfg) = self.config.retain else {
            return Ok(());
        };
        let snapshot = crate::retain_store::capture_snapshot(&self.vm);
        if snapshot.globals.is_empty()
            && snapshot.program_locals.is_empty()
            && snapshot.instance_fields.is_empty()
        {
            return Ok(());
        }
        crate::retain_store::save_to_file(&snapshot, &retain_cfg.path)
    }

    /// Get the current cycle statistics.
    pub fn stats(&self) -> &CycleStats {
        &self.stats
    }

    /// Reset min/max/jitter stats (keeps cycle_count and totals).
    pub fn reset_stats(&mut self) {
        self.stats.min_cycle_time = Duration::MAX;
        self.stats.max_cycle_time = Duration::ZERO;
        self.stats.min_cycle_period = Duration::MAX;
        self.stats.max_cycle_period = Duration::ZERO;
        self.stats.jitter_max = Duration::ZERO;
    }

    /// Get a reference to the VM for variable inspection.
    pub fn vm(&self) -> &Vm {
        &self.vm
    }

    /// Get a mutable reference to the VM for variable manipulation.
    pub fn vm_mut(&mut self) -> &mut Vm {
        &mut self.vm
    }

    /// Apply an online change from new source code.
    /// Call this between scan cycles (not during execution).
    pub fn online_change(&mut self, new_source: &str) -> Result<crate::online_change::ChangeAnalysis, String> {
        let stdlib = st_syntax::multi_file::builtin_stdlib();
        let mut all: Vec<&str> = stdlib;
        all.push(new_source);
        let parse_result = st_syntax::multi_file::parse_multi(&all);
        if !parse_result.errors.is_empty() {
            return Err("Parse errors in new source".to_string());
        }

        let new_module = st_compiler::compile(&parse_result.source_file)
            .map_err(|e| format!("Compilation error: {e}"))?;

        self.online_change_module(new_module)
    }

    /// Apply an online change from an already-compiled [`Module`].
    /// Used by the target agent which deserializes the module from a
    /// program bundle. Call between scan cycles, not during execution.
    pub fn online_change_module(
        &mut self,
        new_module: Module,
    ) -> Result<crate::online_change::ChangeAnalysis, String> {
        // Save retain state before applying change (cold restart save point).
        if let Err(e) = self.save_retain() {
            tracing::warn!("Retain save before online change failed: {e}");
        }

        let old_module = self.vm.module().clone();
        let analysis = crate::online_change::analyze_change(&old_module, &new_module);

        if !analysis.compatible {
            return Err(format!(
                "Incompatible change: {}",
                analysis.incompatible_reasons.join("; ")
            ));
        }

        // Update program name if it changed
        if let Some(new_prog) = new_module.functions.iter().find(|f| f.kind == st_ir::PouKind::Program) {
            self.program_name = new_prog.name.clone();
        }

        crate::online_change::apply_online_change(&mut self.vm, new_module, &analysis)
            .map_err(|e| format!("Apply error: {e}"))?;

        Ok(analysis)
    }
}
