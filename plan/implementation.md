# IEC 61131-3 Compiler + LSP + Online Debugger — Progress Tracker

> **Design document:** [design_core.md](design_core.md) — architecture, phase descriptions, design notes.
> **See also:**
> - [implementation_dap.md](implementation_dap.md) — DAP debugger progress (breakpoints, stepping, variables, attach, roadmap)
> - [dap.md](dap.md) — DAP audit & roadmap (gap analysis, competitive position)
> - [implementation_comm.md](implementation_comm.md) — communication layer progress
> - [design_comm.md](design_comm.md) — communication layer design
> - [implementation_native.md](implementation_native.md) — LLVM native compilation + hardware targets
> - [implementation_deploy.md](implementation_deploy.md) — remote deployment & online management
> - [design_deploy.md](design_deploy.md) — remote deployment design
> - [cross_compilation_gaps.md](cross_compilation_gaps.md) — cross-compilation reference

---

## Phases 0-11: Core Platform (COMPLETED)

1050+ tests, zero clippy warnings.

- [x] Phase 0: Project scaffolding, workspace, CI, VSCode extension scaffold
- [x] Phase 1: Tree-sitter ST grammar (case-insensitive, incremental)
- [x] Phase 2: AST types + CST→AST lowering
- [x] Phase 3: Semantic analysis: scopes, types, 30+ diagnostics
- [x] Phase 4: LSP server skeleton + VSCode extension
- [x] Phase 5: Advanced LSP (completion, signature help, rename, formatting, code actions)
- [x] Phase 6: Register-based IR + AST→IR compiler (50+ instructions)
- [x] Phase 7: Bytecode VM + scan cycle engine + stdlib + pointers
- [x] Phase 8: DAP debugger (breakpoints, stepping, variables, force/unforce)
- [x] Phase 9: Online change manager (hot-reload with variable migration)
- [x] Phase 10: WebSocket monitor server + VSCode panel
- [x] Phase 11: CLI tool (check, run, serve, debug, compile, fmt, --json)

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

- [x] Diagnostic routing via virtual concatenated coordinate system
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
- [x] Force variable: `forced_global_slots` HashSet, narrowing, lock icon, type validation, struct/FB field force
- [x] Global variable initialization via synthetic `__global_init` function

---

## Cycle-Time Feedback

### Tier 1 — scanCycleInfo + real cycle stats (COMPLETED)

- [x] DAP session owns `CycleStats`, times each cycle in run loop
- [x] `handle_cycle_info` reports real metrics

### Tier 2 — live status bar (COMPLETED)

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

### Subscription model + watch list (COMPLETED)

- [x] `DapSession.watched_variables` — telemetry only ships watched values
- [x] REPL commands: `addWatch`, `removeWatch`, `watchVariables`, `clearWatch`, `varCatalog`
- [x] Immediate telemetry push on each watch mutation
- [x] `Vm::monitorable_catalog()` and `Vm::monitorable_variables()`
- [x] `plc/varCatalog` telemetry event on launch

### Monitor panel UX (COMPLETED)

- [x] Preact-based webview (replaced vanilla DOM manipulation)
- [x] Virtual DOM diffing — buttons survive live value updates, no click-swallowing
- [x] Watch list table with autocomplete, Force, Remove, Clear all
- [x] Per-workspace persistence via `workspaceState`
- [x] Force dialog popup: validates input, shows current value when forced
- [x] Trigger (single-cycle force) command — TIA Portal style
- [x] Unforce button on forced rows + inline forced-value badge
- [x] Force/Unforce wired to DAP evaluate REPL (local) and WebSocket (remote)
- [x] Live cycle stats display
- [x] Tests: `test_watch_list_flow`, `test_var_catalog_emitted_on_launch`

### Watch Tables

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

Infrastructure: `@vscode/test-electron` (real Electron instance) + Playwright (webview DOM in Docker) + unit tests.
14 Electron tests, 21 Playwright tests, 12 unit tests. Makefile orchestrates all suites via Docker containers.

### Extension tests (via @vscode/test-electron) — 5 passing

- [x] ST language registered
- [x] `.st` files recognized as Structured Text
- [x] Extension activates on `.st` file
- [x] Diagnostics appear for undeclared variable
- [x] Syntax highlighting provides tokens

