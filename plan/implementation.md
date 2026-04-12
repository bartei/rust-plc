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

## Phase 16: RETAIN / PERSISTENT Variable Persistence (COMPLETED)

> Design notes: [design_core.md § Phase 16](design_core.md#phase-16-retain--persistent-variable-persistence)

Non-volatile storage for RETAIN and PERSISTENT variables across runtime restarts
and program downloads, per IEC 61131-3 semantics. 14 tests.

- [x] IR: `persistent: bool` field on `VarSlot` (serde-default for backward compat)
- [x] Compiler: set both `retain` and `persistent` from `VarQualifier` for globals and locals
- [x] `RetainStore` module: capture, restore, save (atomic JSON), load
- [x] Engine: restore on startup, periodic checkpoint, save on shutdown + before online change
- [x] CLI: resolve `.st-retain/<program>.retain` next to plc-project.yaml
- [x] Target agent: save on stop/shutdown, default `/var/lib/st-plc/retain/`
- [x] `plc-project.yaml`: `engine.retain.checkpoint_cycles` config
- [x] JSON schema updated
- [x] 14 unit/integration tests (capture, restore, warm/cold restart, type mismatch, round-trip, engine restart lifecycle)

### Remaining

- [ ] DAP: show retain/persistent badge in Variables panel
- [ ] Monitor panel: indicate retain/persistent variables visually
- [ ] E2E test: QEMU target — deploy, run, stop service, restart, verify values preserved

---

## Phase 17: Singleton Enforcement + Debug Attach to Running Engine

> **⚠️ REMOTE DEBUGGING IS BROKEN — DO NOT USE IN PRODUCTION ⚠️**
>
> The Rust-side DAP protocol works (verified by integration tests) but the VS
> Code extension fails to properly remap source paths between the target and
> local workspace. Breakpoints don't work, stepping doesn't track lines, and
> the session is unreliable. 21 of 24 Electron E2E tests pass against a real
> target, but the 3 that fail are breakpoints, stepping, and full lifecycle —
> the most critical features.
>
> **Status:** Singleton enforcement and engine infrastructure are solid.
> Remote debug attach is ON HOLD until the VS Code extension path remapping
> is fundamentally reworked. The current `dap_attach_handler.rs` and
> `PlcDapTracker` source remapping approach is too fragile.

### Phase A — Singleton enforcement (COMPLETED)

- [x] PID file with `flock(LOCK_EX | LOCK_NB)` at `/run/st-runtime/st-runtime.pid`
- [x] `SingletonGuard` RAII struct: holds lock, removes PID file on drop
- [x] Integrated into `st-runtime agent` startup
- [x] Systemd unit hardening: `StartLimitBurst=5`, `StartLimitIntervalSec=30`, `RuntimeDirectory=st-runtime`
- [x] 4 unit tests

### Phase B — Handle VmError::Halt as debug pause (COMPLETED)

- [x] `RuntimeStatus::DebugPaused` added
- [x] `run_cycle_loop` restructured: Halt → debug pause, not fatal error
- [x] Watchdog ignores DebugPaused

### Phase C — Debug command channel (COMPLETED)

- [x] `DebugCommand` / `DebugResponse` enums in `st-engine/src/debug.rs`
- [x] `RuntimeCommand::DebugAttach` / `DebugDetach`
- [x] `RuntimeManager::debug_attach()` / `debug_detach()`
- [x] `handle_debug_commands()` blocking loop with 30-min timeout
- [x] Auto-detach on channel close
- [x] 5 integration tests (attach, pause, resume, reattach lifecycle)

### Phase D — In-process DAP handler (COMPLETED but BROKEN)

- [x] `dap_attach_handler.rs`: concurrent reader/event thread architecture
- [x] DAP proxy routes to attach handler when engine Running
- [x] stopOnEntry support
- [x] Variable inspection when paused
- [x] Engine pause/resume/detach lifecycle works (verified by Rust tests)
- **[!] Source path remapping broken** — VS Code can't open target-side files
- **[!] Breakpoints don't work** — path mismatch between local and target
- **[!] Stepping doesn't track lines** — source offset resolution wrong
- [ ] Needs fundamental rework of path remapping strategy

### Phase E — Safety hardening

- [x] Debug pause timeout (30 min)
- [x] Auto-detach on TCP disconnect
- [x] Call stack cleanup on detach
- [x] stop() accepts DebugPaused state
- [ ] E2E QEMU singleton tests (not yet run)

---

## Phase 18: Unified HTTP Monitoring (TODO — NEXT PRIORITY)

> The PLC Monitor panel should use HTTP API polling for cycle stats, watch
> variables, and force/unforce — working identically for local and remote
> targets. No dependency on DAP debug sessions.

### HTTP API endpoints (st-target-agent)

- [ ] `GET /api/v1/variables/catalog` — list all monitorable variables (names + types)
- [ ] `GET /api/v1/variables?watch=Main.counter,Main.stats` — read watched variable values
- [ ] `POST /api/v1/variables/force` — force a variable: `{ "name": "...", "value": "..." }`
- [ ] `DELETE /api/v1/variables/force/:name` — unforce a variable
- [ ] `GET /api/v1/variables/forced` — list all currently forced variables
- [ ] Existing `GET /api/v1/status` already has cycle stats

### Local HTTP server for st-cli debug

- [ ] `st-cli debug` exposes same HTTP endpoints on a local port
- [ ] Or: DAP server embeds a lightweight HTTP server alongside stdio
- [ ] Monitor panel connects to same URL regardless of local/remote

### Monitor panel changes (VS Code extension)

- [ ] "Connect" button with target dropdown (reuse existing target selector)
- [ ] HTTP polling loop: `/api/v1/status` every 1s, `/api/v1/variables` every 500ms
- [ ] Force/unforce via HTTP POST/DELETE (replace DAP evaluate REPL)
- [ ] Variable catalog via HTTP GET (replace DAP telemetry plc/varCatalog)
- [ ] Remove dependency on active debug session for monitoring
- [ ] Same code path for local and remote

### Remove broken remote debug

- [ ] Disable remote debug F5 attach option (or hide behind feature flag)
- [ ] Keep local debug (launch mode) working as-is
- [ ] Keep `dap_attach_handler.rs` code for future rework

---

## Cross-Cutting Concerns

- [x] Testing: 714+ tests across 10+ crates
- [x] CI/CD: GitHub Actions + release-plz
- [x] Documentation: mdBook site (20+ pages)
- [x] Tracing / logging
- [x] Devcontainer
- [x] Error quality: line:column locations, severity, diagnostic codes
- [ ] IEC 61131-3 compliance tracking checklist