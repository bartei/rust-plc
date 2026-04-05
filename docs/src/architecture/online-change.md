# Online Change

Online change (hot-reload) allows you to modify a running PLC program's logic
without stopping the scan-cycle engine. The system analyzes compatibility
between the old and new compiled modules, migrates variable state, and performs
an atomic swap.

## Overview

The online change pipeline consists of three steps:

```
  Old Module + New Source
        |
        v
  ┌─────────────────────┐
  │  analyze_change()    │ ── Compare old and new modules
  │  → Compatible?       │    for structural equivalence
  └──────────┬──────────┘
             │ yes
             v
  ┌─────────────────────┐
  │  migrate_locals()    │ ── Copy variable values from old
  │  → State preserved   │    VM state into new module layout
  └──────────┬──────────┘
             │
             v
  ┌─────────────────────┐
  │  apply_online_change │ ── Atomic swap of the module in
  │  → Engine updated    │    the running engine
  └─────────────────────┘
```

If the change is **incompatible**, the system reports the reason and the caller
must perform a full restart instead.

## Compatibility Analysis

`analyze_change(old_module, new_module)` compares two `st_ir::Module` values
and returns a `ChangeResult`:

```rust
pub enum ChangeResult {
    Compatible {
        /// Variables whose values can be migrated
        migrateable: Vec<String>,
    },
    Incompatible {
        reason: String,
    },
}
```

### What is Compatible

The following changes can be applied online without stopping the engine:

| Change | Example | Why it works |
|--------|---------|--------------|
| Modified program body logic | Changed an `IF` condition or assignment | Same variable layout, only bytecode changes |
| Reordered statements | Moved assignments around | Same variable layout |
| Changed literal values | `limit := 50` to `limit := 100` | Same variable layout |
| Added/removed comments | `(* new comment *)` | No effect on compiled output |
| Modified function bodies | Changed internal computation | Functions are stateless |

### What is Incompatible

These changes require a full restart:

| Change | Example | Why it fails |
|--------|---------|-------------|
| Added a variable | New `VAR counter2 : INT; END_VAR` | Memory layout changed |
| Removed a variable | Deleted a VAR declaration | Memory layout changed |
| Changed a variable's type | `counter : INT` to `counter : DINT` | Value size changed |
| Renamed a variable | `counter` to `cnt` | Name-based migration cannot match |
| Added/removed a POU | New `FUNCTION` or `FUNCTION_BLOCK` | Module structure changed |
| Changed function signatures | Added a parameter to a function | Call sites would be invalid |

## Variable Migration

When a change is compatible, `migrate_locals(old_vm, new_module)` copies
variable values from the old VM's memory into the new module's memory layout:

```rust
pub fn migrate_locals(
    old_vm: &Vm,
    new_module: &mut Module,
) -> Result<MigrationReport, MigrationError> {
    // For each variable in the new module that also exists in the old module
    // with the same type, copy the current runtime value.
}
```

The migration is **name-and-type-based**: a variable in the new module receives
the old value only if a variable with the same name and same type exists in the
old module. This ensures type safety during the swap.

### Migration Report

The migration returns a report listing:

- **Migrated** -- Variables whose values were copied successfully
- **Defaulted** -- Variables that exist only in the new module (initialized to defaults)
- **Dropped** -- Variables that exist only in the old module (values discarded)

## Atomic Swap

`apply_online_change(engine, new_module)` performs the actual replacement:

1. The engine finishes the current scan cycle (never interrupts mid-cycle)
2. The old module is swapped out and the new module is installed
3. The next scan cycle executes the new bytecode
4. Program locals that were migrated retain their values

```rust
pub fn apply_online_change(
    engine: &mut Engine,
    new_module: Module,
) -> Result<(), OnlineChangeError> {
    // Wait for current cycle to complete
    // Swap module atomically
    // Reset PC to body_start_pc (skip re-initialization)
}
```

The use of `body_start_pc` is critical: it causes the VM to skip the variable
initialization preamble on the next cycle, preserving the migrated values. This
is the same mechanism used for normal scan-cycle local retention in PROGRAMs.

## Code Example: Hot-Reload Workflow

### Original program

```st
PROGRAM Main
VAR
    counter : INT := 0;
    limit   : INT := 50;
    active  : BOOL := FALSE;
END_VAR
    counter := counter + 1;
    IF counter > limit THEN
        active := TRUE;
    END_IF;
END_PROGRAM
```

### Modified program (compatible change)

```st
PROGRAM Main
VAR
    counter : INT := 0;
    limit   : INT := 50;
    active  : BOOL := FALSE;
END_VAR
    counter := counter + 2;         (* changed: increment by 2 *)
    IF counter > limit THEN
        active := TRUE;
        counter := 0;              (* added: reset on overflow *)
    END_IF;
END_PROGRAM
```

This change is **compatible** because:
- The variable declarations are identical (same names, same types, same order)
- Only the program body logic changed

After online change:
- `counter` retains its current runtime value (e.g., 37)
- `limit` retains its value (50)
- `active` retains its value (FALSE or TRUE depending on state)
- The new logic takes effect on the very next scan cycle

### Incompatible change example

```st
PROGRAM Main
VAR
    counter : DINT := 0;           (* changed type: INT → DINT *)
    limit   : INT := 50;
    active  : BOOL := FALSE;
    log     : INT := 0;            (* added new variable *)
END_VAR
    (* ... *)
END_PROGRAM
```

This change is **incompatible** because:
- `counter` changed type from `INT` to `DINT`
- A new variable `log` was added

The system reports the incompatibility and the program must be fully restarted.

## Integration with Monitor Server

Online change can be triggered through the monitor server's WebSocket API using
the `online_change` request type. This allows the VSCode monitor panel (or any
WebSocket client) to push new compiled modules to the running engine. See
[Monitor Server](./monitor-server.md) for the protocol details.
