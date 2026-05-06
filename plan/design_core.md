# IEC 61131-3 Compiler + LSP + Online Debugger — Design Document

> **Progress tracker:** [implementation_core.md](implementation_core.md) — checklist and status.
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
| **18** | Unified HTTP monitoring — cycle stats, watch variables, force/unforce via HTTP API (local + remote) |

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

---

## Phase 16 RETAIN / PERSISTENT — Test architecture

The retain pipeline is end-to-end, so the test surface is layered to
match the data flow. Each layer pins behaviour against real
collaborators (no mocks) and is checklisted in
[implementation_core.md](implementation_core.md).

| Layer | What it locks | Where |
|-------|---------------|-------|
| Engine unit/integration | Capture/restore semantics across globals, locals, struct fields, FB instance fields | `st-engine/tests/retain_tests.rs` |
| HTTP/WS acceptance | Catalog flag plumbing, force/save/load/restore through the agent's real axum router | `st-target-agent/tests/retain_e2e.rs` |
| DAP wire | `presentationHint.attributes` shape on `Variable` rows | `st-dap/tests/dap_integration.rs::test_retain_persistent_presentation_hint` |
| Systemd-style E2E (gated) | Deploy, force, kill+restart agent, verify on-disk snapshot survives | `st-target-agent/tests/e2e_qemu.rs::e2e_x86_64_retain_across_restart` |
| Visual UI (Playwright) | Badge label / tooltip / colour propagation in the real Preact bundle | `editors/vscode/test/ui/retain-badge.spec.js` |

**Children of retained FB / struct slots inherit the parent's qualifier.**
This is the IEC convention; capture (`retain_store::capture_snapshot`)
walks instance fields under any retained slot, so the DAP and monitor
surfaces propagate the badge to every child for visual consistency.

**`RuntimeManager::new_with_retain_dir(...)`** exists primarily so tests
can redirect the snapshot directory away from `/var/lib/st-plc/retain`
and run unprivileged. It is also used by the agent server when the
operator overrides `storage.retain_dir` in `agent.yaml`.

### Deferred retain test paths

Two `retain_store.rs` branches are not reachable from any system-level
entry point and would require unit-level construction or unstable OS
manipulation to exercise. They remain deferred:

- **`restore_snapshot(_, _, warm=false)` (cold restart):** the agent
  always calls the warm variant on engine start. Cold restart is
  modelled as "redeploy a different program", which today flows through
  `online_change` / `Engine::new` — both still warm. A real cold-restore
  invocation would need an explicit "factory reset" or "load from
  PERSISTENT-only snapshot" entry point that doesn't exist yet. When
  that surface lands, an E2E test against it becomes the natural place
  to lock the cold path; until then, unit-level coverage is the only
  option, which we've explicitly excluded for this round.
- **`save_to_file` filesystem-error branches** (write/rename failure on
  read-only FS or out-of-space): exercising these requires either
  filling a tmpfs to capacity or chmod 000 on a directory the test
  process owns. Both are too OS-fragile for CI and the failure modes
  are well-understood `std::io::Error` propagations with low
  real-world risk. Re-evaluate only if a production incident points to
  silent retain-save failure.

---

## Test Coverage Strategy

Coverage is tracked per-file in the `Test Coverage Improvements`
checklist of [implementation_core.md](implementation_core.md). The
strategy that produced the checklist:

### Tier 1 — High-ROI targeted tests (Done, 2026-04-23)

Cheap, mechanical wins. Subprocess instrumentation in `lsp_integration`
unblocked `st-lsp/src/server.rs` (0 % → 58.6 %). Adding
`st-comm-modbus` and `st-comm-serial` to the `show-env` workflow
reclaimed ~1.4 k lines of comm-stack coverage that had been silently
excluded. Two file-local hot spots (`watchdog.rs`, `document.rs`)
absorbed targeted unit tests because their domains (timer maths,
offset/position arithmetic) are pure-function-shaped and don't benefit
from going through the agent.

### Tier 2 — Acceptance/E2E coverage (Done)

