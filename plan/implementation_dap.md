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

### Monitor panel UX (COMPLETED — partially superseded)

- [x] `postMessage`-based incremental DOM updates
- [x] Watch list table with autocomplete, Force, Remove, Clear all
      _(superseded: the flat autocomplete list is replaced by the
      file-backed multi-tab TanStack grid — see "Watch Tables" below)_
- [x] Per-workspace persistence via `workspaceState`
      _(superseded: state moves to `*.st-watch` YAML files; only the
      active tab name remains in `workspaceState`)_
- [x] Force/Unforce wired to DAP evaluate REPL (local) and WebSocket (remote)
- [x] Live cycle stats display
- [x] Tests: `test_watch_list_flow`, `test_var_catalog_emitted_on_launch`
      _(to be deleted alongside the old watch-list storage)_


### Hierarchical FB instance display

- [x] DAP: FB locals with `variablesReference > 0` for tree expansion
- [x] `fb_var_refs` HashMap for ref ID → `(caller_id, slot_idx, fb_func_idx)`
- [x] Nested FB recursive expansion
- [x] Parent FB summary value
- [x] Evaluate handler resolves dotted paths via `resolve_fb_field`
- [x] DAP integration tests (3 tests)
- [x] Evaluate handler: `variablesReference > 0` for FB instances in Watch panel
- [x] Monitor panel: Preact-based webview with virtual DOM diffing
- [x] Monitor panel: recursive tree view via WatchNodeRow components
      _(superseded by TanStack Table + TanStack Virtual; see "Watch Tables")_
- [x] Monitor panel: tree data model (WatchNode tree from server)
- [x] Monitor panel: telemetry sends nested `children` for expanded FBs
- [x] Monitor panel: persist expand/collapse state in workspace state
      _(superseded: expanded paths move into `*.st-watch` YAML)_
