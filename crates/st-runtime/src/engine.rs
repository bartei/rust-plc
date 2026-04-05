//! Scan cycle engine: runs PLC programs in a cyclic loop.

use crate::vm::{Vm, VmConfig, VmError};
use st_ir::*;
use std::time::{Duration, Instant};

/// Scan cycle statistics.
#[derive(Debug, Clone, Default)]
pub struct CycleStats {
    pub cycle_count: u64,
    pub last_cycle_time: Duration,
    pub min_cycle_time: Duration,
    pub max_cycle_time: Duration,
    pub total_time: Duration,
}

impl CycleStats {
    pub fn avg_cycle_time(&self) -> Duration {
        if self.cycle_count == 0 {
            Duration::ZERO
        } else {
            self.total_time / self.cycle_count as u32
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
}

#[allow(clippy::derivable_impls)]
impl Default for EngineConfig {
    fn default() -> Self {
        Self {
            cycle_time: None,
            max_cycles: 0,
            vm_config: VmConfig::default(),
            watchdog_timeout: None,
        }
    }
}

/// The PLC scan cycle engine.
pub struct Engine {
    vm: Vm,
    config: EngineConfig,
    stats: CycleStats,
    program_name: String,
}

impl Engine {
    /// Create a new engine from a compiled module.
    pub fn new(module: Module, program_name: String, config: EngineConfig) -> Self {
        let vm = Vm::new(module, config.vm_config.clone());
        Self {
            vm,
            config,
            stats: CycleStats {
                min_cycle_time: Duration::MAX,
                ..Default::default()
            },
            program_name,
        }
    }

    /// Run the scan cycle loop. Returns after max_cycles or on error.
    pub fn run(&mut self) -> Result<(), VmError> {
        loop {
            if self.config.max_cycles > 0 && self.stats.cycle_count >= self.config.max_cycles {
                return Ok(());
            }
            self.run_one_cycle()?;
        }
    }

    /// Run a single scan cycle.
    pub fn run_one_cycle(&mut self) -> Result<Duration, VmError> {
        let start = Instant::now();

        self.vm.reset_instruction_count();
        self.vm.scan_cycle(&self.program_name)?;

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

        Ok(elapsed)
    }

    /// Get the current cycle statistics.
    pub fn stats(&self) -> &CycleStats {
        &self.stats
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
