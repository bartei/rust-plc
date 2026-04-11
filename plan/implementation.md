# IEC 61131-3 Compiler + LSP + Online Debugger — Progress Tracker

> **Design document:** [design_core.md](design_core.md) — architecture, phase descriptions, design notes.
> **See also:**
> - [implementation_comm.md](implementation_comm.md) — communication layer progress
> - [design_comm.md](design_comm.md) — communication layer design
> - [implementation_native.md](implementation_native.md) — LLVM native compilation + hardware targets
> - [implementation_deploy.md](implementation_deploy.md) — remote deployment & online management
> - [design_deploy.md](design_deploy.md) — remote deployment design

---

## Phases 0-11: Core Platform (COMPLETED)

714+ tests, zero clippy warnings.

| Phase | Scope | Status |
|-------|-------|--------|
| **0** | Project scaffolding, workspace, CI, VSCode extension scaffold | Done |
| **1** | Tree-sitter ST grammar (case-insensitive, incremental) | Done |
| **2** | AST types + CST→AST lowering | Done |
| **3** | Semantic analysis: scopes, types, 30+ diagnostics | Done |
| **4** | LSP server skeleton + VSCode extension | Done |
| **5** | Advanced LSP (completion, signature help, rename, formatting, code actions) | Done |
| **6** | Register-based IR + AST→IR compiler (50+ instructions) | Done |
| **7** | Bytecode VM + scan cycle engine + stdlib + pointers | Done |
| **8** | DAP debugger (breakpoints, stepping, variables, force/unforce) | Done |
| **9** | Online change manager (hot-reload with variable migration) | Done |
| **10** | WebSocket monitor server + VSCode panel | Done |
| **11** | CLI tool (check, run, serve, debug, compile, fmt, --json) | Done |

### Multi-file IDE support
- [x] LSP: project-aware analysis (discovers plc-project.yaml)
- [x] LSP: cross-file go-to-definition
- [x] LSP: cross-file type resolution
- [x] DAP: multi-file project loading and compilation
- [x] DAP: per-file source mapping for stack traces
- [x] DAP: breakpoints work in any project file
- [x] DAP: step-into crosses file boundaries
- [x] DAP: Initialized event after Launch (per DAP spec)
- [x] JSON Schema for plc-project.yaml and device profiles

### LSP features
- [x] `textDocument/selectionRange` — smart expand/shrink selection
- [x] `textDocument/inlayHint` — parameter name hints at call sites
- [x] `textDocument/onTypeFormatting` — auto-indent after Enter
- [x] `textDocument/callHierarchy` — incoming/outgoing calls (cross-reference view)
- [x] `textDocument/linkedEditingRange` — matching keyword pair highlights

