# IEC 61131-3 Compiler + LSP + Online Debugger — Implementation Plan

## Project Overview

A Rust-based IEC 61131-3 Structured Text compiler with LSP support, online debugging via DAP, a bytecode VM runtime, and a VSCode extension (TypeScript). Architecture follows the same model as `rust-analyzer`: Rust core process + thin TypeScript VSCode extension.

---

## Phase 0: Project Scaffolding & Workspace Setup

- [x] Convert to a Cargo workspace with the following crates:
  - `st-grammar` — tree-sitter ST grammar (C grammar + Rust bindings)
  - `st-syntax` — AST types, tree-sitter → AST conversion
  - `st-semantics` — semantic analysis, type checking, symbol tables
  - `st-ir` — intermediate representation / bytecode definitions
  - `st-compiler` — AST → IR lowering
  - `st-runtime` — bytecode VM, scan cycle engine, task scheduler
  - `st-lsp` — LSP server (tower-lsp)
  - `st-dap` — DAP server (debug adapter)
  - `st-monitor` — online monitoring WebSocket server
  - `st-cli` — CLI entry point (compile, run, serve)
- [x] Add shared dependencies to workspace `Cargo.toml`:
  - `tokio`, `serde`, `serde_json`, `thiserror`, `tracing`, `tower-lsp`, `lsp-types`, `dap`
- [x] Set up CI (GitHub Actions): `cargo check`, `cargo test`, `cargo clippy`, `cargo fmt --check`
- [x] Create `.gitignore` for Rust (`/target`, etc.) and Node (`node_modules`, etc.)
- [x] Create the VSCode extension scaffold under `editors/vscode/` (TypeScript, `package.json`, `tsconfig.json`)

---

## Phase 1: Tree-Sitter ST Grammar

This is the foundation — everything else depends on parsing.

- [x] Create `st-grammar/` with tree-sitter project structure (`grammar.js`, `src/`, `tree-sitter.json`)
- [x] Define grammar for core ST constructs:
  - [x] Literal types: INTEGER, REAL, STRING, BOOL, NULL, TIME, DATE, TOD, DT, typed literals
  - [x] Variable declarations: `VAR`, `VAR_INPUT`, `VAR_OUTPUT`, `VAR_IN_OUT`, `VAR_GLOBAL`, `VAR_EXTERNAL`, `VAR_TEMP` with `RETAIN`/`PERSISTENT`/`CONSTANT` qualifiers
  - [x] Data types: elementary types, arrays, structs, enumerations, subranges, STRING/WSTRING with length, REF_TO pointers
  - [x] Program Organization Units (POUs): `PROGRAM`, `FUNCTION`, `FUNCTION_BLOCK`
  - [x] Expressions: arithmetic, boolean, comparison, parenthesized, function calls, power, unary, pointer dereference (`^`)
  - [x] Statements: assignment (`:=`), `IF/ELSIF/ELSE/END_IF`, `CASE/END_CASE`, `FOR/END_FOR`, `WHILE/END_WHILE`, `REPEAT/UNTIL/END_REPEAT`, `RETURN`, `EXIT`
  - [x] FB instantiation and method calls (`fbInstance(IN1 := val)`)
  - [x] Comments: `//` line, `(* ... *)` block, `/* ... */` block
  - [x] Case-insensitive keywords throughout
- [x] Generate the C parser with `tree-sitter generate` (ABI 15)
- [x] Create Rust bindings with `build.rs` (cc-compiled C parser, `language()` fn, node `kind` constants)
- [x] Write parser tests (11 tests): valid programs, error recovery on broken syntax
- [x] Validate incremental parsing works (edit a buffer, re-parse, only changed nodes regenerate)

---

## Phase 2: AST & Syntax Layer (`st-syntax`)

- [x] Define Rust AST types mirroring the grammar:
  - `SourceFile`, `ProgramDecl`, `FunctionDecl`, `FunctionBlockDecl`, `VarBlock`, `VarDeclaration`, `Statement`, `Expression`, `DataType`, `Literal`, `VariableAccess`, `QualifiedName`
