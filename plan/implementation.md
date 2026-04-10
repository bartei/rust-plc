# IEC 61131-3 Compiler + LSP + Online Debugger — Implementation Plan

> **See also:**
> - [implementation_comm.md](implementation_comm.md) — communication layer (Phase 13)
> - [implementation_native.md](implementation_native.md) — LLVM native compilation + hardware targets (Phase 14)

## Project Overview

A Rust-based IEC 61131-3 Structured Text compiler with LSP support, online debugging via DAP, a bytecode VM runtime, and a VSCode extension (TypeScript). Architecture follows the same model as `rust-analyzer`: Rust core process + thin TypeScript VSCode extension.

---

## Phases 0–11: Core Platform (COMPLETED)

All foundational phases are complete. 714+ tests, zero clippy warnings.

| Phase | Scope | Status |
|-------|-------|--------|
| **0** | Project scaffolding, workspace, CI, VSCode extension scaffold | Done |
| **1** | Tree-sitter ST grammar (case-insensitive, incremental, 11 tests) | Done |
| **2** | AST types + CST→AST lowering (21 tests) | Done |
| **3** | Semantic analysis: scopes, types, 30+ diagnostics (127 tests) | Done |
| **4** | LSP server skeleton + VSCode extension (hover, diagnostics, go-to-def, semantic tokens) | Done |
| **5** | Advanced LSP (completion, signature help, rename, formatting, code actions, multi-file workspace) | Done |
| **6** | Register-based IR + AST→IR compiler (50+ instructions, 35 tests) | Done |
| **7** | Bytecode VM + scan cycle engine + stdlib + pointers (31 tests + stdlib tests) | Done |
| **8** | DAP debugger (breakpoints, stepping, variables, force/unforce, 30 tests) | Done |
| **9** | Online change manager (hot-reload with variable migration, 30 tests) | Done |
| **10** | WebSocket monitor server + VSCode panel (26 tests) | Done |
| **11** | CLI tool (check, run, serve, debug, compile, fmt, --json) | Done |

### Multi-file IDE support (completed during Phase 12 work):
- [x] LSP: project-aware analysis (discovers plc-project.yaml, includes all project files)
- [x] LSP: cross-file go-to-definition (opens the correct file at the symbol)
- [x] LSP: cross-file type resolution (hover shows correct type info)
- [x] DAP: multi-file project loading and compilation
- [x] DAP: per-file source mapping for stack traces (correct file + line per frame)
- [x] DAP: breakpoints work in any project file (accumulated per-file, correct source resolution)
- [x] DAP: step-into crosses file boundaries correctly
- [x] DAP: Initialized event after Launch (per DAP spec, so breakpoints arrive after VM exists)
- [x] JSON Schema for plc-project.yaml and device profiles (VS Code autocompletion)

### LSP features (all completed):
- [x] `textDocument/selectionRange` — smart expand/shrink selection (AST-based
      nesting: word → expression → statement → IF/FOR/WHILE body → VarBlock →
      POU → file. 4 integration tests.)
- [x] `textDocument/inlayHint` — parameter name hints at function/FB call sites
      for positional arguments (e.g., `Add(/*a:*/ 10, /*b:*/ 20)`). Skips named
      args and args whose text matches the param name. 3 integration tests.
- [x] `textDocument/onTypeFormatting` — auto-indent after Enter (increases
      indent after THEN/DO/VAR/PROGRAM/etc., holds for END_*) and reindent
      END_* lines after typing `;`. 4 integration tests.
- [x] `textDocument/callHierarchy` — full call hierarchy with incoming calls
      (who calls this?) and outgoing calls (what does this call?). Resolves
      across all open documents. Supports FUNCTION, FB, PROGRAM, and CLASS
      METHOD. 5 integration tests.
- [x] `textDocument/linkedEditingRange` — highlights matching keyword pairs
      (IF↔END_IF, FOR↔END_FOR, PROGRAM↔END_PROGRAM, VAR↔END_VAR, etc.)
      so VS Code can show them linked. Covers all 19 IEC 61131-3 block
      keyword pairs. AST-aware nesting resolution. 5 integration tests.