### Debug button tests (via @vscode/test-electron) — 11 passing

- [x] Launch with `stopOnEntry` pauses at first statement
- [x] Step In advances to next line
- [x] Step Over advances without entering calls
- [x] Multiple Step Ins progress through the program
- [x] Continue → Pause stops execution (counter advances during run)
- [x] Evaluate expression while paused returns correct value
- [x] Stop terminates the session
- [x] Stop during Continue terminates cleanly
- [x] Breakpoint hit stops at correct line
- [x] Force / unforce variable via DAP `evaluate` — `listForced` reflects state
- [x] Multi-file project: breakpoint in `helper.st` fires from `main.st`

### LSP headless tests (via @vscode/test-electron) — 6 passing

- [x] Hover on INT variable returns type info
- [x] Hover on REAL variable returns REAL type
- [x] Hover on whitespace returns no result (handler null path)
- [x] Go-to-definition on a local variable jumps to its declaration
- [x] Go-to-definition across files lands in the helper file
- [x] Go-to-definition on whitespace returns nothing

### Online update headless tests (via @vscode/test-electron) — 5 passing

- [x] Initial deploy via `targetOnlineUpdate` command (cold)
- [x] Online change applied while engine running (variables preserved)
- [x] Incompatible update falls back to clean restart
- [x] `targetOnlineUpdate` command exposed in command palette
- [x] Status bar item is wired to the update command

### Non-intrusive Live Attach tests (via @vscode/test-electron) — 6 passing

- [x] Attach with `stopOnEntry: false` does NOT pause the running engine
- [x] Setting a breakpoint freezes the cycle counter, clear+continue resumes
- [x] Disconnecting the debugger leaves the engine running
- [x] `targetLiveAttach` command appears in the palette
- [x] `targetLiveAttach` command starts a non-intrusive debug session
- [x] `tb:liveAttach` button wired into the Monitor toolbar message handler

### Playwright webview tests — 19 passing, 2 skipped (Docker)

- [x] Watch list CRUD (add, remove, clear)
- [x] FB tree expand/collapse with nested FBs
- [x] Tree from telemetry `children` array
- [x] Value updates without rebuilding structure
- [x] Expand/collapse state persistence
- [x] Session reset: values clear and rebuild on new session
- [x] Force dialog opens reliably during live updates
- [x] Dockerized test runner (`docker/playwright.Dockerfile`)
- [x] Real Rust WS test server (monitor-test-server) compiled in Docker

### Build & test infrastructure

- [x] Makefile with all test targets (`make test`, `make test-ui`, `make test-modbus`, etc.)
- [x] Docker test container (`docker/test.Dockerfile`) — Rust + Node + system deps
- [x] Docker Playwright container (`docker/playwright.Dockerfile`) — multi-stage build
- [x] All tests run in containers — no host-specific dependencies

### Remaining E2E tests