- [x] Implement tree-sitter CST → AST conversion (`lower.rs`: walks CST, builds typed AST)
- [x] Track source spans (`TextRange`) on every AST node for LSP location mapping
- [x] Handle parse errors gracefully: collect CST ERROR/MISSING nodes, produce partial ASTs
- [x] Unit tests (21 tests): programs, functions, FBs, types, control flow, expressions, literals, struct access, error recovery, multi-POU files

---

## Phase 3: Semantic Analysis (`st-semantics`)

- [x] **Symbol Table / Scope Resolution** (`scope.rs`)
  - [x] Build hierarchical scope model: global → POU → block (ScopeId tree)
  - [x] Resolve variable references to declarations (case-insensitive)
  - [x] Resolve POU references (function calls, FB instantiations) with forward references
  - [x] Handle `VAR_INPUT`, `VAR_OUTPUT`, `VAR_IN_OUT` binding at call sites
  - [x] Two-pass analysis: register all top-level names first, then analyze bodies
- [x] **Type System** (`types.rs`)
  - [x] Semantic `Ty` enum: Elementary, Array, String, Struct, Enum, Subrange, FunctionBlock, Alias, Void, Unknown
  - [x] Full IEC 61131-3 type hierarchy with numeric ranking for coercion
  - [x] Type inference for all expression forms (literals, binary, unary, function calls, variable access)
  - [x] Type coercion rules: implicit widening allowed, narrowing rejected
  - [x] Array type checking: dimension count, index type (must be integer)
  - [x] Struct field access validation (including nested structs, FB member access)
  - [x] Enum type support
- [x] **Diagnostic Collection** (`diagnostic.rs`, `analyze.rs`) — 30+ diagnostic codes covering:
  - [x] Undeclared variable/POU/type errors
  - [x] Type mismatch: assignment, condition (IF/WHILE/REPEAT must be BOOL), case selectors, return values
  - [x] Invalid operand types for arithmetic, boolean, comparison, unary operators
  - [x] Duplicate declarations (variables, POUs, types)
  - [x] Function/FB call errors: unknown param, duplicate param, too many args, param type mismatch, not callable
  - [x] Array errors: index type mismatch, dimension mismatch, indexing non-array
  - [x] Struct errors: no such field, field access on non-struct
  - [x] Control flow: EXIT outside loop, FOR variable must be integer, FOR bounds must be integer
  - [x] Unused variable / unused parameter warnings
  - [x] Variable never assigned warnings
  - [x] Shadowed variable warnings
  - [x] Dead code warnings (unreachable after RETURN)
- [x] **Comprehensive tests** (127 semantic tests across 6 test files):
  - `scope_tests.rs` (22 tests): resolution success/failure, forward refs, case-insensitive, duplicates, shadowing
  - `type_tests.rs` (38 tests): assignment compatibility, widening/narrowing, arithmetic, boolean, comparison, unary, conditions
  - `call_tests.rs` (13 tests): named/positional args, FB instances, unknown/duplicate params, type mismatches, coercion
  - `control_flow_tests.rs` (16 tests): FOR/WHILE/REPEAT, EXIT inside/outside loops, CASE selectors, dead code
  - `struct_array_tests.rs` (11 tests): field access, nested structs, array indexing, dimension mismatch
  - `warning_tests.rs` (10 tests): unused vars, never-assigned, underscore suppression, shadowing
  - `end_to_end_tests.rs` (17 tests): realistic PLC programs (PID controller, state machine, array processing), multi-POU, convenience API

---

## Phase 4: LSP Server Skeleton (`st-lsp`)

Ship a working LSP loop early to prove the VSCode integration.