- [x] Monitor panel: Force dialog popup with validation + Trigger (1-cycle force)
- [x] Monitor panel: Dockerized Playwright E2E tests (19 passing)
      _(to be re-scoped or retired once the Electron E2E suite in
      "Watch Tables → Phase E" is in place; both needn't coexist long-term)_
- [ ] Monitor panel: "Collapse all" / "Expand all" for large FB instances
      _(folded into TanStack migration — keyboard + button in the new grid)_
- [ ] `plc/varCatalog`: add `childNames` for FB-typed entries
- [ ] Tests: DAP tree expansion (single + nested FB)
      _(covered by the Electron E2E matrix below, not a separate item)_
- [ ] Tests: performance — FB with 50+ fields doesn't bloat telemetry
      _(kept; not watch-tables-specific)_

### Watch Tables (file-backed, TanStack Table) — see `plan/dap.md` for design

Replaces the flat `plcMonitor.watchList:<workspace>` `workspaceState` store
and the hand-rolled `WatchTable.tsx` recursive renderer. No backwards
compatibility — the workspaceState key is dropped, and any existing watch
list is discarded on upgrade.

#### Phase A — YAML parsing (prerequisite)

- [ ] Add `js-yaml` (+ `@types/js-yaml`) to `editors/vscode/package.json`
      devDependencies; pin to current LTS.
- [ ] Replace the regex YAML reader in `extension.ts:577-627`
      (`getTargetsFromConfig`) with `js-yaml.load` + a typed interface.
- [ ] Delete the regex-based `plc-project.yaml` parser entirely — no
      fallback path.
- [ ] Verify `serde_yaml` (already a workspace dep) is used everywhere Rust
      reads YAML; no hand-rolled parsers left in the codebase.

#### Phase B — `.st-watch` schema + I/O (extension host)

- [ ] New file `editors/vscode/src/watchTables.ts` with TypeScript interfaces
      mirroring the YAML schema in `plan/dap.md`:
      `WatchTable { name, expanded[], rows[] }`,
      `WatchRow { path, comment?, format?, force_value? }`.
- [ ] `discoverWatchTableFiles(root: Uri): Promise<Uri[]>` — globs
      `<root>/*.st-watch` via `vscode.workspace.findFiles`.
- [ ] `loadWatchTable(uri): Promise<WatchTable>` — `workspace.fs.readFile`
      + `js-yaml.load`; strict schema, reject unknown keys.
- [ ] `saveWatchTable(uri, table)` — `js-yaml.dump` (stable key order:
      `name`, `expanded`, `rows`; rows serialized in display order);
      `workspace.fs.writeFile`.
- [ ] Debounced save (200 ms per-file) with coalescing — multiple edits
      within the window collapse to one write.
- [ ] `vscode.workspace.createFileSystemWatcher('**/*.st-watch')` —
      reload table on external edit; drop tab if file deleted; create
      tab if file added.
- [ ] `createWatchTable(root, name)` — slugify name → `<slug>.st-watch`,
      handle filename collisions (append `-2`, `-3`…).
- [ ] `renameWatchTable(uri, newName)` — rename file + update `name:` in
      YAML atomically.
- [ ] `deleteWatchTable(uri)` — confirm dialog, then `workspace.fs.delete`.
- [ ] Drop all `plcMonitor.watchList:…` and `plcMonitor.expandedNodes:…`
      calls from `monitorPanel.ts:661-697`. Delete `loadWatchList`,
      `saveWatchList`, `loadExpandedNodes`, `saveExpandedNodes`.
- [ ] `workspaceState` retains only `plcMonitor.activeWatchTable` (string
      filename of the active tab).

#### Phase C — TanStack Table adoption (webview)

- [ ] Add `@tanstack/react-table`, `@tanstack/react-virtual`, `@dnd-kit/core`,
      `@dnd-kit/sortable`, `@preact/signals` to the webview deps.
- [ ] Configure bundler alias: `react` / `react-dom` → `preact/compat` in
      `esbuild.webview.mjs`.
- [ ] Pin `preact >= 10.19` (for stable `useSyncExternalStore`).
- [ ] Delete `editors/vscode/src/webview/WatchTable.tsx` and its recursive
      row component.
- [ ] New `WatchTableGrid.tsx` built on TanStack Table:
      - `getSubRows` returns FB / struct / array children from server-sent
        `WatchNode` tree
      - Row virtualization via `useVirtualizer` from `@tanstack/react-virtual`
      - Columns: expand toggle, name (tree indent), value, type, comment,
        format dropdown, force-value input, force/unforce button, remove
      - Each **live value cell** subscribes to a `signal<VariableValue>` from
        a per-variable `Map<string, Signal>` — table shell never re-renders
        on tick, only the affected leaf `<td>`
- [ ] Tab strip component (`WatchTableTabs.tsx`): new / rename (inline) /
      duplicate / delete / drag-reorder via `@dnd-kit/sortable`.
- [ ] Row drag-reorder (within a table) via `@dnd-kit/sortable`; persists
      to `.st-watch` row order.
- [ ] Inline-editable comment cell (blur commits).
- [ ] Format dropdown cell (dec/hex/bin/bool/ascii/float) renders value
      according to chosen format; persisted per row.
- [ ] Force-value input cell: accepts typed literal, validates against var
      type; "Force" button invokes existing DAP/HTTP force path using the
      stored `force_value`.
- [ ] `.st-watch` write triggers: add row, remove row, reorder, expand,
      collapse, comment edit, format change, force-value edit.

#### Phase D — DAP / HTTP wire changes

- [ ] Extend the agent's watch-subscribe message to accept an array of
      `{path, format}` pairs instead of bare paths. Server stores display
      preferences per subscription so value strings come back pre-formatted.
- [ ] Extension sends the union of all open tables' row paths on the wire
      (a variable watched in multiple tables is sent once); each tab filters
      the incoming snapshot locally.
- [ ] Telemetry echoes the chosen `format` in each `VariableValue` so tab
      switches don't need to re-request.

#### Phase E — E2E tests (`@vscode/test-electron` — real VS Code, no mocks)

All tests run under `editors/vscode/src/test/suite/watchTables.test.ts` and
drive a real VS Code instance with a fixture workspace that contains
`plc-project.yaml` + at least one `.st` program.

**File discovery & parsing**
- [ ] Opens panel on a project with no `.st-watch` files → empty default tab
      auto-created, file written to disk.
- [ ] Project with two `.st-watch` files → two tabs in the declared order.
- [ ] Malformed YAML (`yaml: : :`) → error diagnostic surfaced, other tabs
      still load.
- [ ] Unknown keys in YAML → strict load rejects with diagnostic.
- [ ] File deleted while panel open → tab disappears within 1 s.
- [ ] File created in the project root while panel open → new tab appears.
- [ ] External edit (simulated via `workspace.fs.writeFile` from the test) →
      panel reflects the change within 1 s.
- [ ] `plc-project.yaml` with `js-yaml` (no regex) — resolves targets
      identically to the pre-refactor behavior (golden comparison).

**Persistence**
- [ ] Add row → file written within 500 ms contains the new row.
- [ ] Remove row → file written without that row.
- [ ] Reorder rows via drag → file written with new order; close/reopen
      panel preserves order.
- [ ] Expand FB → file's `expanded` list contains the path.
- [ ] Collapse → path removed from `expanded`.
- [ ] Edit comment → file's `comment` field updated.
- [ ] Change format → file's `format` field updated.
- [ ] Edit force-value → file's `force_value` field updated.
- [ ] Close & reopen VS Code → all per-row state intact (comments, formats,
      force values, expand/collapse, row order, active tab).
- [ ] Close panel, reopen → active tab restored from `workspaceState`;
      other tab state from files.
- [ ] Rapid edits (10 changes in 100 ms) → exactly one coalesced write.

**Tab management**
- [ ] `+` button creates a new `.st-watch` file with unique slug.
- [ ] Slug collision: second "My Table" becomes `my-table-2.st-watch`.
- [ ] Rename tab → file renamed on disk; other tabs unaffected.
- [ ] Delete tab → file deleted; confirm dialog shown first.
- [ ] Drag-reorder tabs → new order persists across reload
      (stored in `workspaceState`, not in files — tab order is UI).
- [ ] Duplicate tab → new file with `-copy` suffix; all rows copied.

**Tree rendering (TanStack Table)**
- [ ] FB root renders with expand chevron; value cell is summary.
- [ ] Expanding FB reveals children; virtualization keeps DOM node count
      < 100 for 1000-row tables.
- [ ] Nested FB (3 levels) — each level expands independently.
- [ ] Array variable renders indexed children `[0]..[N-1]`.
- [ ] Struct renders field children.
- [ ] Keyboard: Arrow down/up moves selection; Right expands, Left collapses.
- [ ] Expand-all / collapse-all buttons.

**Live updates via signals**
- [ ] Value updates at 20 Hz don't cause full table re-render
      (instrument with `preact/devtools` counter or a simple ref-count
      assertion on a sentinel cell).
- [ ] Adding a row mid-session starts receiving values within one tick.
- [ ] Removing the last subscriber to a path stops updates (agent unsubscribes).

**Force / unforce**
- [ ] Per-row force button forces to the stored `force_value`.
- [ ] Force button disabled if `force_value` is null.
- [ ] Unforce button appears on forced rows and clears the force.
- [ ] Forced state renders with a distinct style.
- [ ] Force persists across panel close/reopen (state from agent, not file).

**Format column**
- [ ] INT value displays as `255`, `16#FF`, `2#1111_1111`, `"\xff"`,
      depending on format selection.
- [ ] REAL with float format shows decimal; with hex shows bit pattern.
- [ ] BOOL with bool format shows `TRUE`/`FALSE`; with dec shows `0`/`1`.
- [ ] Invalid format for type (e.g., ascii on REAL) falls back to dec with
      a warning tooltip.

**Smoke / regression**
- [ ] `npm run compile` + `build:webview` produce no warnings after TanStack
      migration.
- [ ] Bundle size for `out/webview/monitor.js` stays under 250 KB gzipped
      (assertion in CI).
- [ ] No usage of `plcMonitor.watchList:…` remains in the codebase
      (grep assertion in the E2E test setup).
- [ ] No regex YAML parsing remains (grep assertion).
- [ ] Monitor-tree unit tests in `editors/vscode/test/monitor-tree.test.js`
      either pass unchanged or are updated for the new tree model and still
      pass.

**Multi-file / cross-file**
- [ ] Watch paths from files in subdirectories resolve correctly.
- [ ] `plc-project.yaml` with multiple source dirs — all watched paths work.

**CI wiring**
- [ ] `vscode-electron` CI job (already added) picks up the new suite;
      timeout bumped if needed.
- [ ] Fixture workspace committed under
      `editors/vscode/src/test/fixtures/watch-tables/` with `plc-project.yaml`,
      a sample program, and golden `.st-watch` files.

#### Phase F — Docs

- [ ] `docs/src/monitor/watch-tables.md` — user-facing guide:
      creating tables, committing to git, YAML schema, per-row features.
- [ ] Update top-level README's monitor section.
- [ ] Remove any reference to the old `workspaceState` watch list in docs.

#### Deferred / out of scope for this effort (follow-up tickets)

- [ ] TIA Portal `.tww` import
- [ ] Trigger expressions (boolean ST expression, sample only when true)
- [ ] Snapshot / Compare (capture + side-by-side diff)
- [ ] Charting view (sparkline / line chart for numeric variables)

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