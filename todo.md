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
- [ ] **Additional LSP features (remaining):**
  - [x] `textDocument/documentHighlight` — highlight all occurrences of symbol under cursor
  - [x] `textDocument/foldingRange` — collapse PROGRAM/IF/VAR/FOR/WHILE/CASE blocks
  - [x] `textDocument/typeDefinition` — jump to TYPE declaration of a variable's type
  - [x] `workspace/symbol` — Ctrl+T search for any POU/type across workspace
  - [x] `textDocument/documentLink` — make file paths in comments clickable
  - [ ] `textDocument/selectionRange` — smart expand/shrink selection (word → expr → stmt → block)
  - [ ] `textDocument/inlayHint` — show inferred types, parameter names at call sites
  - [ ] `textDocument/onTypeFormatting` — auto-indent after `;` or `THEN`
  - [ ] `textDocument/callHierarchy` — show callers/callees of a function
  - [ ] `textDocument/linkedEditingRange` — edit matching IF/END_IF pairs simultaneously

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
- [x] `st-cli compile <file> -o <output>` — compile to JSON bytecode file
- [x] `st-cli fmt [path]` — format source file(s) in place (single file or project autodiscovery)
- [x] `--json` flag on `check` — structured JSON error output for CI integration

---

## Phase 12: IEC 61131-3 Object-Oriented Extensions (Classes)