- [x] Set up `tower-lsp` server with `tokio` runtime (`server.rs`)
- [x] Implement document synchronization (`textDocument/didOpen`, `didChange`, `didClose`)
- [x] Maintain per-document state: source text, tree-sitter tree, AST, semantic info (`document.rs`)
- [x] Re-parse and re-analyze on `didChange` (full sync mode)
- [x] **Initial LSP capabilities:**
  - [x] `textDocument/publishDiagnostics` — parse errors + all 30+ semantic diagnostic codes
  - [x] `textDocument/hover` — type info, variable kind, function signatures with params
  - [x] `textDocument/definition` — jump to variable/POU declaration
  - [x] `textDocument/semanticTokens/full` — 10 token types (keyword, type, variable, function, parameter, number, string, comment, operator, enum member)
- [x] Wire up CLI: `st-cli serve` starts LSP on stdio, `st-cli check <file>` runs offline diagnostics
- [x] **VSCode extension:**
  - [x] Language configuration (`language-configuration.json`): bracket pairs, comments, auto-closing, folding markers
  - [x] TextMate grammar (`syntaxes/structured-text.tmLanguage.json`): keywords, types, comments, strings, numbers, operators, time/date literals
  - [x] Extension activates on `.st`, `.scl` files
  - [x] Spawns `st-cli serve --stdio` on activation
  - [x] Configurable server path via `structured-text.serverPath` setting
  - [x] Semantic token scope mapping for theme integration

---

## Phase 5: Advanced LSP Features

- [x] **`textDocument/completion`** (`completion.rs`)
  - [x] ST keywords with snippet templates (IF...END_IF, FOR...END_FOR, PROGRAM, FUNCTION, etc.)
  - [x] Variable names in scope (walks scope chain)
  - [x] POU/FB names with function signature snippets (auto-fills parameter names)
  - [x] Struct field names after `.` (dot-triggered)
  - [x] FB member access after `.` (outputs + inputs)
  - [x] Elementary type names
  - [x] User-defined type names
- [x] **`textDocument/documentSymbol`** — outline view with nested POUs, variables, types
- [x] **LSP integration tests** (13 tests): initialize, diagnostics, hover, go-to-def, semantic tokens, completion (variables, dot-struct, function snippets), document symbols, multi-document, close/clear
- [x] **VSCode extension e2e test scaffolding** (`@vscode/test-electron`): language registration, .st recognition, diagnostics verification
- [x] `textDocument/signatureHelp` — parameter hints for function/FB calls (trigger on `(` and `,`)
- [x] `textDocument/references` — find all usages of a variable/POU (case-insensitive, whole-word)
- [x] `textDocument/rename` — rename variables/POUs across the file
- [x] `textDocument/formatting` — auto-format ST source (indent normalization)
- [x] `textDocument/codeAction` — quick fix: declare undeclared variable as INT
- [x] **Multi-file workspace support** (`project.rs`):
  - [x] **Autodiscovery** (default, zero-config): recursively walks directories for `.st`/`.scl` files, skips `.hidden/`, `target/`, `node_modules/`
  - [x] **Project file** (`plc-project.yaml`, optional): name, entryPoint, sources (globs), libraries, exclude patterns
  - [x] **Discovery logic**: yaml > explicit sources > autodiscovery, always prepends stdlib
  - [x] **CLI integration**: `st-cli run` (current dir), `st-cli run dir/` (project dir), `st-cli run file.st` (single file), `st-cli check` (same modes)
  - [x] **LSP workspace-aware**: documents analyzed with stdlib context for cross-file POU resolution
  - [x] 13 project discovery tests + multi-file playground example (`playground/multi_file_project/`)
  - [ ] Cross-file go-to-definition (open other file at declaration) — future
  - [ ] Cross-file diagnostics (errors check all project files) — future

---

## Phase 6: Intermediate Representation & Bytecode (`st-ir`, `st-compiler`)