### Multi-file infrastructure fixes (completed in this session):
- [x] **Diagnostic routing**: `parse_multi()` now shifts all byte ranges to a
      virtual concatenated coordinate system so diagnostics from file A never
      appear in file B. LSP + DAP + breakpoints all updated to use virtual
      offsets. (Was causing `ramp_step` warning from conveyor.st to show in
      fill_controller.st.)
- [x] **Compiler FB field index bug**: FBs compiled AFTER their callers had
      empty locals → all field accesses resolved to index 0. Fixed by compiling
      FBs/FUNCTIONs before PROGRAMs in a separate pass.
- [x] **`VarType::FbInstance` propagation**: compiler now sets proper
      `VarType::FbInstance(func_idx)` on FB locals instead of placeholder
      `VarType::Int`, enabling the debugger to detect and expand FB instances.
- [x] **Debugger FB field display**: `current_locals_with_fb_fields`,
      `resolve_fb_field`, hierarchical Variables panel (tree via
      `variablesReference`), Watch panel Evaluate with expandable FBs.
- [x] **PLC Monitor panel tree view**: recursive `buildSubTree` + `renderTree`
      for unlimited nesting depth. Playwright UI test framework (14 tests).

### Remaining minor items:
- [ ] Online change: DAP custom request + VSCode toolbar
- [ ] Monitor: trend recording / time-series chart
- [x] Monitor: cross-reference view — implemented as LSP Call Hierarchy
      (`textDocument/prepareCallHierarchy` + `callHierarchy/incomingCalls` +
      `callHierarchy/outgoingCalls`). This is the standard LSP mechanism for
      cross-referencing; VS Code renders it as the "Call Hierarchy" panel
      accessible via `Shift+Alt+H` or right-click → Show Call Hierarchy.

---

## Phase 12: IEC 61131-3 Object-Oriented Extensions — Classes (COMPLETED)

Full implementation of CLASS, METHOD, INTERFACE, PROPERTY across the entire pipeline.
Grammar → AST → Semantics → Compiler → IR → VM, with multi-file support.

**199 new tests** covering: grammar parsing, semantic analysis (inheritance, interfaces,
abstract/final, access specifiers, THIS/SUPER), compiler (method compilation, vtable,
inherited vars), runtime (method return values, state persistence, instance isolation,
cross-file calls, pointer integration), and DAP integration.

**5 single-file playground examples** (10–14) + **1 multi-file OOP project** (oop_project/).

**Runtime bugs found and fixed during playground testing:**
- Methods couldn't access class instance variables
- Method return values lost (return_reg protocol mismatch)
- Inherited fields invisible to subclass methods
- Pointer cross-function dereference read wrong frame
- Pointer vs NULL comparison always returned equal
- StoreField unimplemented in compiler + VM
- Nested class instances inside different FB instances shared state

### Remaining Phase 12 items:
- [ ] Constructor/destructor support (FB_INIT / FB_EXIT pattern)
- [ ] Online change compatibility with classes

---

## Cross-Cutting Concerns

- [x] **Testing:** 502 tests across 10 crates — unit, integration, LSP protocol, DAP protocol, WebSocket, end-to-end
- [x] **CI/CD:** GitHub Actions (check, test, clippy, audit, cargo-deny, docs deploy), release-plz for semver
- [x] **Documentation:** mdBook site (20+ pages) with architecture, tutorials, language reference, stdlib docs
- [x] **Tracing / logging:** DAP server logs to stderr + Debug Console, `tracing` crate available throughout
- [x] **Devcontainer:** Full VSCode dev environment with auto-build, extension install, playground
- [x] **Error quality:** Line:column source locations, severity levels, diagnostic codes
- [ ] **IEC 61131-3 compliance tracking:** Maintain a checklist of spec sections implemented vs. pending

---

## Dependency Graph

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
Phase 12 (LLVM) — independent, after Phase 6
```