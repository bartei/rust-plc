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

- [ ] Hover shows type information on variables
- [ ] Go-to-definition navigates to symbol
- [ ] Force/unforce variable via custom request (test server doesn't implement force)
- [ ] Multi-file project: breakpoints across files
- [ ] `structured-text.openMonitor` command opens the panel
- [ ] Headless CI via Xvfb in GitHub Actions

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

## Cross-Cutting Concerns

- [x] Testing: 714+ tests across 10+ crates
- [x] CI/CD: GitHub Actions + release-plz
- [x] Documentation: mdBook site (20+ pages)
- [x] Tracing / logging
- [x] Devcontainer
- [x] Error quality: line:column locations, severity, diagnostic codes
- [ ] IEC 61131-3 compliance tracking checklist