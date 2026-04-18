# st-engine

PLC runtime virtual machine and scan cycle engine.

## Purpose

Executes compiled bytecode in a deterministic cyclic scan loop. Manages the VM, communication I/O, task scheduling, watchdog timers, online change (hot-reload), variable retention, and debugging hooks. This is the core PLC runtime.

## Public API

### Engine (Scan Cycle)

```rust
use st_engine::{Engine, EngineConfig};

let engine = Engine::new(module, "Main".to_string(), EngineConfig {
    cycle_time: Some(Duration::from_millis(10)),
    max_cycles: 0, // unlimited
    ..Default::default()
});

// Run continuously
engine.run()?;

// Or run one cycle at a time
let elapsed = engine.run_one_cycle()?;
```

- `Engine` — Scan cycle orchestrator: owns `Vm` + `CommManager`, runs the read→execute→write loop
- `EngineConfig` — Cycle time, max cycles, watchdog timeout, retain config
- `CycleStats` — Performance metrics: cycle count, last/min/max/avg cycle time, period, jitter

### VM (Bytecode Execution)

- `Vm` — Register-based bytecode interpreter with global/local variable frames
- `VmConfig` — Max call depth, max instructions per cycle
- `VmError` — Runtime errors: division by zero, stack overflow, execution limit, halt (debug)

### Key VM Methods

- `get_global(name)` / `get_global_by_slot(u16)` — Read global variables
- `set_global(name, value)` / `set_global_by_slot(u16, value)` — Write globals (respects force)
- `force_variable(name, value)` / `unforce_variable(name)` — Debug force/unforce
- `monitorable_variables()` — All variables with current values (for monitor/OPC-UA)
- `monitorable_catalog()` — Variable names + types (schema only)
- `scan_cycle(program_name)` — Execute one program scan

### Communication Manager

- `CommManager` — Bridges `CommDevice` implementations with VM global variable slots
- `register_device(device, instance_name, vm, cycle_time)` — Map device fields to globals
- `read_inputs(vm)` / `write_outputs(vm)` — Cyclic I/O exchange

### Other Modules

- `debug` — Debug state, breakpoints, stepping, pause/resume
- `online_change` — Hot-reload with variable migration
- `retain_store` — Persistent variable storage (RETAIN/PERSISTENT)

## Functional Description

The scan cycle runs in a tight loop:

```
loop {
    1. comm.read_inputs()        ← device registers → VM globals
    2. vm.scan_cycle("Main")     ← execute user program
    3. vm.enforce_retained_locals()
    4. comm.write_outputs()      ← VM globals → device registers
    5. update stats, check watchdog
    6. sleep(remaining cycle time)
}
```

The engine thread is a dedicated `std::thread` — it is NOT a tokio task. This ensures deterministic timing unaffected by async I/O workloads.

## Configuration

Engine settings come from `plc-project.yaml`:

```yaml
engine:
  cycle_time: 10ms              # target scan cycle period
```

Retention settings:

```yaml
engine:
  retain_checkpoint_cycles: 1000  # save retained vars every N cycles
```

## Dependencies

| Crate | Purpose |
|-------|---------|
| `st-ir` | Bytecode and module types |
| `st-comm-api` | Communication device trait |
| `tokio` | Async support (for sync channels with agent) |
| `tracing` | Structured logging |
| `serde`, `serde_json` | Retain file serialization |

## Production Deployment

The engine runs inside `st-runtime` (as part of `st-target-agent`'s `RuntimeManager`). It is never deployed standalone — the agent manages its lifecycle, restarts on crash, and exposes its state via HTTP/WebSocket/OPC-UA.