- [x] **Register-based IR instruction set** (`st-ir`):
  - [x] Core types: `Value` (Bool, Int, UInt, Real, String, Time, Ref, Null, Void), `VarType`, `Reg`, `Label`
  - [x] 50+ instructions: `LoadConst`, `Move`, `LoadLocal/StoreLocal`, `LoadGlobal/StoreGlobal`, arithmetic (`Add/Sub/Mul/Div/Mod/Pow/Neg`), comparison (`CmpEq/Ne/Lt/Gt/Le/Ge`), logic (`And/Or/Xor/Not`), math intrinsics (`Sqrt/Sin/Cos/Tan/Asin/Acos/Atan/Ln/Log/Exp`), `SystemTime`, conversion (`ToInt/ToReal/ToBool`), control flow (`Jump/JumpIf/JumpIfNot`), calls (`Call/CallFb/Ret/RetVoid`), struct/array (`LoadArray/StoreArray/LoadField/StoreField`), pointers (`MakeRefLocal/MakeRefGlobal/Deref/DerefStore/LoadNull`)
  - [x] `Module` (functions + globals + type defs), `Function` (instructions + locals + source map + labels), `MemoryLayout` (slots with offsets)
  - [x] Serde serialization for offline storage
- [x] **AST → IR compiler** (`st-compiler`):
  - [x] Two-pass compilation (register POUs, then compile bodies)
  - [x] Expression compilation (all binary/unary ops, literals, variables, function calls)
  - [x] Control flow: IF/ELSIF/ELSE → conditional jumps, FOR → loop with init/test/increment, WHILE/REPEAT → loop patterns, CASE → selector comparison + jump table, EXIT → jump to loop exit label
  - [x] Function calls with positional and named argument compilation
  - [x] FB instance calls (`CallFb`) with instance slot tracking
  - [x] Variable addressing: local slot allocation per POU, global slot tracking
  - [x] Source map generation: instruction index → source byte range
  - [x] RETURN statement compilation
- [x] **Tests** (35 tests): basic compilation, vars with init, arithmetic, comparisons, boolean logic, unary, power/mod, all 6 comparison ops, IF/ELSIF/ELSE, FOR/BY, WHILE, REPEAT, CASE with ranges, EXIT, RETURN, function calls, global vars, literals (bool/real/string), value conversions, type sizes, memory layout, module find

---

## Phase 7: PLC Runtime / Scan Cycle Engine (`st-runtime`)

- [x] **Bytecode VM interpreter** (`vm.rs`):
  - [x] Fetch-decode-execute loop for all 30+ instruction types
  - [x] Register file per call frame, local variable slots, global variable storage
  - [x] Function call stack with argument passing and return values
  - [x] Arithmetic: Add, Sub, Mul, Div (with division-by-zero detection), Mod, Pow, Neg
  - [x] Comparison: Eq, Ne, Lt, Gt, Le, Ge (with int/real dispatch)
  - [x] Logic: And, Or, Xor, Not
  - [x] Control flow: Jump, JumpIf, JumpIfNot (label resolution)
  - [x] Function calls: Call (with return value), CallFb (FB instance), Ret, RetVoid
  - [x] Type conversion: ToInt, ToReal, ToBool
  - [x] Safety: configurable max call depth (stack overflow protection), max instruction limit (infinite loop protection)
- [x] **Scan cycle engine** (`engine.rs`):
  - [x] Cyclic execution: runs program N cycles or indefinitely
  - [x] Cycle statistics: count, min/max/avg cycle time, total time
  - [x] Watchdog timeout per cycle
  - [x] Global variable persistence across cycles
  - [x] VM access for variable inspection/manipulation
