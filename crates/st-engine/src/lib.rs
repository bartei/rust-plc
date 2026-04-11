//! PLC runtime: bytecode VM, scan cycle engine, and task scheduler.
//!
//! Executes compiled bytecode in a cyclic scan loop with support for
//! IEC 61131-3 task scheduling, watchdog timers, and online change.

pub mod comm_manager;
pub mod debug;
pub mod engine;
pub mod online_change;
pub mod retain_store;
pub mod vm;

pub use debug::{DebugCommand, DebugResponse, DebugScopeKind};
pub use engine::{Engine, EngineConfig, CycleStats};
pub use retain_store::RetainConfig;
pub use vm::{Vm, VmConfig, VmError};
