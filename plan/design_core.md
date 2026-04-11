# IEC 61131-3 Compiler + LSP + Online Debugger — Design Document

> **Progress tracker:** [implementation.md](implementation.md) — checklist and status.
> **See also:** [design_comm.md](design_comm.md) — communication layer design.
> **See also:** [implementation_native.md](implementation_native.md) — LLVM native compilation + hardware targets.
> **See also:** [design_deploy.md](design_deploy.md) — remote deployment & online management.

## Project Overview

A Rust-based IEC 61131-3 Structured Text compiler with LSP support, online debugging
via DAP, a bytecode VM runtime, and a VSCode extension (TypeScript). Architecture
follows the same model as `rust-analyzer`: Rust core process + thin TypeScript
VSCode extension.

---

## Phase Architecture

### Dependency Graph

```
Phase 0 (scaffolding)
  └─► Phase 1 (tree-sitter grammar)
        └─► Phase 2 (AST)
              ├─► Phase 3 (semantics)
              │     └─► Phase 4 (LSP skeleton) ──► Phase 5 (advanced LSP)
              └─► Phase 6 (IR + compiler)
                    └─► Phase 7 (runtime)
                          ├─► Phase 8 (DAP debugger)
                          ├─► Phase 9 (online change)
                          └─► Phase 10 (monitor UI)
Phase 11 (CLI) — can start after Phase 7, grows with each phase
Phase 12 (OOP) — after Phase 7
Communication layer — after Phase 7, see design_comm.md
Native compilation — after Phase 6, see implementation_native.md
Remote deployment — after Phase 8+9+10+11, see design_deploy.md
Phase 16 (retain/persistent) — after Phase 7+15
```

### Phase Summary

| Phase | Scope |
|-------|-------|
| **0** | Project scaffolding, workspace, CI, VSCode extension scaffold |
| **1** | Tree-sitter ST grammar (case-insensitive, incremental) |
| **2** | AST types + CST→AST lowering |
| **3** | Semantic analysis: scopes, types, 30+ diagnostics |
| **4** | LSP server skeleton + VSCode extension (hover, diagnostics, go-to-def, semantic tokens) |
| **5** | Advanced LSP (completion, signature help, rename, formatting, code actions, multi-file workspace) |
| **6** | Register-based IR + AST→IR compiler (50+ instructions) |
| **7** | Bytecode VM + scan cycle engine + stdlib + pointers |
| **8** | DAP debugger (breakpoints, stepping, variables, force/unforce) |
| **9** | Online change manager (hot-reload with variable migration) |
| **10** | WebSocket monitor server + VSCode panel |
| **11** | CLI tool (check, run, serve, debug, compile, fmt, --json) |
| **12** | IEC 61131-3 OOP extensions (CLASS, METHOD, INTERFACE, PROPERTY) |
| **13** | Communication layer (device profiles, simulated + real I/O) — [design_comm.md](design_comm.md) |
| **14** | Native compilation + hardware targets (LLVM, ESP32, STM32, RPi) — [implementation_native.md](implementation_native.md) |
| **15** | Remote deployment & online management (agent, SSH, remote debug/monitor) — [design_deploy.md](design_deploy.md) |
| **16** | RETAIN / PERSISTENT variable persistence (non-volatile storage across restarts) |
| **17** | Singleton enforcement + debug attach to running engine (safety-critical) |

---

## Phase 16: RETAIN / PERSISTENT Variable Persistence

### IEC 61131-3 Semantics

IEC 61131-3 defines three retention classes for variables:

| Qualifier | Power-cycle (warm restart) | Program download (cold restart) |
|-----------|---------------------------|--------------------------------|
| *(none)* | Cleared to initial value | Cleared to initial value |
| `RETAIN` | **Preserved** | Cleared to initial value |
| `PERSISTENT` | Cleared to initial value | **Preserved** |
| `RETAIN PERSISTENT` | **Preserved** | **Preserved** |

- **Warm restart**: runtime process restarts (service restart, power cycle,
  crash recovery). RETAIN variables survive.