The four files in this tier are integration boundaries (HTTP API,
WebSocket, DAP wire, retain pipeline). Their bugs surface at the user
edge, so the tests live there too: real axum router, real WebSocket
client, real TCP DAP proxy, real Preact bundle in a real browser. No
mocks. The user gives up some speed (process spin-up, multipart
construction, JSON framing) in exchange for the ability to catch
regressions that only manifest end-to-end.

### Tier 3 — External-dependency gated

Modbus-TCP and SSH/installer paths need real network or VM
collaborators. The QEMU harness in `tests/e2e-deploy/vm/` is the
intended host; tests are gated by `ST_E2E_QEMU=1` so default `cargo
test` stays fast.

### Tier 4 — Infrastructure refactor

Before more tests can usefully target `st-cli/src/main.rs`'s 907
uncovered lines, the binary needs to be split into a thin shim over a
library entrypoint. That change is a precondition, not a coverage
problem.

### Wait-and-see

Two files (`st-engine/src/vm.rs`, `st-target-agent/src/runtime_manager.rs`)
were parked under wait-and-see at the 2026-04-23 baseline. The 2026-05-05
re-measurement found:

- **`vm.rs`** is now 36.4 % (was 78.4 %). The drop is structural
  rather than regressive: the file roughly doubled in executable-line
  count when the Tier 5 string opcodes and Tier 1-4 time/date opcodes
  landed. Each new opcode adds a `match Instruction::*` arm which
  llvm-cov counts as an independent line/region. Every opcode IS
  exercised somewhere by `stdlib_tests` / `string_tests` (170+ tests,
  all passing), so the percentage drop overstates the real coverage
  loss. Adding more dedicated VM unit tests for rare opcodes is
  diminishing returns. Re-evaluate only when a real regression slips
  through.
- **`runtime_manager.rs`** is now 24.0 % (was 70.3 %). The new cold
  spot is `handle_debug_commands` — the paused-state DAP dispatcher
  for StepIn / StepOver / StepOut / Evaluate / ClearBreakpoints / the
  new-attach-while-paused swap branch / the 30-minute idle timeout.
  This path is reachable from the real DAP proxy (no mocks needed)
  and is on a hot user-facing surface (live debugging), so it gets
  follow-up E2E tests rather than wait-and-see treatment.

### Limits of the no-mocking rule

Some branches genuinely cannot be reached from a real client without
unbounded test runtime or hardware manipulation. When this happens we
either:

1. Document the path here as deferred, with the precise reason
   ("requires `RecvTimeoutError::Timeout` after 30 minutes",
   "requires read-only `/var/lib/st-plc/retain`"), or
2. Add the missing system-level entry point (e.g.
   `RuntimeManager::new_with_retain_dir`) so tests can drive the
   real code without resorting to mocks.

The deferred list is part of the coverage tracker, not a TODO — it's
where we acknowledge the cost/value trade-off and stop.

### `runtime_manager::handle_debug_commands` — paused-state coverage map

The dispatcher's branches map to user-driven actions as follows. All
reachable arms are exercised by acceptance tests in
`dap_proxy_integration.rs`; the unreachable ones are documented with
the constraint that prevents E2E coverage.