- [x] **CLI integration**: `st-cli run <file> [-n <cycles>]` — full pipeline from source to execution
- [x] **Tests** (31 tests): arithmetic (sum, sub, div, mod, power, real), boolean logic, all 6 comparisons, IF/ELSIF, FOR/BY, WHILE, REPEAT, CASE, EXIT, RETURN, function-calling-function, global variable persistence, cycle stats, division by zero, execution limit, Fibonacci algorithm
- [x] **Standard library** (modular ST in `stdlib/`, auto-included via `builtin_stdlib()`):
  - [x] Counters: CTU, CTD, CTUD | Edge detection: R_TRIG, F_TRIG
  - [x] Timers: TON, TOF, TP — real-time using `SYSTEM_TIME()` and `TIME` values (e.g., `PT := T#5s`)
  - [x] Math: MAX/MIN/LIMIT/ABS (INT+REAL), SEL
  - [x] Trig/math intrinsics: SQRT, SIN, COS, TAN, ASIN, ACOS, ATAN, LN, LOG, EXP (VM instructions)
  - [x] Type conversions: 30+ `*_TO_*` intrinsics (INT_TO_REAL, REAL_TO_INT, BOOL_TO_INT, etc.)
  - [x] `SYSTEM_TIME()` intrinsic — returns elapsed milliseconds since engine start
  - [x] Multi-file compilation, FB instance persistence, FB field access
  - [x] 16 integration tests + 3 playground examples (07_stdlib_demo, 08_custom_module, 09_pointers)
- [x] **Pointers** (`REF_TO`, `^`, `NULL`):
  - [x] `REF_TO <type>` — typed pointer declarations
  - [x] `REF(variable)` — take reference of a variable
  - [x] `ptr^` — dereference (read and write)
  - [x] `NULL` — null pointer literal (safe deref returns default)
  - [x] VM: `Value::Ref(scope, slot)`, `MakeRefLocal/MakeRefGlobal`, `Deref/DerefStore`, `LoadNull`
  - [x] 6 pointer tests + playground example
- [x] Debug hooks (breakpoints, stepping) — completed in Phase 8

---

## Phase 8: DAP Server — Online Debugging (`st-dap`)

- [x] **VM debug hooks** (`debug.rs`):
  - [x] `DebugState`: breakpoints (source-level + instruction-level), step modes, pause state
  - [x] `StepMode`: Continue, StepIn, StepOver, StepOut, Paused
  - [x] `should_pause()` — pre-instruction check for breakpoints and step completion
  - [x] Source-line-to-bytecode breakpoint mapping via source map
  - [x] `continue_execution()` — resume VM from halted debug state
  - [x] Frame inspection: `stack_frames()`, `current_locals()`, `global_variables()`
  - [x] Value formatting for debugger display (IEC format: TIME as `T#5ms`, BOOL as TRUE/FALSE)
- [x] **DAP server** (`server.rs`):
  - [x] `initialize` — capabilities advertisement
  - [x] `launch` — compile source, start VM paused on entry
  - [x] `setBreakpoints` — map source lines to bytecode via source map
  - [x] `continue`, `next` (step over), `stepIn`, `stepOut`
  - [x] `stackTrace` — POU call stack with source locations
  - [x] `scopes` — Locals and Globals scopes
  - [x] `variables` — read variable values formatted per IEC type
  - [x] `evaluate` — look up variable by name in locals/globals
  - [x] `pause` — halt at next instruction
  - [x] `disconnect` / `configurationDone` / `threads`
  - [x] Stopped/Terminated/Output events
- [x] **CLI**: `st-cli debug <file>` starts DAP server on stdin/stdout
- [x] **VSCode extension**: debugger contribution with `st` type, launch configuration
- [x] **Tests** (16 DAP integration + 9 debug unit = 25 tests):
  - Initialize, launch with stop-on-entry, threads, stack trace, scopes+variables
  - StepIn, StepOver, StepOut, Continue to completion
  - Set breakpoints, Evaluate expressions, Pause, Disconnect
  - Full debug session (multi-step with stack trace)
  - Debug state unit tests: breakpoints, step modes, pause/resume, value formatting
- [x] **PLC-specific DAP extensions** (via evaluate expressions): `force x = 42`, `unforce x`, `listForced`, `scanCycleInfo`
- [x] **VSCode debug toolbar** — Force/Unforce/ListForced/CycleInfo buttons (only visible during ST debug sessions)

---

## Phase 9: Online Change Manager