Implement the OOP extensions from IEC 61131-3 Third Edition (Table 48).
Reference: [Fernhill CLASS Declaration](https://www.fernhillsoftware.com/help/iec-61131-3/common-elements/class-declaration.html)
Spec: [IEC 61131-3 Ed.3 §6.6.5](https://webstore.iec.ch/publication/4552) (official standard, paywalled)

- [x] **Grammar extensions:**
  - [x] `CLASS <name> [EXTENDS <base>] [IMPLEMENTS <iface1>, <iface2>] ... END_CLASS`
  - [x] `METHOD [PUBLIC|PRIVATE|PROTECTED] <name> [: <return_type>] ... END_METHOD`
  - [x] `INTERFACE <name> [EXTENDS <base_iface>] ... END_INTERFACE`
  - [x] `PROPERTY <name> : <type> ... END_PROPERTY` with `GET` and `SET` accessors
  - [x] Access specifiers: `PUBLIC`, `PRIVATE`, `PROTECTED`, `INTERNAL`
  - [x] `THIS` keyword for self-reference within methods
  - [x] `SUPER` keyword for calling base class methods
  - [x] `ABSTRACT` and `FINAL` modifiers on classes and methods
  - [x] `OVERRIDE` keyword for overriding virtual methods
- [x] **AST types:**
  - [x] `ClassDecl`: name, base class, interfaces, var blocks, methods, properties
  - [x] `MethodDecl`: access specifier, name, return type, var blocks, body, modifiers
  - [x] `InterfaceDecl`: name, base interfaces, method signatures
  - [x] `PropertyDecl`: name, type, get body, set body
- [x] **Semantic analysis:**
  - [x] Class member resolution (field access via `.` on class instances)
  - [x] Method dispatch (static for non-virtual, vtable for virtual/overridden)
  - [x] Access specifier enforcement (PRIVATE only within class, PROTECTED within hierarchy)
  - [x] Interface conformance checking (all methods implemented)
  - [x] Inheritance type checking (base class compatibility, diamond problem)
  - [x] THIS/SUPER resolution within method bodies
  - [x] ABSTRACT class cannot be instantiated
  - [x] FINAL class cannot be extended, FINAL method cannot be overridden
- [x] **Compiler / IR:**
  - [x] Class instance layout: method table pointer + field slots
  - [x] Method compilation: implicit THIS parameter as first argument
  - [x] Virtual method dispatch via vtable lookup instruction
  - [x] Property access compiled as GET/SET method calls
  - [x] SUPER calls compiled as direct (non-virtual) parent method call
  - [ ] Constructor/destructor support (FB_INIT / FB_EXIT pattern)
- [x] **VM:**
  - [x] Class instance storage (extends FB instance mechanism)
  - [x] VTable for virtual dispatch
  - [x] `CallMethod` instruction for method dispatch
- [ ] **Standard library updates:**
  - [ ] Refactor existing FBs as classes where appropriate
  - [ ] Interface examples (e.g., `IComparable`, `ISerializable`)
- [x] **Tests:**
  - [x] Class instantiation and field access
  - [x] Method calls (public/private/protected enforcement)
  - [x] Inheritance (single level, multi-level, method override)
  - [x] Interface implementation and conformance
  - [x] Abstract class / final class restrictions
  - [x] Properties (get/set)
  - [x] THIS/SUPER within methods
  - [x] Multi-file cross-boundary class/interface resolution
  - [x] Class composition (class inside FB, class inside class)
  - [x] Pointer cross-function scope correctness
  - [x] FB/class field read/write (StoreField/LoadField)
  - [x] Nested instance state isolation (class inside different FB instances)
  - [ ] Online change compatibility with classes
- [x] **Documentation:**
  - [x] Language reference page for Classes, Methods, Interfaces, Properties
  - [ ] Migration guide: FUNCTION_BLOCK to CLASS
  - [x] Playground examples
  - [x] Multi-file OOP project example (playground/oop_project)

---

## Phase 13: Communication Extension System & Modbus Implementation

A PLC is only useful if it can talk to the physical world. This phase establishes the
**communication extension architecture** — a modular, plugin-based system where each
protocol (Modbus, Profinet, EtherCAT, etc.) is an independent, versioned extension —
and delivers the first two implementations: Modbus TCP and Modbus RTU/ASCII.

### Design Principles

1. **Each protocol is an independent crate** — separately versioned, tested, and maintained
2. **No framework recompilation** — extensions are loaded via a trait interface at runtime
3. **Community extensible** — third parties can publish protocol extensions (like an app store)
4. **Device profiles** — reusable configurations for specific hardware (e.g., ABB ACS580 VFD
   registers, Siemens ET200 I/O module maps) that simplify setup for end users
5. **Cyclic + acyclic modes** — like CODESYS, TwinCAT, and Siemens: cyclic I/O happens every
   scan cycle, acyclic requests (parameter reads, diagnostics) happen on-demand

### Architecture

Follows OSI-inspired layer separation: **links** (Layer 1-2: physical transport) are
separate from **devices** (Layer 7: application protocol). A single link can carry
multiple devices (e.g., multiple Modbus slaves on one RS-485 bus).

```
┌─────────────────────────────────────────────────────────────┐
│                     ST Program (Layer 7)                      │
│   IF rack_left.DI_0 THEN rack_right.DO_3 := TRUE; END_IF;   │
│   pump_vfd.SPEED_REF := 45.0;                               │
│   fan_vfd.RUN := TRUE;  (* same bus, different address *)    │
└──────────────┬───────────────────────────┬──────────────────┘
               │ Read Inputs               │ Write Outputs
               │ (struct fields ← regs)    │ (struct fields → regs)
┌──────────────▼───────────────────────────▼──────────────────┐
│         Communication Manager (orchestrator)                 │
│                                                              │
│  Device Layer (protocol + profiles)                          │
│  ┌─────────────┐ ┌─────────────┐ ┌─────────────┐            │
│  │ rack_left   │ │ pump_vfd    │ │ fan_vfd     │            │
│  │ Modbus TCP  │ │ Modbus RTU  │ │ Modbus RTU  │            │
│  │ unit_id=1   │ │ unit_id=3   │ │ unit_id=4   │            │
│  │ wago_750    │ │ abb_acs580  │ │ abb_acs580  │            │
│  └──────┬──────┘ └──────┬──────┘ └──────┬──────┘            │
│         │               │               │                    │
│  Link Layer (physical transport)        │                    │
│  ┌──────▼──────┐ ┌──────▼───────────────▼──────┐            │
│  │ eth_rack_l  │ │ rs485_bus_1                  │            │
│  │ TCP         │ │ /dev/ttyUSB0, 19200 8E1      │            │
│  │ 192.168.1.  │ │ (shared by pump + fan VFDs)  │            │
│  │ 100:502     │ │                              │            │
│  └──────┬──────┘ └──────────────┬───────────────┘            │
└─────────┼───────────────────────┼────────────────────────────┘
          │                       │
    TCP/IP network          RS-485 bus
          │                       │
    ┌─────▼─────┐     ┌─────▼────┐ ┌─────▼────┐
    │  WAGO     │     │  ABB     │ │  ABB     │
    │  750-352  │     │  ACS580  │ │  ACS580  │
    │  I/O rack │     │  pump    │ │  fan     │
    └───────────┘     └──────────┘ └──────────┘
```

### Trait Architecture (Layered)

The trait design mirrors the link/device separation. Link traits manage the physical
transport. Device traits manage the protocol and register mapping. The Communication
Manager composes them.

```rust
/// Link layer: manages a physical transport channel.
/// One link can serve multiple devices (e.g., RS-485 bus with multiple slaves).
pub trait CommLink: Send + Sync {
    fn name(&self) -> &str;
    fn link_type(&self) -> &str;  // "tcp", "serial", "udp", etc.

    /// Open the physical channel with the configured settings.
    fn open(&mut self) -> Result<(), CommError>;
    fn close(&mut self) -> Result<(), CommError>;
    fn is_open(&self) -> bool;

    /// Raw data exchange (used by device layer).
    fn send(&mut self, data: &[u8]) -> Result<(), CommError>;
    fn receive(&mut self, buffer: &mut [u8], timeout_ms: u32) -> Result<usize, CommError>;

    fn diagnostics(&self) -> LinkDiagnostics;
}

/// Device layer: protocol-specific communication with a single addressable unit.
/// Reads/writes device registers and maps them to/from struct fields.
pub trait CommDevice: Send + Sync {
    fn name(&self) -> &str;
    fn protocol(&self) -> &str;  // "modbus-tcp", "modbus-rtu", "profinet", etc.

    /// Configure with the device section from plc-project.yaml.
    fn configure(&mut self, config: &serde_yaml::Value) -> Result<(), String>;

    /// Bind to a link (the device uses this link for all I/O).
    fn bind_link(&mut self, link: Arc<Mutex<dyn CommLink>>) -> Result<(), CommError>;

    /// Return the device profile (struct schema + register map).
    fn device_profile(&self) -> &DeviceProfile;

    /// Cyclic I/O: read input registers → struct field values.
    fn read_inputs(&mut self) -> Result<HashMap<String, Value>, CommError>;

    /// Cyclic I/O: struct field values → write output registers.
    fn write_outputs(&mut self, outputs: &HashMap<String, Value>) -> Result<(), CommError>;

    /// Acyclic request: on-demand read/write.
    fn acyclic_request(&mut self, request: AcyclicRequest) -> Result<AcyclicResponse, CommError>;

    fn is_connected(&self) -> bool;
    fn diagnostics(&self) -> DeviceDiagnostics;
}
```

The Communication Manager creates links from the `links:` section and devices from the
`devices:` section, binding each device to its declared link. Multiple devices sharing
a link use coordinated access (mutex/queue) to avoid bus collisions.

### Configuration in plc-project.yaml

The YAML is the **single source of truth** between hardware configuration and software
symbol mapping. Each communication entry defines a **named instance** of a device profile.
The `name` field becomes the global variable name in ST — giving a clear, unambiguous
correlation between physical hardware and code.

The YAML separates **links** (physical/transport layer) from **devices** (application
layer), following OSI layering principles. A link defines the shared transport — a
serial bus or a TCP endpoint. Devices are the addressable units on that link.

```yaml
name: BottleFillingLine
target: host

# ─── Links: physical/transport layer ─────────────────────────
# Each link is a communication channel with its own physical settings.
# Multiple devices can share a single link (same bus/connection).
links:
  # Ethernet link — one TCP endpoint per remote host
  - name: eth_rack_left
    type: tcp
    host: 192.168.1.100
    port: 502
    timeout: 500ms

  - name: eth_rack_right
    type: tcp
    host: 192.168.1.101
    port: 502
    timeout: 500ms

  # RS-485 serial bus — one port, shared by all slaves on the wire
  - name: rs485_bus_1
    type: serial
    port: /dev/ttyUSB0
    baud: 19200
    parity: even
    data_bits: 8
    stop_bits: 1
    timeout: 200ms

  # Second serial bus (different physical settings = different wire)
  - name: rs485_bus_2
    type: serial
    port: /dev/ttyUSB1
    baud: 9600
    parity: none
    data_bits: 8
    stop_bits: 2
    timeout: 500ms

  # TCP link for acyclic parameter access
  - name: eth_neighbor
    type: tcp
    host: 192.168.1.200
    port: 502

# ─── Devices: application/protocol layer ─────────────────────
# Each device is an addressable unit on a link. The `name` becomes
# the global struct instance name in ST code.
devices:
  # Two identical I/O racks on separate TCP links
  - name: rack_left              # ← VAR_GLOBAL rack_left : Wago750352;
    link: eth_rack_left
    protocol: modbus-tcp
    unit_id: 1
    mode: cyclic
    cycle_time: 10ms
    device_profile: wago_750_352

  - name: rack_right             # ← VAR_GLOBAL rack_right : Wago750352;
    link: eth_rack_right
    protocol: modbus-tcp
    unit_id: 1
    mode: cyclic
    cycle_time: 10ms
    device_profile: wago_750_352

  # Two VFDs on the SAME RS-485 bus — different slave addresses
  - name: pump_vfd               # ← VAR_GLOBAL pump_vfd : AbbAcs580;
    link: rs485_bus_1
    protocol: modbus-rtu
    unit_id: 3
    mode: cyclic
    device_profile: abb_acs580

  - name: fan_vfd                # ← VAR_GLOBAL fan_vfd : AbbAcs580;
    link: rs485_bus_1             # same bus! different address
    protocol: modbus-rtu
    unit_id: 4
    mode: cyclic
    device_profile: abb_acs580

  # Temperature sensor on a different serial bus (9600 baud)
  - name: temp_sensor
    link: rs485_bus_2
    protocol: modbus-rtu
    unit_id: 1
    mode: cyclic
    cycle_time: 100ms
    device_profile: generic_temp_rtd

  # Acyclic-only connection for on-demand parameter reads
  - name: plc_neighbor
    link: eth_neighbor
    protocol: modbus-tcp
    unit_id: 1
    mode: acyclic
```

This auto-generates struct types from device profiles and named global instances:

```st
(* Auto-generated from device profiles — DO NOT EDIT *)

(* Struct type generated from profile: wago_750_352 *)
TYPE Wago750352 : STRUCT
    DI_0 : BOOL;  DI_1 : BOOL;  DI_2 : BOOL;  DI_3 : BOOL;
    DI_4 : BOOL;  DI_5 : BOOL;  DI_6 : BOOL;  DI_7 : BOOL;
    AI_0 : INT;   AI_1 : INT;   AI_2 : INT;   AI_3 : INT;
    DO_0 : BOOL;  DO_1 : BOOL;  DO_2 : BOOL;  DO_3 : BOOL;
    AO_0 : INT;   AO_1 : INT;
END_STRUCT;

(* Struct type generated from profile: abb_acs580 *)
TYPE AbbAcs580 : STRUCT
    RUN        : BOOL;     (* control word bit 0, output *)
    STOP       : BOOL;     (* control word bit 1, output *)
    FAULT_RST  : BOOL;     (* control word bit 7, output *)
    READY      : BOOL;     (* status word bit 0, input *)
    RUNNING    : BOOL;     (* status word bit 1, input *)
    FAULT      : BOOL;     (* status word bit 3, input *)
    SPEED_REF  : REAL;     (* holding register 1, 0.1 Hz, output *)
    SPEED_ACT  : REAL;     (* input register 2, 0.1 Hz, input *)
    CURRENT    : REAL;     (* input register 3, 0.1 A, input *)
    TORQUE     : REAL;     (* input register 4, 0.1 Nm, input *)
    POWER      : REAL;     (* input register 5, 0.1 kW, input *)
END_STRUCT;
END_TYPE

(* Global instances — names from plc-project.yaml *)
VAR_GLOBAL
    rack_left  : Wago750352;   (* 192.168.1.100, unit 1 *)
    rack_right : Wago750352;   (* 192.168.1.101, unit 1 *)
    pump_vfd   : AbbAcs580;    (* /dev/ttyUSB0, unit 3 *)
END_VAR
```

User code is clear, portable, and hardware-agnostic:

```st
PROGRAM Main
VAR
    motor_on : BOOL;
END_VAR
    (* Unambiguous: which rack, which channel *)
    IF rack_left.DI_0 AND NOT rack_left.DI_7 THEN
        rack_right.DO_3 := TRUE;
    END_IF;

    (* VFD control — readable field names from the profile *)
    pump_vfd.RUN := motor_on;
    pump_vfd.SPEED_REF := 45.0;

    IF pump_vfd.FAULT THEN
        pump_vfd.FAULT_RST := TRUE;
    END_IF;

    (* Swap hardware? Change YAML, code stays the same. *)
END_PROGRAM
```

**Key benefits of the struct-based approach:**
- **No name collisions** — two identical cards don't fight over `DI_0`
- **Self-documenting** — `rack_left.DI_3` is unambiguous in code
- **Portability** — change `device_profile` or connection params in YAML, code unchanged
- **Reusable profiles** — define `wago_750_352.yaml` once, share across projects
- **Type safety** — the compiler knows which fields exist on each device
- **YAML as single source of truth** — hardware config and symbol mapping in one place

### Device Profile System

A device profile is a reusable YAML file that defines **both** the struct schema (fields
visible in ST code) **and** the register map (how the communication runtime reads/writes
the physical device). Profiles can be shared between projects and published as a community
library.

Each profile defines:
1. **Struct type name** — becomes the TYPE name in generated ST code
2. **Fields** — each field has a name, ST data type, direction, and register mapping
3. **Register mapping** — Modbus register address, type, bit position, scaling

```yaml
# profiles/abb_acs580.yaml
name: AbbAcs580
vendor: ABB
protocol: modbus-rtu
description: "Standard Modbus register map for ABB ACS580 series VFDs"

fields:
  # Control outputs (ST writes → Modbus writes)
  - name: RUN
    type: BOOL
    direction: output
    register: { address: 0, bit: 0, kind: coil }

  - name: STOP
    type: BOOL
    direction: output
    register: { address: 0, bit: 1, kind: coil }

  - name: FAULT_RST
    type: BOOL
    direction: output
    register: { address: 0, bit: 7, kind: coil }

  # Status inputs (Modbus reads → ST reads)
  - name: READY
    type: BOOL
    direction: input
    register: { address: 0, bit: 0, kind: discrete_input }

  - name: RUNNING
    type: BOOL
    direction: input
    register: { address: 0, bit: 1, kind: discrete_input }

  - name: FAULT
    type: BOOL
    direction: input
    register: { address: 0, bit: 3, kind: discrete_input }

  # Analog I/O (scaled values)
  - name: SPEED_REF
    type: REAL
    direction: output
    register: { address: 1, kind: holding_register, scale: 0.1, unit: Hz }

  - name: SPEED_ACT
    type: REAL
    direction: input
    register: { address: 2, kind: input_register, scale: 0.1, unit: Hz }

  - name: CURRENT
    type: REAL
    direction: input
    register: { address: 3, kind: input_register, scale: 0.1, unit: A }

  - name: TORQUE
    type: REAL
    direction: input
    register: { address: 4, kind: input_register, scale: 0.1, unit: Nm }

  - name: POWER
    type: REAL
    direction: input
    register: { address: 5, kind: input_register, scale: 0.1, unit: kW }
```

A generic I/O module profile shows the pattern for digital/analog boards:

```yaml
# profiles/wago_750_352.yaml
name: Wago750352
vendor: WAGO
protocol: modbus-tcp
description: "WAGO 750-352 fieldbus coupler with 8 DI, 4 AI, 4 DO, 2 AO"

fields:
  - { name: DI_0, type: BOOL, direction: input,  register: { address: 0, bit: 0, kind: coil } }
  - { name: DI_1, type: BOOL, direction: input,  register: { address: 0, bit: 1, kind: coil } }
  - { name: DI_2, type: BOOL, direction: input,  register: { address: 0, bit: 2, kind: coil } }
  - { name: DI_3, type: BOOL, direction: input,  register: { address: 0, bit: 3, kind: coil } }
  - { name: DI_4, type: BOOL, direction: input,  register: { address: 0, bit: 4, kind: coil } }
  - { name: DI_5, type: BOOL, direction: input,  register: { address: 0, bit: 5, kind: coil } }
  - { name: DI_6, type: BOOL, direction: input,  register: { address: 0, bit: 6, kind: coil } }
  - { name: DI_7, type: BOOL, direction: input,  register: { address: 0, bit: 7, kind: coil } }
  - { name: AI_0, type: INT,  direction: input,  register: { address: 0, kind: input_register } }
  - { name: AI_1, type: INT,  direction: input,  register: { address: 1, kind: input_register } }
  - { name: AI_2, type: INT,  direction: input,  register: { address: 2, kind: input_register } }
  - { name: AI_3, type: INT,  direction: input,  register: { address: 3, kind: input_register } }
  - { name: DO_0, type: BOOL, direction: output, register: { address: 0, bit: 0, kind: coil } }
  - { name: DO_1, type: BOOL, direction: output, register: { address: 0, bit: 1, kind: coil } }
  - { name: DO_2, type: BOOL, direction: output, register: { address: 0, bit: 2, kind: coil } }
  - { name: DO_3, type: BOOL, direction: output, register: { address: 0, bit: 3, kind: coil } }
  - { name: AO_0, type: INT,  direction: output, register: { address: 0, kind: holding_register } }
  - { name: AO_1, type: INT,  direction: output, register: { address: 1, kind: holding_register } }
```

### Extension Crate Structure

The crate layout mirrors the layer separation. Link implementations and device/protocol
implementations are separate. Device profiles are protocol-agnostic YAML.

```
st-comm-api/                    # Shared traits + types (lightweight, no I/O)
├── Cargo.toml
└── src/
    ├── lib.rs                  # CommLink + CommDevice traits
    ├── types.rs                # Value, CommError, LinkDiagnostics, etc.
    └── profile.rs              # DeviceProfile schema + YAML parser

st-comm-link-tcp/               # Link: TCP socket implementation
├── Cargo.toml                  # depends on st-comm-api
├── src/
│   └── lib.rs                  # implements CommLink for TCP
└── tests/

st-comm-link-serial/            # Link: serial port (RS-485/RS-232)
├── Cargo.toml
├── src/
│   └── lib.rs                  # implements CommLink for serial
└── tests/

st-comm-modbus/                 # Device: Modbus protocol (TCP + RTU framing)
├── Cargo.toml                  # depends on st-comm-api (NOT on link crates)
├── src/
│   ├── lib.rs                  # implements CommDevice for Modbus
│   ├── tcp_framing.rs          # MBAP header framing (for TCP links)
│   ├── rtu_framing.rs          # RTU framing + CRC-16 (for serial links)
│   ├── ascii_framing.rs        # ASCII framing + LRC (for serial links)
│   └── registers.rs            # Coil/register read/write logic
└── tests/

profiles/                       # Device profiles (shared across protocols)
├── wago_750_352.yaml           # WAGO I/O coupler
├── abb_acs580.yaml             # ABB VFD
├── siemens_g120.yaml           # Siemens VFD
├── danfoss_fc302.yaml          # Danfoss VFD
├── generic_io_16di.yaml        # Generic 16-ch digital input
├── generic_temp_rtd.yaml       # Generic RTD temperature sensor
└── README.md                   # How to create a device profile
```

**Why this structure?** A Modbus device doesn't care whether it's on TCP or serial —
the protocol framing changes, but the register map is the same. The `st-comm-modbus`
crate detects the link type and selects the appropriate framing (MBAP for TCP, RTU/ASCII
for serial). Adding a new transport (e.g., UDP, Bluetooth serial) only requires a new
link crate — all existing device crates work unchanged.

### Scan Cycle Integration

```
┌────────────────────────────────────────────────────┐
│              Engine Scan Cycle                       │
│                                                     │
│  1. comm_manager.read_inputs()                      │
│     → For each cyclic device:                       │
│       → Read Modbus registers from physical device  │
│       → Map register values → struct fields         │
│       → Write struct fields into VM globals          │
│       (e.g., rack_left.DI_0, pump_vfd.SPEED_ACT)    │
│                                                     │
│  2. vm.scan_cycle("Main")                           │
│     → Execute user's ST program                     │
│     → Program reads rack_left.DI_0, writes          │
│       pump_vfd.SPEED_REF, etc.                      │
│                                                     │
│  3. comm_manager.write_outputs()                    │
│     → For each cyclic device:                       │
│       → Read struct fields from VM globals           │
│       → Map struct fields → register values         │
│       → Write Modbus registers to physical device   │
│       (only output-direction fields are written)     │
│                                                     │
│  4. comm_manager.process_acyclic()                  │
│     → Handle queued on-demand requests              │
└────────────────────────────────────────────────────┘
```

### Implementation Plan

- [ ] **`st-comm-api` crate** (shared traits + types):
  - [ ] `CommLink` trait (open, close, send, receive, diagnostics)
  - [ ] `CommDevice` trait (configure, bind_link, read_inputs, write_outputs, acyclic)
  - [ ] `DeviceProfile` struct (name, vendor, fields with register mappings)
  - [ ] `ProfileField` struct (name, ST type, direction, register address/kind/bit/scale)
  - [ ] `CommError`, `LinkDiagnostics`, `DeviceDiagnostics` types
  - [ ] `AcyclicRequest`/`AcyclicResponse` types
  - [ ] Device profile YAML parser (profile → struct schema + register map)
  - [ ] Profile-to-ST code generator (profile → TYPE struct + VAR_GLOBAL instances)
  - [ ] Project YAML parser (links + devices sections)
- [ ] **`st-comm-link-tcp` crate** (TCP link):
  - [ ] TCP socket management (connect, reconnect, timeout)
  - [ ] Implements `CommLink` trait
  - [ ] Unit tests with mock TCP listener
- [ ] **`st-comm-link-serial` crate** (serial link):
  - [ ] Serial port management (RS-485/RS-232, baud, parity, data bits, stop bits)
  - [ ] Implements `CommLink` trait
  - [ ] Unit tests with mock serial port / PTY pair
- [ ] **`st-comm-modbus` crate** (Modbus protocol — works over any link):
  - [ ] Implements `CommDevice` trait for Modbus
  - [ ] TCP framing: MBAP header (auto-selected when link is TCP)
  - [ ] RTU framing: CRC-16, silence detection (auto-selected when link is serial)
  - [ ] ASCII framing: LRC (optional, for serial links)
  - [ ] Read coils, discrete inputs, holding registers, input registers
  - [ ] Write single/multiple coils, single/multiple registers
  - [ ] Cyclic polling with configurable interval
  - [ ] Device profile field ↔ register mapping with scaling
  - [ ] Unit tests with mock link
  - [ ] Integration tests with Modbus simulator
- [ ] **Communication Manager** (in `st-runtime`):
  - [ ] Parse `links:` and `devices:` sections from plc-project.yaml
  - [ ] Create link instances, bind devices to their declared links
  - [ ] Coordinate bus access for shared links (mutex/queue for serial buses)
  - [ ] Integrate into scan cycle: read_inputs → execute → write_outputs → acyclic
  - [ ] Map device profile struct fields ↔ VM global struct instance slots
  - [ ] Direction-aware I/O: only read `input` fields, only write `output` fields
  - [ ] Register value scaling (raw register ↔ engineering units via `scale` factor)
  - [ ] Connection monitoring and automatic reconnection
  - [ ] Diagnostics exposed via monitor server
- [ ] **Engine integration**:
  - [ ] `st-cli run` loads link/device config and starts communication
  - [ ] `st-cli comm-status` shows link health and device connection state
  - [ ] `st-cli comm-test` sends a test read to verify connectivity
- [ ] **Bundled device profiles**:
  - [ ] Generic Modbus I/O (coils + registers, 8/16/32 channel variants)
  - [ ] ABB ACS580 VFD
  - [ ] Siemens G120 VFD
  - [ ] WAGO 750-352 I/O coupler
  - [ ] Generic temperature sensor (RTD/thermocouple via analog input)
- [ ] **Documentation**:
  - [ ] Communication architecture guide (link/device layering explained)
  - [ ] "Creating a Link Extension" tutorial
  - [ ] "Creating a Device Extension" tutorial
  - [ ] "Creating a Device Profile" guide
  - [ ] Modbus quickstart (TCP + RTU examples)
  - [ ] Playground example: Modbus I/O with simulated devices
- [ ] **Future extensions** (separate crates, independent development):
  - [ ] `st-comm-link-udp` — UDP link (for protocols using UDP transport)
  - [ ] `st-comm-profinet` — PROFINET I/O device extension
  - [ ] `st-comm-ethercat` — EtherCAT device extension
  - [ ] `st-comm-canopen` — CANopen / CAN bus device extension
  - [ ] `st-comm-opcua` — OPC UA client device extension
  - [ ] `st-comm-mqtt` — MQTT publish/subscribe device extension
  - [ ] `st-comm-s7` — Siemens S7 protocol device extension
  - [ ] `st-comm-ethernet-ip` — EtherNet/IP (Allen-Bradley) device extension

---

## Phase 14 (Future): Native Compilation & Hardware Target Platform System

Two major capabilities: (1) LLVM native compilation backend, and (2) a plugin-based platform system
that lets each hardware target define its peripherals, I/O mapping, and compilation settings as a
self-contained extension — no framework recompilation required.

### 13a: LLVM Native Compilation Backend

- [ ] Integrate `inkwell` (Rust LLVM bindings)
- [ ] IR → LLVM IR lowering for all 50+ bytecode instructions
- [ ] JIT compilation for development mode (fast iteration on host)
- [ ] AOT cross-compilation for embedded targets (ARM Cortex-M, RISC-V, Xtensa)
- [ ] Adapt online change for native code (requires careful relocation strategy)
- [ ] Benchmark: VM interpreter vs LLVM-compiled cycle times

### 13b: Hardware Target Platform System

The platform system allows each hardware target (ESP32, STM32, Raspberry Pi, etc.) to be defined
as a **platform extension** — a self-contained package that provides:
1. **Compilation target**: LLVM triple, linker scripts, startup code
2. **Peripheral definitions**: typed ST variables/FBs that map to hardware registers
3. **Configuration schema**: user-configurable pin assignments, clock settings, peripheral modes
4. **Runtime HAL**: hardware abstraction layer bridging ST I/O to physical pins

A platform extension is loaded at compile time — the user selects a target in `plc-project.yaml`
and the platform's peripheral definitions become available as typed variables in their ST code.
No recompilation of the rust-plc framework is needed to add new platforms.

#### Architecture

```
plc-project.yaml
  target: esp32-wroom-32
  peripherals:
    gpio:
      pin_2: { mode: output, alias: LED }
      pin_4: { mode: input, pull: up, alias: BUTTON }
    uart:
      uart0: { baud: 115200, tx: 1, rx: 3 }
    adc:
      adc1_ch0: { pin: 36, attenuation: 11db, alias: TEMP_SENSOR }

↓ Platform extension generates:

VAR_GLOBAL
    LED           : BOOL;        (* GPIO2 output — mapped by platform *)
    BUTTON        : BOOL;        (* GPIO4 input — mapped by platform *)
    TEMP_SENSOR   : INT;         (* ADC1_CH0 — mapped by platform *)
    UART0_TX_DATA : STRING[256]; (* UART0 transmit buffer *)
END_VAR
```

The user's ST program reads/writes these variables like any other global.
The platform runtime maps them to hardware registers in the scan cycle.

#### Platform Extension Structure

```
platforms/
├── esp32/
│   ├── platform.yaml          # Platform metadata + LLVM triple
│   ├── peripherals/
│   │   ├── gpio.yaml          # GPIO pin definitions, modes, pull-up/down
│   │   ├── uart.yaml          # UART channels, baud rates, pin mappings
│   │   ├── spi.yaml           # SPI bus definitions
│   │   ├── i2c.yaml           # I2C bus definitions
│   │   ├── adc.yaml           # ADC channels, resolution, attenuation
│   │   ├── dac.yaml           # DAC channels
│   │   ├── pwm.yaml           # PWM/LEDC channels
│   │   └── timer.yaml         # Hardware timer definitions
│   ├── stdlib/                # Platform-specific ST function blocks
│   │   ├── esp_wifi.st        # WiFi connection FB
│   │   ├── esp_ble.st         # BLE communication FB
│   │   └── esp_sleep.st       # Deep sleep control
│   ├── hal/                   # Rust HAL implementation
│   │   └── lib.rs             # Maps ST globals ↔ hardware registers
│   ├── linker.ld              # Linker script for the target
│   └── startup.s              # Startup / vector table
├── stm32f103/
│   ├── platform.yaml
│   ├── peripherals/
│   │   ├── gpio.yaml          # PA0-PA15, PB0-PB15, PC13, etc.
│   │   ├── uart.yaml          # USART1, USART2, USART3
│   │   ├── spi.yaml           # SPI1, SPI2
│   │   ├── i2c.yaml           # I2C1, I2C2
│   │   ├── adc.yaml           # ADC1 (10 channels)
│   │   ├── pwm.yaml           # TIM1-TIM4 PWM channels
│   │   └── can.yaml           # CAN bus
│   ├── stdlib/
│   │   └── stm32_flash.st     # Flash read/write FB
│   ├── hal/
│   │   └── lib.rs
│   └── linker.ld
├── raspberry-pi/
│   ├── platform.yaml
│   ├── peripherals/
│   │   ├── gpio.yaml          # BCM GPIO 0-27
│   │   ├── uart.yaml          # /dev/ttyAMA0, /dev/ttyS0
│   │   ├── spi.yaml           # SPI0, SPI1
│   │   ├── i2c.yaml           # I2C1
│   │   └── pwm.yaml           # Hardware PWM channels
│   ├── stdlib/
│   │   └── rpi_camera.st      # Camera interface FB
│   └── hal/
│       └── lib.rs             # Uses rppal or embedded-hal
├── raspberry-pico/
│   ├── platform.yaml          # RP2040 / RP2350
│   ├── peripherals/
│   │   ├── gpio.yaml          # GP0-GP29
│   │   ├── uart.yaml          # UART0, UART1
│   │   ├── spi.yaml           # SPI0, SPI1
│   │   ├── i2c.yaml           # I2C0, I2C1
│   │   ├── adc.yaml           # ADC0-ADC3 + temp sensor
│   │   ├── pwm.yaml           # 16 PWM channels
│   │   └── pio.yaml           # Programmable I/O state machines
│   └── hal/
│       └── lib.rs             # Uses embassy-rp or rp-hal
└── risc-v/                    # Generic RISC-V target
    ├── platform.yaml
    └── hal/
        └── lib.rs
```

#### platform.yaml Schema

```yaml
name: ESP32-WROOM-32
vendor: Espressif
arch: xtensa
llvm_target: xtensa-esp32-none-elf
flash_size: 4MB
ram_size: 520KB
clock_speed: 240MHz

# Rust HAL crate to use for the runtime
hal_crate: esp-hal
hal_version: "0.22"

# Supported peripherals (references files in peripherals/)
peripherals:
  - gpio
  - uart
  - spi
  - i2c
  - adc
  - dac
  - pwm
  - timer

# Build settings
build:
  toolchain: esp       # rustup toolchain
  runner: espflash      # flash tool
  flash_command: "espflash flash --monitor"
```

#### User Configuration in plc-project.yaml

```yaml
name: MyIoTProject
target: esp32

peripherals:
  gpio:
    pin_2:  { mode: output, alias: STATUS_LED }
    pin_4:  { mode: input, pull: up, alias: START_BUTTON }
    pin_5:  { mode: output, alias: MOTOR_EN }
    pin_18: { mode: alternate, function: spi_clk }
    pin_19: { mode: alternate, function: spi_miso }
    pin_23: { mode: alternate, function: spi_mosi }
  uart:
    uart0: { baud: 115200, tx: 1, rx: 3, alias: DEBUG }
    uart2: { baud: 9600, tx: 17, rx: 16, alias: MODBUS }
  adc:
    adc1_ch0: { pin: 36, attenuation: 11db, alias: TEMP_SENSOR }
    adc1_ch3: { pin: 39, attenuation: 11db, alias: PRESSURE }
  spi:
    spi2: { clk: 18, miso: 19, mosi: 23, cs: 15, speed: 1000000, alias: DISPLAY }
```

This generates auto-included ST globals:
```st
(* Auto-generated from platform config — DO NOT EDIT *)
VAR_GLOBAL
    STATUS_LED    : BOOL;    (* GPIO2 output *)
    START_BUTTON  : BOOL;    (* GPIO4 input, pull-up *)
    MOTOR_EN      : BOOL;    (* GPIO5 output *)
    TEMP_SENSOR   : INT;     (* ADC1_CH0, 12-bit, 0-3.3V *)
    PRESSURE      : INT;     (* ADC1_CH3, 12-bit, 0-3.3V *)
END_VAR
```

#### Implementation Plan

- [ ] **Platform registry**: discover and load platform extensions from `platforms/` directory
- [ ] **Peripheral YAML schema**: define the configuration grammar for GPIO, UART, SPI, I2C, ADC, DAC, PWM
- [ ] **Config-to-ST generator**: read user's `plc-project.yaml` peripheral config, generate `VAR_GLOBAL` declarations with hardware-mapped names
- [ ] **LLVM cross-compilation**:
  - [ ] Target triple selection from platform.yaml
  - [ ] Linker script and startup code integration
  - [ ] `st-cli build --target esp32` compiles to flashable binary
- [ ] **Platform HAL runtime**:
  - [ ] Scan cycle integration: read physical inputs → execute program → write physical outputs
  - [ ] Map ST global variable slots to hardware register addresses
  - [ ] Interrupt-safe I/O access
- [ ] **Platform-specific stdlib**: each platform can ship additional `.st` files (e.g., WiFi FBs, BLE FBs)
- [ ] **CLI integration**:
  - [ ] `st-cli build --target esp32` — cross-compile for target
  - [ ] `st-cli flash --target esp32` — compile and flash to device
  - [ ] `st-cli targets` — list available platform extensions
  - [ ] `st-cli target-info esp32` — show peripherals, pins, capabilities
- [ ] **Initial platform implementations**:
  - [ ] ESP32 (Xtensa, via esp-hal)
  - [ ] STM32F103 (ARM Cortex-M3, via stm32f1xx-hal)
  - [ ] Raspberry Pi (Linux/ARM64, via rppal)
  - [ ] Raspberry Pi Pico / RP2040 (ARM Cortex-M0+, via embassy-rp)
  - [ ] Generic RISC-V (via riscv-hal)
- [ ] **Tests**:
  - [ ] Platform discovery and loading
  - [ ] Peripheral config parsing and validation
  - [ ] Config-to-ST generation (verify correct VAR_GLOBAL output)
  - [ ] Cross-compilation smoke test (compile to ELF, verify target arch)
  - [ ] Platform-specific stdlib compilation
- [ ] **Documentation**:
  - [ ] "Creating a Platform Extension" guide
  - [ ] Per-platform quickstart (ESP32, STM32, RPi, Pico)
  - [ ] Peripheral configuration reference
  - [ ] Hardware I/O mapping tutorial

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