| Branch | Reachable via | Test |
|--------|---------------|------|
| `DebugCommand::Continue` | DAP `continue` | `test_dap_attach_pause_resume_reattach_lifecycle`, `test_dap_attach_clear_breakpoints_while_paused` |
| `DebugCommand::StepIn` | DAP `stepIn` | `test_dap_attach_step_in_while_paused` |
| `DebugCommand::StepOver` | DAP `next` | `test_dap_attach_step_over_while_paused` |
| `DebugCommand::StepOut` | DAP `stepOut` | `test_dap_attach_step_out_while_paused` |
| `DebugCommand::GetVariables` | DAP `variables(scope_ref)` | `test_dap_attach_variables_for_fb_fields` |
| `DebugCommand::GetStackTrace` | DAP `stackTrace` | `test_dap_attach_stacktrace_inside_fb_body` |
| `DebugCommand::Evaluate` | DAP `evaluate` | `test_dap_attach_evaluate_while_paused` |
| `DebugCommand::SetBreakpoints` | DAP `setBreakpoints` | `test_dap_attach_breakpoint_in_helper_file` |
| `DebugCommand::ClearBreakpoints` | DAP `setBreakpoints` with empty array | `test_dap_attach_clear_breakpoints_while_paused` |
| `DebugCommand::Pause` (re-pause) | n/a | trivial no-op, not separately tested |
| `DebugCommand::Disconnect` | DAP `disconnect` | `test_dap_attach_to_running_engine`, `test_dap_attach_pause_resume_reattach_lifecycle` |
| `RuntimeCommand::ForceVariable` (paused) | **deferred** — dispatcher races with `recv_timeout`, see "known runtime limitation" below |
| `RuntimeCommand::ForceVariable` (around session) | force before pause, verify across pause+disconnect | `test_dap_attach_force_around_debug_session` |
| `RuntimeCommand::UnforceVariable` (around session) | unforce after resume | covered indirectly by force-around test |
| `RuntimeCommand::Stop` | POST `/api/v1/program/stop` while paused | `test_dap_attach_http_stop_while_paused` |
| `RuntimeCommand::Shutdown` | (agent shutdown) | covered indirectly by `Drop` paths |
| `RuntimeCommand::DebugDetach` | (proxy-internal) | n/a — see "swap" note below |
| `RuntimeCommand::DebugAttach` (swap) | second TCP client | rejected at proxy layer, see "second-connection" test |
| `RuntimeCommand::OnlineChange` while paused | POST `/api/v1/program/update` while paused | rejected (returns "Cannot apply online change while debug session is paused") — covered indirectly |
| `RecvTimeoutError::Timeout` (30 min) | **deferred** — would require >30 min CI runtime |
| `RecvTimeoutError::Disconnected` | TCP drop without Disconnect | `test_dap_attach_tcp_drop_releases_engine` |

**Swap branch (deferred at the proxy level).** The
`RuntimeCommand::DebugAttach` arm of `handle_debug_commands` swaps a
new debug session for the currently paused one. Today's `dap_proxy`
implementation rejects a second TCP connection at the proxy layer
(single-session lock), so the swap branch never runs in production.
`test_dap_attach_second_connection_rejected_while_first_paused` pins
the proxy-layer behaviour we DO have. To exercise the underlying swap
arm, the proxy would need a multi-client mode (planned with the OPC-UA
session work) or the runtime would need an in-process attach API.
Until then this is a documented dead-but-defensive code path.

**30-minute idle timeout (deferred).** Reaching
`RecvTimeoutError::Timeout` requires a 30-minute paused session with
no commands. CI cost > value: a regression here would only fire when
a developer forgot a debug session, and the failure mode (engine
keeps cycling normally after the timeout) is conservative. Re-evaluate
only if the timeout itself becomes adjustable via config and we want to
test the config end-to-end.

**`RuntimeCommand::ForceVariable` while paused — known runtime
limitation.** The dispatcher's `try_recv` does handle ForceVariable
and sends the oneshot reply, but the surrounding `loop` iterates only
*after* `session.cmd_rx.recv_timeout(timeout)` returns. If a debug
session is paused with no DAP traffic, an HTTP `/api/v1/variables/force`
arriving during the pause queues into `runtime_cmd_rx` but nobody runs
`try_recv` until the next DAP command arrives (or the 30-min timeout
fires). The HTTP handler is awaiting a oneshot reply, so it stalls —
and any test that drives both sides synchronously inside the same
spawn_blocking thread will deadlock.

In production this is a latency bug, not a correctness bug: as soon as
the user sends *any* DAP command (typically Continue or a follow-up
inspection), the queued force is processed and the HTTP reply lands.
The fix on the runtime side is to use `tokio::select!` (or a matching
`crossbeam::select!`) to wait on both channels simultaneously, but
that's a refactor of the dispatcher, not test work. Until that lands:

- The `RuntimeCommand::ForceVariable` arm of `handle_debug_commands`
  remains documented but unexercised by the no-mocking E2E suite.
- Force functionality during a debug session is covered by
  `test_dap_attach_force_around_debug_session` (force *before* pause,
  verify pause+disconnect doesn't drop the override).
- Re-add a paused-state force test once the dispatcher uses `select!`
  so HTTP no longer races with `recv_timeout`.