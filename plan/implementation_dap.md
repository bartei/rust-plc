# DAP Debugger — Progress Tracker

> **Audit & roadmap:** [dap.md](dap.md) — gap analysis vs. industry, competitive position, prioritized feature tiers.
> **Core design:** [design_core.md](design_core.md) — architecture, VM, engine, scan cycle model.
> **See also:** [implementation.md](implementation.md) — master project tracker.

---

## Phase 8: DAP Debugger Foundation (COMPLETED)

### DAP Requests — Launch Mode (st-dap server)

- [x] `initialize` — 2 capabilities: `configurationDone`, `evaluateForHovers`
- [x] `launch` — multi-file project discovery, compilation, Initialized event
- [x] `attach` — reuses launch logic, returns Attach response body
- [x] `setBreakpoints` — line-only, multi-file, virtual offset mapping, verification
- [x] `configurationDone`
- [x] `continue`
- [x] `next`
- [x] `stepIn`
- [x] `stepOut`
- [x] `pause`
- [x] `stackTrace` — multi-file, per-function source mapping
- [x] `scopes` — Locals + Globals scopes
- [x] `variables` — FB instance expansion, struct fields, lazy loading
- [x] `evaluate` — Hover + REPL with 9 custom PLC commands
- [x] `disconnect`
- [x] `threads` — single "PLC Scan Cycle" thread

### DAP Requests — Attach Mode (dap_attach_handler)

- [x] `initialize` — same 2 capabilities
- [x] `attach` / `launch` — same handler
- [x] `setBreakpoints` — PathMapper remaps local↔target paths
- [x] `configurationDone`
- [x] `continue`
- [x] `next`
- [x] `stepIn`
- [x] `stepOut`
- [x] `pause`
- [x] `stackTrace` — PathMapper remaps target→local paths
- [x] `scopes` — Locals + Globals
- [x] `variables`
- [x] `evaluate` — hover evaluation only, no REPL commands
- [x] `disconnect`
- [x] `threads` — single thread
- [x] `loadedSources` — stub, returns empty array
- [x] `setExceptionBreakpoints` — stub, no-op response

### PLC-Specific REPL Commands (launch mode only)

- [x] `force <var> = <value>` — force variable to value
- [x] `unforce <var>` — release forced variable
- [x] `listForced` / `forced` — list all forced variables
- [x] `scanCycleInfo` / `cycleinfo` — cycle timing statistics
- [x] `addWatch <var>` — add variable to watch list
- [x] `removeWatch <var>` — remove variable from watch list
- [x] `clearWatch` — clear entire watch list
- [x] `watchVariables <csv>` — replace watch list
- [x] `varCatalog` — emit full variable catalog

### Capabilities Declared

- [x] `supportsConfigurationDoneRequest`
- [x] `supportsEvaluateForHovers`

---

## Multi-File Debug Support (COMPLETED)

- [x] Multi-file project loading and compilation
- [x] Per-file source mapping for stack traces
- [x] Breakpoints work in any project file
- [x] Step-into crosses file boundaries
- [x] Initialized event after Launch (per DAP spec)
- [x] DAP launch error points to Problems panel instead of generic dialog

---

## Runtime + Debugger Improvements (COMPLETED)

- [x] DAP interruptible run loop (reader thread + mpsc channel)
- [x] Removed 100k-cycle hard cap; 10M safety net for tests
- [x] `scope_refs` leak fix (cleared on resume)
- [x] Continue response sent before blocking run loop (play/pause button fix)
- [x] Live event streaming during Continue (writer passed into run loop)
- [x] Pause button fix: `resume_with_source` no longer clears pending pause flag
- [x] Default 1ms cycle period when no `cycle_time` configured (Pause works reliably)
- [x] Monitor panel session reset: `valueMap`/`childrenMap` cleared on new session
- [x] Watch list resync: tracker retries `sendWatchListToDap` on empty telemetry
- [x] Force variable: `forced_global_slots` HashSet, narrowing, lock icon, type validation, struct/FB field force

---

## Cycle-Time Feedback via DAP

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

- [x] `postMessage`-based incremental DOM updates
- [x] Watch list table with autocomplete, Force, Remove, Clear all
- [x] Per-workspace persistence via `workspaceState`
- [x] Force/Unforce wired to DAP evaluate REPL (local) and WebSocket (remote)
- [x] Live cycle stats display
- [x] Tests: `test_watch_list_flow`, `test_var_catalog_emitted_on_launch`

### Known issues

- [ ] Monitor panel: scan cycle stats not updating when watch list is empty.
  The DAP server emits `plc/cycleStats` events every ~500ms with full stats
  regardless of the watch list, but the VS Code extension panel appears inactive
  when the `variables` array in the event payload is empty. Fix needed in the
  extension's webview JS — cycle stats should render independently of the
  watched variables section.

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

---

## Debug Attach to Running Engine (COMPLETED)

### Source Path Remapping (COMPLETED)

