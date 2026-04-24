# DAP Implementation Audit & Roadmap

> Last updated: 2026-04-12
> **Implementation tracker:** [implementation_dap.md](implementation_dap.md) — detailed progress, completed work, test counts.

## Current Implementation Summary

### DAP Requests Implemented

**Launch mode (st-dap server)** — 15 requests:

| Request | Quality | Notes |
|---------|---------|-------|
| initialize | Basic | 2 capabilities declared (configurationDone, evaluateForHovers) |
| launch | Complete | Multi-file project discovery, compilation, Initialized event |
| attach | Complete | Reuses launch logic, returns Attach response body |
| setBreakpoints | Complete | Line-only, multi-file, virtual offset mapping, verification |
| configurationDone | Complete | |
| continue | Complete | |
| next / stepIn / stepOut | Complete | |
| pause | Complete | |
| stackTrace | Complete | Multi-file, per-function source mapping |
| scopes | Complete | Locals + Globals scopes |
| variables | Complete | FB instance expansion, struct fields, lazy loading |
| evaluate | Enhanced | Hover + REPL with 9 custom PLC commands |
| disconnect | Complete | |
| threads | Basic | Single "PLC Scan Cycle" thread |

**Attach mode (dap_attach_handler)** — 13 requests:

Same core set minus some evaluate REPL commands. Adds:
- PathMapper with localRoot/remoteRoot (9 unit tests)
- Adapter-side source path remapping (stackTrace + setBreakpoints)

### PLC-Specific Commands (via evaluate REPL)

- `force <var> = <value>` / `unforce <var>` / `listForced`
- `scanCycleInfo` / `cycleinfo`
- `addWatch` / `removeWatch` / `clearWatch` / `watchVariables`
- `varCatalog`

### Test Coverage

- 45 DAP integration tests (st-dap crate)
- 9 PathMapper unit tests (target-agent)
- 4 engine breakpoint tests
- 40+ target-agent API integration tests (HTTP + WebSocket, including force)
- 14 Electron E2E tests, 21 Playwright webview tests

---

## Gap Analysis vs. Industry

### Feature Matrix: What Major Adapters Implement

#### Breakpoints

| Feature | Spec | Node.js | Python | Go | CodeLLDB | cpptools | Java | **st-dap** | PLC Relevance |
|---------|------|---------|--------|----|----------|----------|------|------------|---------------|
| Line breakpoints | Yes | Yes | Yes | Yes | Yes | Yes | Yes | **Yes** | Essential |
| Conditional | Yes | Yes | Yes | Yes | Yes | Yes | Yes | **No** | High |
| Hit count | Yes | Yes | Yes | No | Yes | Yes | Yes | **No** | Very High |
| Logpoints | Yes | Yes | Yes | Yes | Yes | No | No | **No** | Very High |
| Function BP | Yes | Yes | No | Yes | Yes | Yes | Yes | **No** | Medium |
| Data BP | Yes | No | No | No | Yes | Yes | No | **No** | Very High |
| Instruction BP | Yes | No | No | Yes | Yes | No | No | **No** | Low |

#### Execution Control

| Feature | Spec | Common? | **st-dap** | PLC Relevance |
|---------|------|---------|------------|---------------|
| Continue/Next/StepIn/Out/Pause | Yes | Universal | **Yes** | Essential |
| Terminate | Yes | Common (5/6) | **No** | Medium |
| Restart | Yes | Common (3/6) | **No** | High |
| Step Back | Yes | Unique (CodeLLDB+rr) | **No** | Medium |
| Stepping granularity | Yes | Rare (2/6) | **No** | Low |

#### Variables & Evaluation

| Feature | Spec | Common? | **st-dap** | PLC Relevance |
|---------|------|---------|------------|---------------|
| Locals/Globals/Scopes | Yes | Universal | **Yes** | Essential |
| Watch expressions | Yes | Universal | **Yes** | Essential |
| Hover evaluation | Yes | Universal | **Yes** | Essential |
| FB/struct expansion | Implicit | Universal | **Yes** | Essential |
| **setVariable** | Yes | Common (5/6) | **No** | Very High |
| Completions | Yes | Common (3/6) | **No** | Medium |
| Value formatting (hex/bin) | Yes | Rare (2/6) | **No** | Medium |

#### Advanced Features

| Feature | Spec | Common? | **st-dap** | PLC Relevance |
|---------|------|---------|------------|---------------|
| Exception breakpoints | Yes | Common (5/6) | **No** | Medium |
| Loaded sources | Yes | Common (3/6) | **No** | Medium |
| Modules list | Yes | Rare (2/6) | **No** | Medium |
| Disassembly | Yes | Rare (3/6) | **No** | Low |
| Read/write memory | Yes | Rare (2/6) | **No** | Medium |
| Inline values | VS Code API | Universal | **No** | Very High |
| Progress reporting | Yes | Rare (0/6) | **No** | Medium |