- **Cold restart**: new program deployed (online change that changes variable
  layout, or explicit `st-cli target deploy`). PERSISTENT variables survive;
  RETAIN variables are re-initialized.

### Storage Locations

The persistence file location depends on the execution context:

| Context | Retain file path | Discovery method |
|---------|-----------------|------------------|
| **Target host** (agent daemon) | `/var/lib/st-plc/retain/<program>.retain` | Default, configurable via `agent.yaml` |
| **Local development** (st-cli run / DAP debug) | `<project-root>/.st-retain/<program>.retain` | Sibling of `plc-project.yaml`; falls back to CWD |

The `.st-retain/` directory is created on first write. It should be added to
`.gitignore` in project templates.

### Retain File Format

Binary file with a header and a sequence of named variable entries. Using a
name-keyed format (not offset-based) so that the file survives minor program
changes where variable order shifts but names/types are preserved.

```
[Header]
  magic:      4 bytes  "STRT"
  version:    u16      format version (1)
  program:    u16+str  program name (length-prefixed)
  timestamp:  i64      Unix epoch millis when snapshot was taken
  entry_count: u32     number of variable entries

[Entry] × entry_count
  name:       u16+str  fully qualified variable name (e.g., "Main.stats.bottles_filled")
  qualifier:  u8       0=retain, 1=persistent, 2=retain+persistent
  var_type:   u8       VarType discriminant
  int_width:  u8       IntWidth discriminant
  value:      variable-length encoded Value (bool=1B, int/uint/real/time=8B, string=u16+data)
```

### Save/Restore Lifecycle

**Save triggers:**
1. **Clean shutdown** — engine stop, service stop, SIGTERM
2. **Periodic checkpoint** — every N scan cycles (configurable, default 1000)
3. **Before program download** — snapshot taken before online change is applied

**Restore on startup:**
1. Engine reads the retain file before the first scan cycle
2. For each entry in the file, match by name and type against the compiled module
3. If name + type match, inject the value into the VM's initial state
4. Mismatched entries (renamed/retyped variables) are silently skipped
5. Variables not present in the file are initialized to their declared defaults

**Warm restart** (retain file exists, same program):
- Load entries with qualifier `RETAIN` or `RETAIN PERSISTENT`
- Skip entries with qualifier `PERSISTENT` only (these survive cold restart, not warm)

**Cold restart** (new program deployed):
- Load entries with qualifier `PERSISTENT` or `RETAIN PERSISTENT`
- Skip entries with qualifier `RETAIN` only (these are cleared on download)

### Configuration

`plc-project.yaml` extension:
```yaml
engine:
  cycle_time: "10ms"
  retain:
    checkpoint_cycles: 1000     # save every N cycles (0 = only on shutdown)
    path: ".st-retain"          # override retain directory (relative to project root)
```

`agent.yaml` extension:
```yaml
storage:
  retain_dir: /var/lib/st-plc/retain   # default on target host
```

### IR Changes

Add a `persistent` flag to `VarSlot` alongside the existing `retain` flag:
```rust
pub struct VarSlot {
    // ... existing fields ...
    pub retain: bool,
    pub persistent: bool,
}
```

The compiler sets both flags from the parsed `VarQualifier` list:
- `VAR RETAIN` → `retain=true, persistent=false`
- `VAR PERSISTENT` → `retain=false, persistent=true`
- `VAR RETAIN PERSISTENT` → `retain=true, persistent=true`

### Engine Integration

The `Engine` gains a `RetainStore` that handles serialization:
- `RetainStore::save(vm, path)` — snapshot all retain/persistent variables
- `RetainStore::load(path, module) → HashMap<String, Value>` — read back
- `Engine::apply_retained(values)` — inject into VM before first cycle

The DAP and CLI `run` command pass the retain path based on context (project
root for local, `/var/lib/st-plc/retain/` for agent).

---

## LSP Feature Descriptions

