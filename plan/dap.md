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