---

## Capabilities Declaration Gap

Currently declared: `supportsConfigurationDoneRequest`, `supportsEvaluateForHovers`.

Should also declare (already supported): none additional currently — but as features are added, each new capability must be declared in the initialize response or VS Code hides the corresponding UI.

---

## Prioritized Roadmap

### Tier 1 — High-Impact DAP Conformance

1. **Conditional breakpoints** (`supportsConditionalBreakpoints`)
   - Break when `counter > 100`
   - Pass `SourceBreakpoint.condition` to debug engine, evaluate at breakpoint PC
   - All 6 major adapters support this

2. **Hit count breakpoints** (`supportsHitConditionalBreakpoints`)
   - Break on Nth scan cycle
   - Critical for PLC where you debug at the 50th iteration
   - 5/6 adapters support this

3. **Logpoints** (`supportsLogPoints`)
   - Log `"counter={counter}"` without stopping
   - Non-intrusive monitoring — essential for real-time PLC
   - 4/6 adapters support this

4. **setVariable** (`supportsSetVariable`)
   - Right-click variable → type new value
   - Currently requires `force` command in debug console
   - 5/6 adapters support this; users expect it

5. **Inline values** (VS Code `InlineValuesProvider` API)
   - Show variable values inline next to code during debug
   - Matches PLC "online view" of commercial IDEs
   - 6/6 adapters support this

6. **Trace/trend recording** (PLC-specific, WebView chart)
   - Oscilloscope-like recording of variable values over time
   - Data pipeline exists (WS streams per-cycle data)
   - Need: ring buffer + chart view (Chart.js/uPlot in WebView)
   - Most impactful PLC differentiator — CODESYS/TIA/TwinCAT all have it

### Tier 2 — Differentiators

7. **Data breakpoints** (`supportsDataBreakpoints`)
   - Break when a variable's value changes
   - "Why did my output turn on?" — PLC-specific killer feature
   - Our VM controls all storage, implementation straightforward
   - Only 2/6 adapters have it

8. **Execution marking** (PLC-specific, VS Code decorations)
   - Highlight lines that executed in last scan cycle
   - Unique to PLC IDEs (CODESYS green marking, TwinCAT)
   - VM tracks PC; emit line ranges via custom event

9. **Function breakpoints** (`supportsFunctionBreakpoints`)
   - Break on FB call by name
   - 4/6 adapters support this

10. **Completions** (`supportsCompletionsRequest`)
    - Autocomplete variable names in debug console
    - Catalog data already available

11. **Restart** (`supportsRestartRequest`)
    - Restart PLC program without restarting debug session
    - Maps to "warm restart" in PLC terms
    - 3/6 adapters support this

### Tier 3 — Polish

12. **loadedSources** — list all project source files (data exists in `project_files`)
13. **modules** — list loaded POUs (PROGRAM, FB, FUNCTION)
14. **Exception breakpoints** — break on runtime faults (div by zero, array OOB, watchdog)
15. **Progress reporting** — compile/download progress indicators
16. **Value formatting** — hex/decimal/binary display for bitmask I/O variables
17. **Terminate request** — explicit terminate vs. disconnect

### Tier 4 — Future

18. Step back / reverse debugging (requires cycle recording)
19. Disassembly view (VM bytecode)
20. Multiple threads as multiple IEC tasks
21. Read/write memory

---

## Competitive Position

| vs. | We match on | We're missing |
|-----|-------------|---------------|
| **CODESYS** | Force/unforce, online change, watch tables, cycle monitor, simulation | Trace/trend, power flow, conditional BP |
| **TIA Portal** | Watch/force tables, breakpoints while running | Trace, conditional BP, data watchpoints |
| **TwinCAT** | Remote monitoring, modern editor | Mini Scope (inline trends), execution marking |

**Our unique advantages**: VS Code ecosystem, cross-platform, WebSocket monitoring (same path local/remote), simulated VM without hardware, open protocol.

---

## Watch Tables — file-backed design

Watch tables replace the current `workspaceState`-backed flat-list watch list with
**project-folder YAML files** that sit next to `plc-project.yaml`. The extension
loads all `*.st-watch` files on panel open, shows one tab per table, and
auto-saves every user edit back to disk. This is not a migration — we replace the
storage wholesale.

### Why file-backed

- **Git-trackable / portable** — checking in `*.st-watch` means a teammate or CI
  job opens the project with the same watch set the author had. `workspaceState`
  is per-machine and opaque to version control.
- **Feature headroom** — per-row comments, display formats, pre-configured force
  values, trigger expressions all require structured storage. A typed YAML
  schema scales; a key/value store does not.
- **One source of truth** — the only ephemeral UI state left in `workspaceState`
  is "which tab is active"; that's session-local, not project state.

### File layout