### Multi-file infrastructure fixes
- [x] Diagnostic routing via virtual concatenated coordinate system — *see [design](design_core.md#virtual-coordinate-system)*
- [x] Compiler FB field index bug (compilation order fix)
- [x] `VarType::FbInstance` propagation to debugger
- [x] Debugger FB field display (hierarchical Variables panel)
- [x] PLC Monitor panel tree view (Playwright UI test framework, 21 tests)
- [x] Parse error locations: virtual offset subtraction in LSP `publish_diagnostics`
- [x] Parse error quality: contextual messages, errors at start of ERROR nodes
- [x] Parse errors visible on edit (lower_errors always updated in `document.update`)
- [x] DAP launch error points to Problems panel instead of generic dialog

### Remaining
- [ ] Online change: DAP custom request + VSCode toolbar

---

## Phase 12: OOP Extensions (COMPLETED)

> Design notes: [design_core.md § Phase 12](design_core.md#phase-12-oop-extensions--design-notes)

199 new tests. 5 single-file playground examples + 1 multi-file OOP project.

- [x] CLASS, METHOD, INTERFACE, PROPERTY across full pipeline
- [x] Grammar → AST → Semantics → Compiler → IR → VM
- [x] Multi-file support
- [x] 7 runtime bugs found and fixed during playground testing

### Remaining
- [ ] Constructor/destructor support (FB_INIT / FB_EXIT pattern)
- [ ] Online change compatibility with classes

---

## Runtime + Compiler Improvements (COMPLETED)

> Design notes: [design_core.md § Runtime + Compiler Improvements](design_core.md#runtime--compiler-improvements)

- [x] Configurable `engine.cycle_time` from plc-project.yaml
- [x] `Engine::run` honors cycle_time with sleep
- [x] DAP interruptible run loop (reader thread + mpsc channel)
- [x] Removed 100k-cycle hard cap; 10M safety net for tests
- [x] Cycle period + jitter tracking (`last/min/max_cycle_period`, `jitter_max`)
- [x] `avg_cycle_time` overflow fix (u128 division)
- [x] `scope_refs` leak fix (cleared on resume)
- [x] Continue response sent before blocking run loop (play/pause button fix)
- [x] Live event streaming during Continue (writer passed into run loop)
- [x] Pause button fix: `resume_with_source` no longer clears pending pause flag
- [x] Default 1ms cycle period when no `cycle_time` configured (Pause works reliably)
- [x] Monitor panel session reset: `valueMap`/`childrenMap` cleared on new session
- [x] Watch list resync: tracker retries `sendWatchListToDap` on empty telemetry
- [x] IntWidth enum + two's complement wrapping at every store boundary
- [x] Literal context typing in semantic checker
- [x] Force variable: `forced_global_slots` HashSet, narrowing, lock icon, type validation
- [x] Global variable initialization via synthetic `__global_init` function

---

## Cycle-Time Feedback

> Design: [design_core.md § Cycle-Time Feedback](design_core.md#cycle-time-feedback)

### Tier 1 — scanCycleInfo + real cycle stats
- [x] DAP session owns `CycleStats`, times each cycle in run loop
- [x] `handle_cycle_info` reports real metrics

### Tier 2 — live status bar
- [x] DAP emits `plc/cycleStats` every N cycles via telemetry events
- [x] VS Code extension subscribes via `registerDebugAdapterTrackerFactory`
- [x] StatusBarItem with cycle stats + color-coded watchdog margin
- [x] Status bar tooltip shows target/period/jitter
- [x] Hide when no `st`-type debug session active

### Tier 3 — PLC Scan Cycle tree view
- [ ] `contributes.views` under `debug` container
- [ ] `TreeDataProvider` fed from `plc/cycleStats`
- [ ] Rows: cycle count, timing stats, per-device connection leaves

### Tier 4 — CodeLens + watchdog Diagnostic
- [ ] CodeLens above each POU header showing timing
- [ ] Watchdog budget from `plc-project.yaml` (`engine.watchdog_ms`)
- [ ] Warning diagnostic when `last_us > budget`

### Tier 5 — MonitorPanel sparkline
- [ ] "Cycle time" card in Monitor panel
- [ ] Rolling sparkline (300 cycles), histogram, max/watchdog markers

### Tier 6 — per-POU profiling (stretch)
- [ ] VM tracks per-POU `call_count` + `total_time_ns`
- [ ] DAP custom event `plc/poStats`
- [ ] CodeLens upgraded to per-POU timing
- [ ] "Top POUs by time" table in Monitor panel

### Tier 7 — watchdog breakpoint (stretch)
- [ ] `launch.json` option `"breakOnWatchdog": true`
- [ ] DAP emits `Stopped` on overrun

---

## Live Variable Monitor

> Design: [design_core.md § Live Variable Monitor](design_core.md#live-variable-monitor)

### Subscription model + watch list
- [x] `DapSession.watched_variables` — telemetry only ships watched values
- [x] REPL commands: `addWatch`, `removeWatch`, `watchVariables`, `clearWatch`, `varCatalog`
- [x] Immediate telemetry push on each watch mutation
- [x] `Vm::monitorable_catalog()` and `Vm::monitorable_variables()`
- [x] `plc/varCatalog` telemetry event on launch

### Monitor panel UX
- [x] `postMessage`-based incremental DOM updates
- [x] Watch list table with autocomplete, Force, Remove, Clear all
- [x] Per-workspace persistence via `workspaceState`
- [x] Force/Unforce wired to DAP evaluate REPL
- [x] Live cycle stats display
- [x] Tests: `test_watch_list_flow`, `test_var_catalog_emitted_on_launch`

### Watch Tables (future)
- [ ] Multiple named watch tables with tab strip
- [ ] Per-table persistence (key: `plcMonitor.watchTables:<workspace>`)
- [ ] Comment column (editable inline, persisted on blur)
- [ ] Display format selector per row (dec/hex/bin/bool/ascii/float)
- [ ] Modify column: one-click force to pre-configured value
- [ ] Tab management: new / rename / duplicate / delete / drag-reorder
- [ ] Import/export to `.plc-watch.json`
- [ ] TIA Portal `.tww` import
- [ ] DAP v2 wire protocol (`watchVariablesV2` with display preferences)
- [ ] Trigger expressions (boolean ST expression, sample only when true)
- [ ] Snapshot / Compare (capture + side-by-side diff)
- [ ] Charting view (sparkline / line chart for numeric variables)
- [ ] Documentation: `docs/src/cli/watch-tables.md`

### Hierarchical FB instance display
- [x] DAP: FB locals with `variablesReference > 0` for tree expansion
- [x] `fb_var_refs` HashMap for ref ID → `(caller_id, slot_idx, fb_func_idx)`
- [x] Nested FB recursive expansion
- [x] Parent FB summary value
- [x] Evaluate handler resolves dotted paths via `resolve_fb_field`
- [x] DAP integration tests (3 tests)
- [x] Evaluate handler: `variablesReference > 0` for FB instances in Watch panel
- [x] Monitor panel: recursive `buildSubTree()` + `renderTree()` tree view
- [x] Monitor panel: tree data model (flat → WatchEntry with children)
- [x] Monitor panel: telemetry sends nested `children` for expanded FBs
- [x] Monitor panel: persist expand/collapse state in workspace state
- [ ] Monitor panel: "Collapse all" / "Expand all" for large FB instances
- [ ] `plc/varCatalog`: add `childNames` for FB-typed entries
- [ ] Tests: DAP tree expansion (single + nested FB)
- [ ] Tests: Monitor panel tree renders with correct expand/collapse
- [ ] Tests: tree state persists across panel close/reload
- [ ] Tests: performance — FB with 50+ fields doesn't bloat telemetry

---

## VS Code Extension E2E Testing

> Design: [design_core.md § VS Code Extension E2E Testing](design_core.md#vs-code-extension-e2e-testing)

Infrastructure: `@vscode/test-electron` (real Electron instance) + Playwright (webview DOM).
14 Electron tests, 21 Playwright tests, 10 unit tests.

### Extension tests (via @vscode/test-electron) — 5 passing
- [x] ST language registered
- [x] `.st` files recognized as Structured Text
- [x] Extension activates on `.st` file
- [x] Diagnostics appear for undeclared variable
- [x] Syntax highlighting provides tokens

### Debug button tests (via @vscode/test-electron) — 9 passing
- [x] Launch with `stopOnEntry` pauses at first statement
- [x] Step In advances to next line
- [x] Step Over advances without entering calls
- [x] Multiple Step Ins progress through the program
- [x] Continue → Pause stops execution (counter advances during run)
- [x] Evaluate expression while paused returns correct value
- [x] Stop terminates the session
- [x] Stop during Continue terminates cleanly
- [x] Breakpoint hit stops at correct line

### Playwright webview tests — 21 passing
- [x] Watch list CRUD (add, remove, clear)
- [x] FB tree expand/collapse with nested FBs
- [x] Tree from telemetry `children` array
- [x] Value updates without rebuilding structure
- [x] Expand/collapse state persistence
- [x] Session reset: values clear and rebuild on new session

### Remaining E2E tests
- [ ] Hover shows type information on variables
- [ ] Go-to-definition navigates to symbol
- [ ] Force/unforce variable via custom request
- [ ] Multi-file project: breakpoints across files
- [ ] `structured-text.openMonitor` command opens the panel
- [ ] Headless CI via Xvfb in GitHub Actions

---

## Phase 16: RETAIN / PERSISTENT Variable Persistence

> Design notes: [design_core.md § Phase 16](design_core.md#phase-16-retain--persistent-variable-persistence)

Non-volatile storage for RETAIN and PERSISTENT variables across runtime restarts
and program downloads, per IEC 61131-3 semantics.

### IR + Compiler

- [ ] Add `persistent: bool` field to `VarSlot` (alongside existing `retain`)
- [ ] Compiler: set `retain` and `persistent` from `VarQualifier` list
- [ ] Compiler: support combined `VAR RETAIN PERSISTENT` qualifier
- [ ] Semantic checker: validate retain/persistent only on VAR_GLOBAL and PROGRAM locals

### Retain file format + serialization

- [ ] Define binary retain file format (header + named entries)
- [ ] `RetainStore::save(vm, path)` — serialize retain/persistent variables
- [ ] `RetainStore::load(path, module)` — deserialize and match by name+type
- [ ] Handle version migration (skip mismatched entries gracefully)
- [ ] Unit tests: round-trip save/load for all Value types

### Engine integration

- [ ] `Engine::apply_retained(values)` — inject into VM before first scan cycle
- [ ] Save on clean shutdown (SIGTERM / engine stop)
- [ ] Periodic checkpoint every N cycles (configurable, default 1000)
- [ ] Snapshot before online change / program download
- [ ] Warm restart: load RETAIN + RETAIN PERSISTENT entries
- [ ] Cold restart: load PERSISTENT + RETAIN PERSISTENT entries
- [ ] Integration tests: values survive engine restart

### Storage location resolution

- [ ] Target host: default `/var/lib/st-plc/retain/<program>.retain`
- [ ] Local dev: `<project-root>/.st-retain/<program>.retain` (sibling of plc-project.yaml)
- [ ] Fallback: CWD when no project file found
- [ ] CLI `run` / DAP `launch` resolve and pass retain path to engine
- [ ] Agent resolves retain path from `agent.yaml` config
- [ ] Add `.st-retain/` to template project `.gitignore`

### Configuration

- [ ] `plc-project.yaml`: `engine.retain.checkpoint_cycles` (default 1000)
- [ ] `plc-project.yaml`: `engine.retain.path` (override retain directory)
- [ ] `agent.yaml`: `storage.retain_dir` (default `/var/lib/st-plc/retain`)
- [ ] JSON schema updates for both config files

### Remaining

- [ ] DAP: show retain/persistent badge in Variables panel
- [ ] Monitor panel: indicate retain/persistent variables visually
- [ ] Documentation: `docs/src/language/retain-persistent.md`
- [ ] E2E test: QEMU target — deploy, run, stop service, restart, verify values preserved

---

## Phase 17: Singleton Enforcement + Debug Attach to Running Engine (SAFETY-CRITICAL)

> Design notes: [design_core.md § Phase 17](design_core.md#phase-17-singleton-enforcement--debug-attach-to-running-engine)

Two instances controlling the same physical I/O can cause machinery damage or
personal injury. The debugger must attach to the running engine, not spawn a
second VM. This is the most safety-critical feature in the system.

### Phase A — Singleton enforcement

- [ ] PID file with `flock(LOCK_EX | LOCK_NB)` at `/run/st-runtime/st-runtime.pid`
- [ ] `SingletonGuard` RAII struct: holds lock, removes PID file on drop
- [ ] Integrate into `st-runtime agent` startup — exit with clear error if locked
- [ ] Systemd unit hardening: `StartLimitBurst=5`, `StartLimitIntervalSec=30`, `RuntimeDirectory=st-runtime`
- [ ] Unit tests: acquire/double-acquire/drop-releases/stale-file (4 tests)

### Phase B — Handle VmError::Halt as debug pause

- [ ] Add `RuntimeStatus::DebugPaused` to runtime status enum
- [ ] Restructure `run_cycle_loop`: `Halt` → debug pause, not fatal error
- [ ] Unit test: halt becomes DebugPaused not Error

### Phase C — Debug command channel

- [ ] `DebugCommand` / `DebugResponse` enums in `st-engine/src/debug.rs`
- [ ] Extend `RuntimeCommand` with `DebugAttach` / `DebugDetach`
- [ ] `RuntimeManager::debug_attach()` / `debug_detach()` async methods
- [ ] `handle_debug_commands()` blocking loop in runtime thread
- [ ] Watchdog: ignore `DebugPaused` status (don't restart paused engine)
- [ ] Unit tests: attach/detach/variables/channel-drop (5 tests)

### Phase D — In-process DAP handler

- [ ] `dap_attach_handler.rs`: translate DAP protocol ↔ DebugCommand/DebugResponse
- [ ] DAP proxy: route to attach handler when engine Running, subprocess when Idle
- [ ] Source file + virtual offset resolution from ProgramStore
- [ ] Integration tests: attach/breakpoints/step/variables/disconnect (6 tests)

### Phase E — Safety hardening

- [ ] Debug pause timeout (30 min default) — auto-detach on timeout
- [ ] Auto-detach on TCP disconnect (reader EOF → Disconnect command)
- [ ] I/O ordering invariant: outputs not written until paused cycle completes

### E2E QEMU tests

- [ ] `test_singleton_second_instance_fails`
- [ ] `test_singleton_process_count_exactly_one`
- [ ] `test_dap_attach_no_second_process`
- [ ] `test_program_resumes_after_debug_disconnect`
- [ ] `test_debug_pause_timeout_auto_resumes`

---

## Cross-Cutting Concerns

- [x] Testing: 714+ tests across 10+ crates
- [x] CI/CD: GitHub Actions + release-plz
- [x] Documentation: mdBook site (20+ pages)
- [x] Tracing / logging
- [x] Devcontainer
- [x] Error quality: line:column locations, severity, diagnostic codes
- [ ] IEC 61131-3 compliance tracking checklist