- [x] **Type compatibility checker** (`online_change.rs`):
  - [x] Compare old/new POU signatures: `Unchanged`, `CodeOnly`, `VarsAdded`, `LayoutChanged`, `New`, `Removed`
  - [x] Track preserved, new, and removed variables per POU
  - [x] Reject incompatible: type changes, function removal, global type changes
- [x] **Retain variable preservation:**
  - [x] `migrate_locals()` — name+type based mapping, handles reordering
  - [x] New vars get defaults, removed vars dropped, preserved vars kept
- [x] **Atomic swap:**
  - [x] `vm.swap_module()` — replaces module + globals + retained locals atomically between cycles
  - [x] `engine.online_change(source)` — full pipeline: parse → compile → analyze → migrate → swap
- [x] **Tests** (20 unit + 10 integration = 30 tests): compatibility analysis, variable migration, end-to-end hot-reload with counter preservation, multiple sequential changes, incompatible rejection
- [ ] DAP custom request + VSCode toolbar — future

---

## Phase 10: Monitor Server & Custom VSCode UI (`st-monitor`)

- [x] **WebSocket monitor server** (`server.rs`):
  - [x] JSON-RPC protocol over WebSocket (subscribe, unsubscribe, read, write, force, unforce, getCycleInfo, onlineChange)
  - [x] `MonitorHandle` for engine integration (push variable updates, forced vars, online change)
  - [x] `MonitorState` shared between server and engine (variables, cycle info, forced vars)
  - [x] Multi-client support (each client has independent subscriptions)
  - [x] Invalid message handling with error responses
- [x] **Monitor protocol** (`protocol.rs`):
  - [x] 8 request types: Subscribe, Unsubscribe, Read, Write, Force, Unforce, GetCycleInfo, OnlineChange
  - [x] 4 message types: Response, VariableUpdate, CycleInfo, Error
  - [x] Full serde serialization/deserialization
- [x] **VSCode extension panels:**
  - [x] `MonitorPanel` webview — live variable table with values, types, force/unforce buttons
  - [x] Scan cycle statistics display (cycle count, last/min/max/avg cycle time)
  - [x] Force table with forced variable highlighting
  - [x] VSCode-native theming (uses CSS variables for colors)
  - [x] Command palette: "ST: Open PLC Monitor"
  - [x] Editor title menu button for quick access
- [x] **Tests** (19 WebSocket integration + 4 protocol serialization + 3 handle unit = 26 tests):
  - Subscribe/unsubscribe, read variables, read nonexistent, force/unforce, get cycle info
  - Online change request, invalid JSON handling, multiple clients
  - Protocol round-trip serialization for all request/message types
  - MonitorHandle update, forced vars, online change pending/consume
- [ ] Trend recording / time-series chart — future
- [ ] Cross-reference view — future

---

## Phase 11: CLI Tool (`st-cli`)

- [x] `st-cli check [path]` — parse + semantic analysis (single file, directory, or project)
- [x] `st-cli run [path] [-n N]` — compile and execute (single file, directory autodiscovery, or project yaml)
- [x] `st-cli run` (no args) — autodiscover from current directory
- [x] `st-cli serve` — start LSP server on stdio
- [x] `st-cli debug <file>` — start DAP debug server on stdio
- [x] Proper exit codes (0=ok, 1=errors)
- [ ] `st-cli compile <file> -o <output>` — compile to bytecode file
- [ ] `st-cli fmt <file>` — format source file
- [ ] Structured JSON error output for CI integration

---

## Phase 12 (Future): LLVM Backend

Optional — adds native compilation for production PLC targets.

- [ ] Integrate `inkwell` (Rust LLVM bindings)
- [ ] IR → LLVM IR lowering
- [ ] JIT compilation for development mode (fast iteration)
- [ ] AOT compilation for deployment (static binary for target platform)
- [ ] Adapt online change for native code (requires code relocation or process restart)
- [ ] Benchmark: VM vs LLVM-compiled cycle times

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