- [x] Adapter-side `PathMapper` with `localRoot`/`remoteRoot` prefix swap (9 unit tests)
- [x] `stackTrace` responses: target paths remapped to local workspace paths
- [x] `setBreakpoints` requests: local paths remapped to target paths (preserves subdirectory structure)
- [x] Windows path separator normalization (`\` → `/`)
- [x] VS Code `package.json`: `localRoot` property in attach config (default: `${workspaceFolder}`)
- [x] Extension injects `localRoot` automatically from workspace folder
- [x] Removed fragile client-side `PlcDapTracker` path remapping

### Source Map Infrastructure (COMPLETED)

- [x] `SourceMap` struct: computes virtual file offsets from stdlib + project files, builds func→file mapping
- [x] Fixed `resolve_frame_location`: subtracts file virtual offset from `source_offset` before line calculation
- [x] Fixed breakpoints: `DebugCommand::SetBreakpoints` now carries `source_offset` field

### Debug Command Channel (COMPLETED)

- [x] `DebugCommand` / `DebugResponse` enums in `st-engine/src/debug.rs`
- [x] `RuntimeCommand::DebugAttach` / `DebugDetach`
- [x] `RuntimeManager::debug_attach()` / `debug_detach()`
- [x] `handle_debug_commands()` blocking loop with 30-min timeout
- [x] Auto-detach on channel close
- [x] 5 integration tests (attach, pause, resume, reattach lifecycle)

### Safety Hardening (COMPLETED)

- [x] Debug pause timeout (30 min)
- [x] Auto-detach on TCP disconnect
- [x] Call stack cleanup on detach
- [x] stop() accepts DebugPaused state

---

## VS Code Debug E2E Tests

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

### Remaining E2E tests

- [ ] Force/unforce variable via custom request
- [ ] Multi-file project: breakpoints across files
- [ ] Headless CI via Xvfb in GitHub Actions

---

## Test Coverage

- [x] 45 DAP integration tests (`crates/st-dap/tests/dap_integration.rs`)
- [x] 9 PathMapper unit tests (`crates/st-target-agent/src/dap_attach_handler.rs`)
- [x] 4 engine breakpoint tests (`crates/st-engine/tests/dap_breakpoint_test.rs`)
- [x] 5 DAP proxy integration tests (`crates/st-target-agent/tests/dap_proxy_integration.rs`)
- [x] 9 Electron debug E2E tests (`editors/vscode/src/test/`)
- [x] 21 Playwright monitor tests (`editors/vscode/src/test/`)

---

## DAP Feature Roadmap

### Tier 1 — High-Impact DAP Conformance

- [ ] Conditional breakpoints (`supportsConditionalBreakpoints`) — break when `counter > 100`
- [ ] Hit count breakpoints (`supportsHitConditionalBreakpoints`) — break on Nth scan cycle
- [ ] Logpoints (`supportsLogPoints`) — log without stopping, essential for real-time PLC
- [ ] setVariable (`supportsSetVariable`) — right-click → type new value (currently requires `force` REPL)
- [ ] Inline values (VS Code `InlineValuesProvider`) — show values inline, matches PLC "online view"
- [ ] Trace/trend recording (PLC-specific WebView) — WS data pipeline exists; needs ring buffer + chart view

### Tier 2 — Differentiators

- [ ] Data breakpoints (`supportsDataBreakpoints`) — break when value changes, PLC killer feature
- [ ] Execution marking (VS Code decorations) — highlight lines executed in last scan cycle
- [ ] Function breakpoints (`supportsFunctionBreakpoints`) — break on FB call by name
- [ ] Completions (`supportsCompletionsRequest`) — autocomplete in debug console
- [ ] Restart (`supportsRestartRequest`) — warm restart without restarting debug session

### Tier 3 — Polish

- [ ] loadedSources — list all project source files (attach handler has stub, launch mode missing)
- [ ] Modules list — list loaded POUs (PROGRAM, FB, FUNCTION)
- [ ] Exception breakpoints — break on runtime faults (attach handler has no-op stub, no filters/catching)
- [ ] Progress reporting — compile/download progress indicators
- [ ] Value formatting — hex/decimal/binary display for bitmask I/O
- [ ] Terminate request — explicit terminate vs. disconnect

### Tier 4 — Future

- [ ] Step back / reverse debugging (requires cycle recording)
- [ ] Disassembly view (VM bytecode)
- [ ] Multiple threads as multiple IEC tasks
- [ ] Read/write memory

---

## Remaining Cross-Cutting Items

- [ ] Online change: DAP custom request + VSCode toolbar
- [ ] DAP: show retain/persistent badge in Variables panel
- [ ] Attach mode: REPL commands (force, unforce, watch, etc.) — currently launch-only
- [ ] Local `st-cli debug` HTTP/WS server (currently only remote targets use HTTP/WS)
- [ ] Disable remote debug F5 attach (keep code for future rework)