### `textDocument/selectionRange`
Smart expand/shrink selection. AST-based nesting: word → expression → statement →
IF/FOR/WHILE body → VarBlock → POU → file. 4 integration tests.

### `textDocument/inlayHint`
Parameter name hints at function/FB call sites for positional arguments (e.g.,
`Add(/*a:*/ 10, /*b:*/ 20)`). Skips named args and args whose text matches the
param name. 3 integration tests.

### `textDocument/onTypeFormatting`
Auto-indent after Enter (increases indent after THEN/DO/VAR/PROGRAM/etc., holds
for END_*) and reindent END_* lines after typing `;`. 4 integration tests.

### `textDocument/callHierarchy`
Full call hierarchy with incoming calls (who calls this?) and outgoing calls (what
does this call?). Resolves across all open documents. Supports FUNCTION, FB,
PROGRAM, and CLASS METHOD. 5 integration tests. Also serves as the cross-reference
view — VS Code renders it as the "Call Hierarchy" panel accessible via
`Shift+Alt+H` or right-click → Show Call Hierarchy.

### `textDocument/linkedEditingRange`
Highlights matching keyword pairs (IF↔END_IF, FOR↔END_FOR, PROGRAM↔END_PROGRAM,
VAR↔END_VAR, etc.) so VS Code can show them linked. Covers all 19 IEC 61131-3
block keyword pairs. AST-aware nesting resolution. 5 integration tests.

---

## Multi-File Infrastructure

### Virtual Coordinate System
`parse_multi()` shifts all byte ranges to a virtual concatenated coordinate system
so diagnostics from file A never appear in file B. LSP + DAP + breakpoints all
use virtual offsets. This was originally causing `ramp_step` warning from
`conveyor.st` to show in `fill_controller.st`.

### Compiler Compilation Order
FBs compiled AFTER their callers had empty locals → all field accesses resolved to
index 0. Fixed by compiling FBs/FUNCTIONs before PROGRAMs in a separate pass.

### VarType::FbInstance Propagation
Compiler now sets proper `VarType::FbInstance(func_idx)` on FB locals instead of
placeholder `VarType::Int`, enabling the debugger to detect and expand FB instances.

### Debugger FB Field Display
`current_locals_with_fb_fields`, `resolve_fb_field`, hierarchical Variables panel
(tree via `variablesReference`), Watch panel Evaluate with expandable FBs.

### PLC Monitor Panel Tree View
Recursive `buildSubTree` + `renderTree` for unlimited nesting depth. Playwright
UI test framework (19 tests).

### Parse Error Quality
Two bugs caused all parse errors to cluster at the end of the file with generic
"syntax error" messages:

1. **Virtual offset not subtracted**: `publish_diagnostics` passed `lower_errors`
   ranges (in virtual concatenated space from `parse_multi`) directly to
   `text_range_to_lsp` without subtracting `virtual_offset`. Semantic diagnostics
   already did this subtraction. Fixed: parse errors now use the same
   `file_start`/`file_end` filtering and offset correction.

2. **Errors invisible on edit**: `document.update()` discarded `lower_errors`
   when parse errors existed (keeping old empty errors). Fixed: `lower_errors`
   and `virtual_offset` are always updated; only AST and semantic analysis are
   preserved from the last good state.

Additionally, `collect_cst_errors` in the lowering pass was improved:
- Errors reported at the **start** of tree-sitter ERROR nodes (where the problem
  actually is) instead of spanning the entire recovery region.
- Squiggles limited to one line for readability.
- Contextual messages based on parent node (`"expected END_IF"`,
  `"unexpected 'filling'"`, `"syntax error in FOR — check DO/END_FOR"`).
- No recursive reporting into ERROR node children (one error per problem).

DAP launch: error message changed from generic `"N parse error(s) found"` to
`"Cannot launch: N parse error(s) — fix the errors shown in the Problems panel
(Ctrl+Shift+M)"`.

---

## Phase 12: OOP Extensions — Design Notes

Full implementation of CLASS, METHOD, INTERFACE, PROPERTY across the entire pipeline:
Grammar → AST → Semantics → Compiler → IR → VM, with multi-file support.