```
project-root/
├── plc-project.yaml
├── default.st-watch            # auto-loaded; "Default" tab
├── commissioning.st-watch      # "Commissioning" tab
└── troubleshoot-filler.st-watch
```

- Files are discovered by scanning the project root (resolved the same way
  `plc-project.yaml` is today — workspace folder, editor ancestry walk).
- Tab display name comes from the `name:` key in the YAML, not the filename
  (so users can rename tabs without renaming files); filename is the persistent
  key (rename-tab = rename-file).

### Schema

```yaml
# default.st-watch
name: Default                   # tab label (string)
expanded:                       # paths currently expanded in the tree
  - Main.filler
  - Main.filler.counter
rows:
  - path: Main.counter
    comment: "Cycle count since last reset"
    format: dec                 # dec | hex | bin | bool | ascii | float
    force_value: null           # one-click force target; null if unset
  - path: Main.filler.CV
    comment: ""
    format: hex
    force_value: "16#FF"
  - path: Main.status
    format: bool                # every field except `path` is optional
```

Default values: `comment: ""`, `format: dec`, `force_value: null`, `expanded: []`.
Unknown fields fail the load with a diagnostic — we're not committing to
free-form extensibility until the feature matures.

### Runtime behavior

- **Load** on panel open: scan project root for `*.st-watch`, parse each, build
  one `WatchTable` object per file, populate the tab strip.
- **Save** on every user edit: debounced (~200 ms) write of the affected table
  only. Writes go through `vscode.workspace.fs` so VS Code's file watchers see
  the change and external edits round-trip cleanly.
- **External edits** (user edits the YAML by hand in another editor): watch the
  directory; on change, reload the affected file and reconcile with the panel.
- **New table**: tab strip `+` button creates `<slug>.st-watch` with a sensible
  default name.
- **Delete table**: confirm, then `vscode.workspace.fs.delete`.
- **Rename table**: prompt for new name; if new filename is available, rename
  the file and update in-memory state.
- **Active tab**: stored in `workspaceState` under `plcMonitor.activeWatchTable`
  (not in the YAML — it's UI state, not project state).

### YAML parsing

The tree-sitter-regex parser in `editors/vscode/src/extension.ts:577-627` is
replaced workspace-wide by **`js-yaml`** for the TypeScript side and
**`serde_yaml`** (already a workspace dep) everywhere in Rust. No regex parsing
survives. `plc-project.yaml` target resolution switches to `js-yaml` as part of
the same change.

### Tree rendering

The hand-rolled recursive `WatchTable.tsx` is retired. We adopt **TanStack
Table v8** + **TanStack Virtual**, with optional **@dnd-kit/core** for row
drag-reorder, all under `preact/compat`. TanStack is headless (no fiber
internals, works cleanly with Preact 10.19+), supports hierarchical rows via
`getSubRows`, lets each column render arbitrary DOM (force buttons, format
dropdown, editable comment field, editable force-value field), and ships at
~25-35 KB gzipped. Detailed selection rationale is in
`/tmp/test-reports/` (session research, 2026-04-24).

### 20 Hz update strategy

Live values push through **`@preact/signals`**, one signal per watched variable.
The TanStack row model stays static; only the leaf `<td>` re-renders on each
WS tick. This decouples update frequency from table size — 1000 rows at 20 Hz
is still only "changed cells per second" repaints, not "rows × fps".

### Feature parity vs. CODESYS watch/force tables

| Feature | CODESYS | Ours (target) |
|---|---|---|
| Multiple named tables | ✅ | ✅ |
| Project-file persistence (git-trackable) | ❌ (proprietary) | ✅ |
| Per-row comment | ✅ | ✅ |
| Per-row display format | ✅ | ✅ |
| One-click force to pre-configured value | ✅ | ✅ |
| Drag-reorder rows | ✅ | ✅ |
| Trigger expressions | ✅ | Tier 2 |
| Snapshot/compare | ✅ | Tier 2 |
| Charting (sparkline) | ✅ | Tier 2 |
| TIA Portal `.tww` import | — | Tier 3 |

---

## Key Files

| File | Purpose |
|------|---------|
| `crates/st-dap/src/server.rs` | DAP server (launch mode, 2509 lines) |
| `crates/st-target-agent/src/dap_attach_handler.rs` | DAP attach handler (807 lines) |
| `crates/st-target-agent/src/dap_proxy.rs` | DAP TCP proxy (routes launch vs attach) |
| `crates/st-dap/tests/dap_integration.rs` | 45 DAP integration tests |
| `crates/st-target-agent/tests/dap_proxy_integration.rs` | Attach proxy tests |
| `crates/st-engine/tests/dap_breakpoint_test.rs` | Engine-level breakpoint tests |
| `editors/vscode/src/extension.ts` | VS Code DAP factory + tracker |
| `editors/vscode/package.json` | Debug adapter schema + commands |