- [x] Hover shows type information on variables
- [x] Go-to-definition navigates to symbol
- [x] Force/unforce variable via custom request (test server doesn't implement force)
- [x] Multi-file project: breakpoints across files
- [x] `structured-text.openMonitor` command opens the panel
- [x] Headless CI via Xvfb in GitHub Actions

---

## Phase 16: RETAIN / PERSISTENT Variable Persistence (COMPLETED)

16 tests.

- [x] IR: `persistent: bool` field on `VarSlot` (serde-default for backward compat)
- [x] Compiler: set both `retain` and `persistent` from `VarQualifier` for globals and locals
- [x] `RetainStore` module: capture, restore, save (atomic JSON), load
- [x] Engine: restore on startup, periodic checkpoint, save on shutdown + before online change
- [x] CLI: resolve `.st-retain/<program>.retain` next to plc-project.yaml
- [x] Target agent: save on stop/shutdown, default `/var/lib/st-plc/retain/`
- [x] `plc-project.yaml`: `engine.retain.checkpoint_cycles` config
- [x] JSON schema updated
- [x] Struct-typed PERSISTENT RETAIN variables: `instance_fields` section in `RetainSnapshot`, capture/restore via `fb_instances`
- [x] 16 unit/integration tests

### Remaining

- [ ] DAP: show retain/persistent badge in Variables panel
- [ ] Monitor panel: indicate retain/persistent variables visually
- [ ] E2E test: QEMU target — deploy, run, stop service, restart, verify values preserved

---

## Phase 17: Singleton Enforcement + Debug Attach to Running Engine (COMPLETED)

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

### Phase D — In-process DAP handler + source path remapping (COMPLETED)

- [x] `dap_attach_handler.rs`: concurrent reader/event thread architecture
- [x] DAP proxy routes to attach handler when engine Running
- [x] stopOnEntry support
- [x] Variable inspection when paused
- [x] Engine pause/resume/detach lifecycle works (verified by Rust tests)
- [x] Adapter-side `PathMapper` with `localRoot`/`remoteRoot` prefix swap (9 unit tests)
- [x] `stackTrace` responses: target paths remapped to local workspace paths
- [x] `setBreakpoints` requests: local paths remapped to target paths (preserves subdirectory structure)
- [x] Windows path separator normalization (`\` → `/`)
- [x] VS Code `package.json`: `localRoot` property in attach config (default: `${workspaceFolder}`)
- [x] Extension injects `localRoot` automatically from workspace folder
- [x] Removed fragile client-side `PlcDapTracker` path remapping (marker-based `current_source/` detection)
- [x] `SourceMap` struct: computes virtual file offsets from stdlib + project files, builds func→file mapping
- [x] Fixed `resolve_frame_location`: subtracts file virtual offset from `source_offset` before line calculation
- [x] Fixed breakpoints: `DebugCommand::SetBreakpoints` now carries `source_offset` field
- [x] Diagnostic logging: source map build, setBreakpoints path/offset/content, stackTrace frame resolution, attach lifecycle

### Phase E — Safety hardening

- [x] Debug pause timeout (30 min)
- [x] Auto-detach on TCP disconnect
- [x] Call stack cleanup on detach
- [x] stop() accepts DebugPaused state
- [ ] E2E QEMU singleton tests (not yet run)

### Phase F — Live Attach UX (COMPLETED)

- [x] `structured-text.targetLiveAttach` command — `request: "attach"` with
      `stopOnEntry: false`, host/DAP-port/localRoot resolved from active
      target or explicit args
- [x] PLC Monitor toolbar: "Live Attach" button next to Run/Stop, disabled
      unless engine is running
- [x] Webview message wiring: `tb:liveAttach` posts to host, host forwards
      to the command with the selected target's host + DAP port
- [x] Command palette + status-bar discoverability via `shortTitle` + icon
- [x] TDD acceptance suite (`attach-running.test.ts`, gated by
      `ST_E2E_ATTACH=1`, runs under xvfb in CI):
  - [x] attach with `stopOnEntry: false` keeps cycle counter advancing
  - [x] setBreakpoints freezes counter; clear+continue resumes
  - [x] disconnect leaves engine running
  - [x] Live Attach command appears in palette
  - [x] Live Attach command starts session with the right config
  - [x] Toolbar.tsx + compiled webview bundle reference `tb:liveAttach`

---

## Phase 18: Unified HTTP/WS Monitoring (COMPLETED)

### HTTP API endpoints (st-target-agent)

- [x] `GET /api/v1/variables/catalog` — list all monitorable variables (names + types)
- [x] `GET /api/v1/variables?watch=Main.counter,Main.stats` — read watched variable values
- [x] `POST /api/v1/variables/force` — force a variable
- [x] `DELETE /api/v1/variables/force/:name` — unforce a variable
- [x] `GET /api/v1/status` — cycle stats + runtime status

### WebSocket endpoint

- [x] `GET /api/v1/monitor/ws` — real-time variable streaming (20 Hz throttled)
- [x] Protocol: subscribe, unsubscribe, read, force, unforce, getCatalog, getCycleInfo, resetStats
- [x] Per-client subscription filtering, broadcast from engine thread

### Monitor panel changes (VS Code extension)

- [x] Target dropdown with host/port selection from plc-project.yaml
- [x] HTTP status polling (every 5s) + auto WS connect when running
- [x] Force/unforce via WebSocket (scalars + struct/FB fields)
- [x] Variable catalog via WS getCatalog
- [x] Force controls on struct field leaf nodes (tree children)
- [x] Same code path for local and remote targets
- [x] No dependency on active debug session for monitoring

### Force variable support

- [x] Scalar globals: value written directly, `forced_global_slots` blocks program writes
- [x] Scalar PROGRAM locals: written to `retained_locals`, re-enforced after each cycle
- [x] Struct/FB fields: written to `fb_instances`, re-enforced via `enforce_retained_locals`
- [x] Force controls in monitor panel for both top-level scalars and struct field children
- [x] 6 integration tests (HTTP force, WS force, bool force, unforce, not-running, lifecycle)

### Remaining

- [ ] Local `st-cli debug` HTTP/WS server (currently only remote targets use HTTP/WS)
- [ ] `GET /api/v1/variables/forced` — list all currently forced variables
- [ ] Disable remote debug F5 attach (keep code for future rework)

---

## Time & Type Conversion Functions (IEC 61131-3 / CODESYS-compatible)

### Tier 1 — TIME ↔ numeric conversions (COMPLETED)

- [x] IR: `ToTime(Reg, Reg)` instruction
- [x] IR: `Value::as_time()` method (Int/UInt/Real/Bool → milliseconds)
- [x] IR: Fix `VarType::Time` doc comment (milliseconds, not nanoseconds)
- [x] VM: execute `ToTime` instruction
- [x] Semantics: register `TIME_TO_INT`, `TIME_TO_DINT`, `TIME_TO_LINT`, `TIME_TO_REAL`, `TIME_TO_LREAL`, `TIME_TO_BOOL`
- [x] Semantics: register `INT_TO_TIME`, `DINT_TO_TIME`, `LINT_TO_TIME`, `REAL_TO_TIME`, `LREAL_TO_TIME`, `BOOL_TO_TIME`
- [x] Semantics: register `TIME_TO_SINT`, `TIME_TO_UINT`, `TIME_TO_USINT`, `TIME_TO_UDINT`, `TIME_TO_ULINT`
- [x] Semantics: register `SINT_TO_TIME`, `UINT_TO_TIME`, `USINT_TO_TIME`, `UDINT_TO_TIME`, `ULINT_TO_TIME`
- [x] Compiler: map all `TIME_TO_*` to `ToInt`/`ToReal`/`ToBool`; map all `*_TO_TIME` to `ToTime`

### Tier 2 — Overloaded TO_* / ANY_TO_* generic dispatch (COMPLETED)

- [x] Semantics: register `TO_INT`, `TO_DINT`, `TO_LINT`, `TO_SINT`, `TO_REAL`, `TO_LREAL`, `TO_BOOL`, `TO_TIME`
- [x] Semantics: register `TO_UINT`, `TO_USINT`, `TO_UDINT`, `TO_ULINT`
- [x] Semantics: register `ANY_TO_INT`, `ANY_TO_DINT`, `ANY_TO_LINT`, `ANY_TO_SINT`, `ANY_TO_REAL`, `ANY_TO_LREAL`, `ANY_TO_BOOL`, `ANY_TO_TIME`
- [x] Semantics: register `ANY_TO_UINT`, `ANY_TO_USINT`, `ANY_TO_UDINT`, `ANY_TO_ULINT`
- [x] Compiler: map `TO_*` / `ANY_TO_*` to the same IR instructions as typed conversions
- [x] Stdlib: update `conversions.st` documentation

### Tier 3 — DATE / TOD / DT types + conversions (COMPLETED)

All date/time types share `Value::Time(i64)` in milliseconds (no separate Value/VarType variants needed). DATE = ms since epoch, TOD = ms since midnight, DT = ms since epoch.

- [x] Compiler: `parse_date_literal()` — D#YYYY-MM-DD → ms since epoch via civil-date algorithm
- [x] Compiler: `parse_tod_literal()` — TOD#HH:MM:SS[.frac] → ms since midnight
- [x] Compiler: `parse_dt_literal()` — DT#YYYY-MM-DD-HH:MM:SS[.frac] → ms since epoch
- [x] Compiler: `ymd_to_epoch_ms()` helper for date→epoch conversion
- [x] Compiler: `parse_time_literal()` — fixed: now handles `d` (days) suffix
- [x] IR: `DtExtractDate(Reg, Reg)`, `DtExtractTod(Reg, Reg)`, `DayOfWeek(Reg, Reg)`, `ToTod(Reg, Reg)` instructions
- [x] VM: execute all four new instructions (day-boundary truncation, modulo, weekday, TOD wrap)
- [x] TOD wrapping: `ToTod` instruction wraps modulo 86,400,000 ms (CODESYS-compatible)
- [x] TOD wrapping applies to: `*_TO_TOD`, `TO_TOD`, `ANY_TO_TOD`, `ADD_TOD_TIME`, `SUB_TOD_TIME`, `DtExtractTod`, TOD literal parsing
- [x] Semantics: register all `DATE_TO_*`, `TOD_TO_*`, `DT_TO_*` conversions (INT/SINT/DINT/LINT/UINT/USINT/UDINT/ULINT/REAL/LREAL/BOOL)
- [x] Semantics: register all `*_TO_DATE`, `*_TO_TOD`, `*_TO_DT` conversions
- [x] Semantics: register cross-type: `DT_TO_DATE`, `DT_TO_TOD`, `DATE_TO_DT`, `TIME_TO_DATE/TOD/DT`, `DATE/TOD/DT_TO_TIME`
- [x] Semantics: register `TO_DATE`, `TO_TOD`, `TO_DT`, `ANY_TO_DATE`, `ANY_TO_TOD`, `ANY_TO_DT`
- [x] Semantics: register two-arg arithmetic: `ADD_TOD_TIME`, `ADD_DT_TIME`, `SUB_TOD_TIME`, `SUB_DATE_DATE`, `SUB_TOD_TOD`, `SUB_DT_TIME`, `SUB_DT_DT`, `CONCAT_DATE_TOD`
- [x] Compiler: two-argument intrinsic handling for arithmetic functions → Add/Sub instructions
- [x] Compiler: map extraction functions to specialized IR instructions

### Tier 4 — Date/time utilities + string conversions (PARTIAL)

- [x] `MULTIME(IN1: TIME, IN2: INT) : TIME` — maps to Mul instruction
- [x] `DIVTIME(IN1: TIME, IN2: INT) : TIME` — maps to Div instruction
- [x] `DAY_OF_WEEK(IN1: DATE) : INT` — 0=Sunday..6=Saturday
- [ ] `TIME_TO_STRING`, `STRING_TO_TIME` — requires string formatting infrastructure
- [ ] `DATE_TO_STRING`, `STRING_TO_DATE` — requires string formatting infrastructure
- [ ] `SPLIT_DATE`, `SPLIT_TOD`, `SPLIT_DT` — requires multi-output function support
- [ ] `CONCAT_DATE`, `CONCAT_TOD`, `CONCAT_DT` (from year/month/day components) — requires multi-input
- [ ] `MULTIME` with REAL factor (currently INT only)

### Testing

- [x] Unit tests: `Value::as_time()`, `Value::as_uint()`, `Value::as_bool()` for Time/UInt/Real
- [x] Compiler tests: `ToTime`, `DtExtractDate`, `DtExtractTod`, `DayOfWeek`, `ConcatDateTod`, `Multime` instruction emission
- [x] Compiler tests: DATE/TOD/DT literal value verification
- [x] Stdlib integration tests: TIME_TO_*, *_TO_TIME, TO_*, ANY_TO_* (Tier 1+2)
- [x] Stdlib integration tests: DATE/TOD/DT literal parsing, extraction, arithmetic, round-trips (Tier 3)
- [x] Stdlib integration tests: MULTIME, DIVTIME, DAY_OF_WEEK (Tier 4)
- [x] Playground: `16_time_conversions.st` (TIME conversions + generics)
- [x] Playground: `17_date_time_types.st` (DATE/TOD/DT + arithmetic + extraction)
- [x] E2E: `playground_16_time_conversions_e2e` — 28 assertions
- [x] E2E: `playground_17_date_time_types_e2e` — 30 assertions

---

## String Manipulation & Formatting Functions (IEC 61131-3 / CODESYS-compatible)

Tier 5 lays down the string-function foundation that L486-487 (`TIME_TO_STRING`,
`DATE_TO_STRING`, etc.) depends on. Reference: CODESYS Standard library —
"String Functions" (`Strings.library`) plus the IEC 61131-3 standard library.

### Reference list — CODESYS string functions

These are the functions tracked here. Operations are 1-indexed (IEC convention)
and operate on byte-oriented `STRING` (no UTF-8 multi-byte handling — `STRING`
is a byte string per IEC). `WSTRING` is out of scope for Tier 5 and tracked
separately if needed.

#### Tier 5a — Core IEC 61131-3 manipulation (COMPLETED)

- [x] `LEN(IN: STRING) : INT` — length in bytes
- [x] `LEFT(STR: STRING, SIZE: INT) : STRING` — leftmost SIZE bytes
- [x] `RIGHT(STR: STRING, SIZE: INT) : STRING` — rightmost SIZE bytes
- [x] `MID(STR: STRING, LEN: INT, POS: INT) : STRING` — substring of LEN bytes starting at 1-based POS
- [x] `CONCAT(STR1: STRING, STR2: STRING) : STRING` — binary concatenation (variadic CONCAT deferred)
- [x] `INSERT(STR1: STRING, STR2: STRING, POS: INT) : STRING` — insert STR2 into STR1 after POS
- [x] `DELETE(STR: STRING, LEN: INT, POS: INT) : STRING` — delete LEN bytes starting at POS
- [x] `REPLACE(STR1: STRING, STR2: STRING, LEN: INT, POS: INT) : STRING` — replace LEN bytes at POS with STR2 (4-arg)
- [x] `FIND(STR1: STRING, STR2: STRING) : INT` — 1-based first match position; 0 if not found

#### Tier 5b — Case conversion (COMPLETED, CODESYS extension)

- [x] `TO_UPPER(IN: STRING) : STRING` (alias `UPPER_CASE`)
- [x] `TO_LOWER(IN: STRING) : STRING` (alias `LOWER_CASE`)

#### Tier 5c — Trimming (COMPLETED, CODESYS extension)

- [x] `TRIM(IN: STRING) : STRING` — strip leading + trailing ASCII whitespace
- [x] `LTRIM(IN: STRING) : STRING` — strip leading whitespace
- [x] `RTRIM(IN: STRING) : STRING` — strip trailing whitespace

#### Tier 5d — Numeric ↔ STRING (COMPLETED, foundation for Tier 4 STRING-formatting items)

- [x] `INT_TO_STRING`, `DINT_TO_STRING`, `LINT_TO_STRING`, `SINT_TO_STRING`
- [x] `UINT_TO_STRING`, `USINT_TO_STRING`, `UDINT_TO_STRING`, `ULINT_TO_STRING`
- [x] `REAL_TO_STRING`, `LREAL_TO_STRING` (`1.0` formats as `"1.0"`)
- [x] `BOOL_TO_STRING` (`"TRUE"` / `"FALSE"`)
- [x] `TO_STRING` / `ANY_TO_STRING` — runtime-typed overload
- [x] `STRING_TO_INT`, `STRING_TO_DINT`, `STRING_TO_LINT`, `STRING_TO_SINT`
- [x] `STRING_TO_UINT`, `STRING_TO_USINT`, `STRING_TO_UDINT`, `STRING_TO_ULINT`
- [x] `STRING_TO_REAL`, `STRING_TO_LREAL`
- [x] `STRING_TO_BOOL` (`"TRUE"` (any case) or `"1"` → TRUE; everything else → FALSE)

#### Tier 5e — Implementation (COMPLETED)

- [x] IR: `Instruction::StringLen(Reg, Reg)`
- [x] IR: `Instruction::StringConcat(Reg, Reg, Reg)`
- [x] IR: `Instruction::StringLeft(Reg, Reg, Reg)`, `StringRight(Reg, Reg, Reg)`
- [x] IR: `Instruction::StringMid(Reg, Reg, Reg, Reg)` — 3-input
- [x] IR: `Instruction::StringFind(Reg, Reg, Reg)`
- [x] IR: `Instruction::StringInsert(Reg, Reg, Reg, Reg)` — 3-input
- [x] IR: `Instruction::StringDelete(Reg, Reg, Reg, Reg)` — 3-input
- [x] IR: `Instruction::StringReplace { dst, str1, str2, len, pos }` — 4-input
- [x] IR: `Instruction::StringTrim/LTrim/RTrim(Reg, Reg)`
- [x] IR: `Instruction::StringToUpper/Lower(Reg, Reg)`
- [x] IR: `Instruction::IntToString`, `UIntToString`, `RealToString`, `BoolToString`
- [x] IR: `Instruction::StringToInt`, `StringToUInt`, `StringToReal`, `StringToBool`
- [x] IR: `Instruction::ToString(Reg, Reg)` — generic runtime-typed `TO_STRING` / `ANY_TO_STRING`
- [x] VM: every instruction implemented with IEC 1-indexed semantics + saturating clamps (`crates/st-engine/src/vm.rs`)
- [x] Semantics: all functions registered in `register_intrinsics()` using a generic n-arg helper (`crates/st-semantics/src/analyze.rs`)
- [x] Compiler: single/two/three/four-arg intrinsic dispatch in `compile.rs`; new `compile_call_arg` helper
- [x] Stdlib: `stdlib/strings.st` documentation file registered in `multi_file::builtin_stdlib()`
- [x] Docs: "String Functions" section added to `docs/src/language/standard-library.md`

#### Tier 5f — Testing & playground (COMPLETED)

- [x] Acceptance tests: `crates/st-engine/tests/string_tests.rs` — **89 tests** covering every function, edge cases (empty, position 0, position past end, oversized, negative arguments), round-trips, composition.
- [x] Playground: `playground/18_strings.st` covering every Tier 5 function with expected results in comments.
- [x] E2E: `playground_18_strings_e2e` in `stdlib_tests.rs` — 70+ assertions on every global produced by the playground program.
- [x] LSP unit tests (`crates/st-lsp/tests/unit_tests.rs`): 5 tests pinning that string intrinsics surface in completion with correct `FUNCTION(...) : RET` detail and named-arg snippets — covers `LEN`, `MID` (3-arg), `REPLACE` (4-arg), `INT_TO_STRING` return-type.
- [x] LSP integration tests (`crates/st-lsp/tests/lsp_integration.rs`): 3 tests via the LSP wire protocol — `signatureHelp` for `MID` (3 params), `signatureHelp` for `REPLACE` (4 params), `hover` for `INT_TO_STRING`.
- [x] DAP integration tests (`crates/st-dap/tests/dap_integration.rs`): 2 tests via the DAP wire protocol — STRING locals and globals populated by string intrinsics display with IEC single-quote rendering (`'HELLO'`, `'foobar'`) in both the variables view and the `evaluate` path; STRING type tag asserted.

#### Deferred (out of Tier 5)

- [ ] Variadic `CONCAT` (CODESYS extension accepting >2 args) — needs varargs intrinsic
- [ ] `FORMAT_STRING` (printf-like) — separate design effort
- [ ] `WSTRING` variants (`WLEFT`, `WCONCAT`, etc.) — only after WSTRING is wired through the value model
- [ ] `TIME_TO_STRING` / `DATE_TO_STRING` / `STRING_TO_TIME` / `STRING_TO_DATE` (Tier 4 line 486-487) — now unblocked; needs IEC date/time format-string support
- [ ] **Performance:** every Tier 5 instruction allocates a fresh `String` for its result (per-op heap allocation, O(n) in length). Acceptable for typical PLC workloads but adds up in tight loops. Candidate optimisations: `SmolStr` / small-string optimisation in `Value::String`, per-scan-cycle string arena, or in-place mutation for `INSERT`/`DELETE`/`REPLACE` when the source register is dead. Revisit if a benchmark shows string ops eating >5 % of a representative scan cycle.

---

## Cross-Cutting Concerns

- [x] Testing: 714+ tests across 10+ crates
- [x] CI/CD: GitHub Actions + release-plz
- [x] Documentation: mdBook site (20+ pages)
- [x] Tracing / logging
- [x] Devcontainer
- [x] Error quality: line:column locations, severity, diagnostic codes
- [ ] IEC 61131-3 compliance tracking checklist

---

## Test Coverage Improvements

Tracked from the coverage gap analysis in `/tmp/test-reports/COVERAGE_GAPS.md`
(baseline 65.05 % line coverage on 2026-04-23). After Phase 1, line coverage
is now **71.78 %** (+6.73 pp).

### Done (2026-04-23)

- [x] Subprocess coverage in `lsp_integration.rs`: `find_st_cli()` via
      `current_exe()`, `TestClient::clean_stop()` sends `shutdown` + `exit`
      and closes `child.stdin` so tower-lsp returns from `serve()` and the
      `.profraw` flushes before SIGKILL. Result: **`st-lsp/src/server.rs`
      0 % → 58.6 %**, `st-lsp` crate 27 % → 67 %.
- [x] Coverage now includes `st-comm-modbus` (50.6 %) and `st-comm-serial`
      (60.1 %) via the `show-env` workflow (no longer excluded).
- [x] `st-target-agent/src/watchdog.rs` — 8 new unit tests using
      `tokio::time::pause()`: **0 % → 96.94 %**.
- [x] `st-lsp/src/document.rs` — 13 new direct unit tests for offset↔position,
      virtual-space mapping, update happy/error paths: **53.6 % → 75.5 %**.

### Phase 2 — high-ROI targeted tests (deferred)

- [ ] **`st-target-agent/src/api/program.rs`** (73.5 % → 90 %): error-path tests
      for invalid programs, multipart-upload edge cases.
- [ ] **`st-target-agent/src/api/monitor_ws.rs`** (65.8 % → 85 %): WS
      subscribe/unsubscribe/force error paths, catalog-empty edge case.
- [ ] **`st-engine/src/retain_store.rs`** (58.9 % → 85 %):
      `capture_instance_fields`, warm vs. cold `restore_snapshot`, `save_to_file` /
      `load_from_file` error branches, `is_compatible` corner cases.
- [ ] **`st-target-agent/src/dap_attach_handler.rs`** (56.5 % → 80 %):
      request-level unit tests using an in-memory DAP transport recorder. Cover
      `disconnect`, `stackTrace` edge cases, `variables` for FB fields, and
      breakpoint resolution at `virtual_offset`.

### Phase 3 — external-dependency gated (need fakes/fixtures)

- [ ] **`st-comm-modbus-tcp/src/client.rs`** (0 %): table-driven tests against
      a mock `TcpStream` (tokio `DuplexStream`).
- [ ] **`st-comm-modbus-tcp/src/device_fb.rs`** (40.2 %) and `transport.rs`
      (48.8 %): use `st-comm-sim` as a fake or add an in-process TCP fixture.
- [ ] **`st-deploy/src/ssh.rs`** (37.8 %): small integration suite against the
      existing QEMU x86_64 VM; harness already exists in
      `tests/e2e-deploy/vm/`.
- [ ] **`st-deploy/src/installer.rs`** (0 %, 230 lines, no tests at all):
      unit-test `find_static_binary`; integration-test `install`/`uninstall`
      against the QEMU fixture.

### Phase 4 — infrastructure / refactor

- [ ] Split `st-cli/src/main.rs` (907 uncovered lines) into a thin shim over a
      library entrypoint `st_cli::run(argv) -> ExitCode`, then unit-test the
      library surface.
- [ ] Decide on `st-comm-sim/src/web.rs` (0 %, 211 lines): delete if dead
      code, otherwise add `reqwest`-based smoke tests.
- [ ] Binary-search the 525 uncovered lines in `st-dap/src/server.rs` (73.9 %)
      via `cargo llvm-cov --html` and add targeted tests for `exceptionInfo`,
      `modules`, `loadedSources`, `setExpression`, breakpoint hit-counts, and
      log-message conditions.

### Wait-and-see

- [ ] `st-engine/src/vm.rs` (78.4 %): 353 uncovered lines mostly in rare
      opcodes and error paths; diminishing returns below 80 %, re-evaluate
      after Phase 2.
- [ ] `st-target-agent/src/runtime_manager.rs` (70.3 %): 203 uncovered lines
      in command dispatcher branches; will improve as Phase 2 tests exercise
      the API layer more thoroughly.