199 new tests covering: grammar parsing, semantic analysis (inheritance, interfaces,
abstract/final, access specifiers, THIS/SUPER), compiler (method compilation, vtable,
inherited vars), runtime (method return values, state persistence, instance isolation,
cross-file calls, pointer integration), and DAP integration.

5 single-file playground examples (10-14) + 1 multi-file OOP project (oop_project/).

### Runtime Bugs Found and Fixed During Playground Testing
- Methods couldn't access class instance variables
- Method return values lost (return_reg protocol mismatch)
- Inherited fields invisible to subclass methods
- Pointer cross-function dereference read wrong frame
- Pointer vs NULL comparison always returned equal
- StoreField unimplemented in compiler + VM
- Nested class instances inside different FB instances shared state

---

## Runtime + Compiler Improvements

### Configurable Cycle Time
`engine.cycle_time` parsed from `plc-project.yaml` via `EngineProjectConfig::from_project_yaml`.
`Engine::run` honors `EngineConfig.cycle_time` — sleeps `target - elapsed` after each cycle
so wall time matches the configured period.

### DAP Interruptible Run Loop
Dedicated reader thread + mpsc channel. `process_inflight_requests` drains the channel
between cycles. Pause / Disconnect / SetBreakpoints take effect mid-run. Continue response
sent BEFORE entering the run loop so VS Code transitions to "running" state immediately.
`resume_execution` takes a `writer` parameter and drains `pending_events` to the wire on
every cycle (live event streaming during Continue).

### Cycle Period + Jitter Tracking
`CycleStats` tracks `last_cycle_period`, `min_cycle_period`, `max_cycle_period`,
`jitter_max` (period = wall-clock between consecutive cycle starts; cycle time = pure
VM execution). `Engine.run_one_cycle` and `step_one_dap_iteration` both measure period
via `previous_cycle_start` Instant. Jitter reset on Halt so user think-time doesn't
pollute the measurement.

### IntWidth + Two's Complement Overflow
`IntWidth` enum (I8/U8/I16/U16/I32/U32/I64/U64/None) on `VarSlot`. `narrow_value()`
applies two's complement wrapping at every store boundary: `local_set`, `set_global`,
`set_global_by_slot`, `StoreGlobal`, `force_variable`. Add/Sub/Mul use
`i64::wrapping_*` so debug builds don't panic.

### Literal Context Typing
`cycle : SINT := 0` and `cycle := cycle + 1` work without SINT# prefix. The semantic
checker allows integer literals to narrow to the assignment target when the value fits
(matching Codesys/TwinCAT behavior). `integer_type_range()`, `literal_fits_in_target()`,
`integer_literal_value()` in st-semantics. Out-of-range literals still error.

### Force Variable Semantics
`forced_global_slots: HashSet<u16>` on Vm. `set_global_by_slot`, `set_global`,
`StoreGlobal` all skip writes to forced slots. `force_variable` writes the forced
value INTO the slot so every reader sees it naturally. Forced values narrowed by
IntWidth. Monitor panel shows lock icon + orange value.

### Global Variable Initialization
Compiler generates a synthetic `__global_init` function containing one `StoreGlobal`
per `VAR_GLOBAL` initializer. Engine calls `vm.run_global_init()` once at construction.

### Pause Button Fix
Two bugs prevented the Pause button from working during Continue:

1. **`resume_with_source` cleared pending pause**: At the start of each fresh scan
   cycle, `step_one_dap_iteration` called `resume_with_source(mode, 0, 0)` which
   unconditionally set `step_mode = Continue` and `paused = false` — overwriting
   the `step_mode = Paused` that `vm.debug_mut().pause()` had set. Fixed: skip
   `resume_with_source` if `step_mode` is already `Paused`.

2. **Free-running loop starved reader thread**: Without `cycle_time`, the run loop
   spun CPU-bound with `yield_now()` every 64 cycles — too weak for the reader
   thread to deliver the Pause request. Fixed: the run loop now always uses
   `interruptible_sleep` with a default 1ms period when no `cycle_time` is
   configured. `interruptible_sleep` polls the channel in 10ms chunks, so Pause
   and Disconnect arrive within 10ms regardless of configuration.

### Monitor Panel Session Reset
When a debug session is stopped and restarted, the monitor panel's `valueMap` kept
stale data from the previous session. Since the variable keys didn't change, the
`valueMap.size` check never triggered a table rebuild, and values appeared frozen.

Fixed: `updateCatalog` (triggered by the `plc/varCatalog` event on each new session)
sends a `resetSession` message to the webview that clears `valueMap` and `childrenMap`.
The first telemetry tick triggers a full rebuild. Additionally, `PlcDapTracker` detects
empty variable arrays in telemetry and retries `sendWatchListToDap` to handle the
case where the initial send fires before the session is fully active.

---

## Cycle-Time Feedback

Live, glanceable feedback about scan cycle health using DAP custom events + native
VS Code primitives.

**Tier 1 — scanCycleInfo + real cycle stats**: DAP owns its own `CycleStats`, times
each cycle in the refactored run loop. `handle_cycle_info` reports real metrics.

**Tier 2 — live status bar**: DAP emits `plc/cycleStats` every N cycles via
`output` events with `category: telemetry`. VS Code extension subscribes via
`registerDebugAdapterTrackerFactory`. StatusBarItem renders cycle stats with
color-coded watchdog margin. `cycle_event_interval` targets ~500ms between updates
regardless of cycle period.

**Tier 3 — dedicated "PLC Scan Cycle" tree view**: `contributes.views` under the
`debug` container. `TreeDataProvider` fed from `plc/cycleStats`. Rows: cycle count,
timing stats, per-device connection status.

**Tier 4 — CodeLens + watchdog Diagnostic**: CodeLens above each POU header showing
timing. When `last_us > budget`, push Warning diagnostic onto the POU header line.

**Tier 5 — MonitorPanel sparkline**: Rolling sparkline (last 300 cycles), histogram
(10µs buckets), max/watchdog markers in the Monitor panel.

**Tier 6 — per-POU profiling (stretch)**: VM tracks per-POU `call_count` +
`total_time_ns`. DAP custom event `plc/poStats`. CodeLens + "Top POUs" table.

**Tier 7 — watchdog breakpoint (stretch)**: `launch.json` option
`"breakOnWatchdog": true`. DAP emits `Stopped` on overrun.

---

## Live Variable Monitor

### Subscription Model

DAP `DapSession.watched_variables` — telemetry only ships values for variables in
the watch list. Evaluate REPL commands: `addWatch`, `removeWatch`, `watchVariables`,
`clearWatch`, `varCatalog`. Each mutation triggers an immediate telemetry push.

`Vm::monitorable_catalog()` enumerates globals + PROGRAM locals from the module schema
(complete even before first cycle). `Vm::monitorable_variables()` returns globals +
PROGRAM retained locals namespaced as `Main.x`, `Pump.speed`, etc.

### Monitor Panel UX

Full rewrite using `postMessage` for incremental DOM updates. Watch list table with
autocomplete, per-row Force/Remove, "Clear all". Per-workspace persistence via
`workspaceState`. Force/Unforce wires to DAP's `evaluate("force x = 42")` REPL.

### Siemens TIA Portal-Style Watch Tables (Future)

In TIA Portal, a "Watch table" is a named collection of variables with per-row
metadata (display format, comment, modify value, trigger). Users create multiple
tables for different subsystems and switch between them in tabs.

**WatchTable schema:**
```ts
interface WatchTableEntry {
  name: string;             // ST variable name (e.g. "io_rack.DI_0")
  comment?: string;         // user-supplied annotation
  displayFormat?: "dec" | "hex" | "bin" | "bool" | "ascii" | "float";
  modifyValue?: string;     // pre-typed value for one-click force
  triggerExpression?: string; // optional: only show/capture when true
}
interface WatchTable {
  name: string;
  entries: WatchTableEntry[];
  description?: string;     // shown as a tooltip on the tab
}
```

Features: tab strip (new/rename/duplicate/delete), comment column, display format
selector, modify column with one-click force, import/export to `.plc-watch.json`,
TIA Portal `.tww` import, trigger expressions, snapshot/compare, charting view.

### Hierarchical FB Instance Display

When the user watches a FB instance, it displays as a collapsible tree — parent node
shows instance name + type, expanding reveals each field with its live value.

**DAP Variables panel**: FB instance locals returned with `variablesReference > 0`.
`fb_var_refs` HashMap maps ref IDs to `(caller_id, slot_idx, fb_func_idx)`. Nested
FBs get their own refs for recursive expansion. Evaluate handler resolves dotted
paths via `resolve_fb_field`.

**Monitor panel tree**: Recursive `buildSubTree()` + `renderTree()` with unlimited
nesting depth. Telemetry sends nested `children` arrays for expanded FB instances.

**Catalog enhancement**: `plc/varCatalog` should include `childNames: [{name, type}]`
for FB-typed entries so the panel knows the tree structure before values arrive.

---

## VS Code Extension E2E Testing

Three-layer testing strategy for the VS Code extension:

### Layer 1: @vscode/test-electron (14 tests — real Electron instance)
Launches an actual VS Code window with the extension installed, opens a workspace,
and exercises features through the VS Code API. Tests run in the extension host
process with full access to `vscode.*` APIs. Run via `npm test` with Xvfb.

**Extension tests (5)**: Language registration, `.st` file recognition, extension
activation, LSP diagnostics for undeclared variables, syntax highlighting.

**Debug button tests (9)**: Launch with stopOnEntry, Step In, Step Over, multiple
steps, Continue → Pause (verifies counter advances during run), Evaluate while
paused, Stop, Stop during Continue, Breakpoint hit at correct line. Test fixture
is a temp file in `/tmp` (no "Save As" dialogs). `pollUntilStopped` uses
`customRequest("stackTrace")` polling for reliable stop detection.

**Key patterns**:
- `forceStopSession()` in setup/teardown ensures clean state between tests
- `pollUntilStopped()` polls stackTrace every 150ms until frames appear
- Pause uses `onDidChangeActiveStackItem` event + `interruptible_sleep` (1ms default)
- All test documents are file-backed (not untitled) to avoid Save dialogs

**Limitations**: Cannot inspect webview DOM (sandboxed). Cannot reliably assert
on status bar text (no API). Use Playwright for those.

### Layer 2: Playwright (21 tests — webview DOM testing)
Tests the PLC Monitor panel's HTML/JS logic in a real browser via a local HTTP
server. The test fixture (`monitor-panel-visual.html`) mirrors the webview JS,
fed with mock telemetry data. Covers: watch list CRUD, FB tree expand/collapse,
nested children from telemetry, value updates, expand/collapse persistence,
session reset (values clear and rebuild).

### Layer 3: Unit tests (pure JS, no dependencies)
`monitor-tree.test.js` — 10 tests for the tree builder logic in isolation.

### CI integration
Both frameworks run headless. `@vscode/test-electron` needs Xvfb on Linux
(or `--disable-gpu`). Playwright uses headless Chromium. A combined script
runs Rust workspace tests → Electron extension tests → Playwright UI tests.

---

## Cross-Cutting Concerns

### Testing
714+ tests across 10+ crates — unit, integration, LSP protocol, DAP protocol,
WebSocket, end-to-end, Playwright UI.

### CI/CD
GitHub Actions (check, test, clippy, audit, cargo-deny, docs deploy), release-plz
for semver.

### Documentation
mdBook site (20+ pages) with architecture, tutorials, language reference, stdlib docs.

### Tracing / Logging
DAP server logs to stderr + Debug Console, `tracing` crate available throughout.

### Devcontainer
Full VSCode dev environment with auto-build, extension install, playground.

### Error Quality
Line:column source locations, severity levels, diagnostic codes.