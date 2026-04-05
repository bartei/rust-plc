# Architecture Overview

rust-plc is an IEC 61131-3 Structured Text compiler and runtime written in Rust.
The project follows the **rust-analyzer model**: a Rust core that implements
parsing, analysis, compilation, and execution, paired with a thin TypeScript
extension that wires the Language Server Protocol into VSCode.

## High-Level Diagram

```
  +-----------+     +------------+     +--------------+     +---------+
  | st-grammar| --> | st-syntax  | --> | st-semantics | --> | st-ir   |
  | tree-sitter     | CST->AST   |     | scope + types|     | bytecode|
  | parser    |     | lower.rs   |     | diagnostics  |     | defns   |
  +-----------+     +------------+     +--------------+     +---------+
                                                                  |
  +-----------+     +------------+     +--------------+           |
  | st-cli    | --> | st-lsp     | --> | st-dap       |           v
  | commands  |     | lang server|     | debug adapter|     +-----------+
  |           |     | hover, diag|     | breakpoints  |     | st-compiler|
  +-----------+     +------------+     +--------------+     | AST -> IR |
                                                            +-----------+
                                                                  |
  +-----------+                                                   v
  | st-monitor| <-------------------------------------------+-----------+
  | WS live   |                                             | st-runtime|
  | dashboard |                                             | VM engine |
  +-----------+                                             +-----------+

  editors/vscode/   Thin TypeScript extension (launches st-cli serve)
```

## The 10 Crates

| Crate | Path | Purpose |
|---|---|---|
| **st-grammar** | `crates/st-grammar` | Wraps the tree-sitter generated parser for Structured Text. Exposes `language()` and 70+ node-kind constants (`kind::*`). Incremental and error-recovering. |
| **st-syntax** | `crates/st-syntax` | Typed AST definitions (`ast.rs`) and CST-to-AST lowering (`lower.rs`). Every AST node carries a `TextRange` for source-location mapping. Provides the one-call `parse()` convenience function. |
| **st-semantics** | `crates/st-semantics` | Two-pass semantic analyzer. Pass 1 registers top-level names in the global scope; Pass 2 analyzes bodies. Includes the hierarchical scope model (`scope.rs`), semantic type system (`types.rs` -- `Ty` enum with widening/coercion rules), and diagnostics. |
| **st-ir** | `crates/st-ir` | Intermediate representation: `Module`, `Function`, `Instruction` enum (37 variants), `Value` enum, `MemoryLayout`, `VarSlot`, and `SourceLocation`. Register-based design with `u16` register indices and `u32` label indices. Serializable with serde. |
| **st-compiler** | `crates/st-compiler` | Compiles a typed AST (`SourceFile`) into an IR `Module`. Two internal passes: register all POUs, then compile bodies. Emits register-based instructions with source-map entries for debugger integration. |
| **st-runtime** | `crates/st-runtime` | Bytecode VM (`vm.rs`) with fetch-decode-execute loop and scan-cycle engine (`engine.rs`). Provides `CycleStats`, watchdog timeout, configurable max call depth and instruction limits. |
| **st-lsp** | `crates/st-lsp` | Language Server Protocol implementation via `tower-lsp`. Per-document state with incremental re-parse on edits. Provides diagnostics, semantic tokens, completion, hover, and go-to-definition. |
| **st-dap** | `crates/st-dap` | Debug Adapter Protocol server for online debugging: breakpoints, stepping, variable inspection, force/unforce, online change. |
| **st-monitor** | `crates/st-monitor` | WebSocket-based live monitoring server. Streams variable values from the runtime to connected dashboards for real-time trend recording. |
| **st-cli** | `crates/st-cli` | CLI entry point. Commands: `serve` (start LSP on stdio), `check <file>` (parse + analyze, report diagnostics), `run <file> [-n N]` (compile and execute N scan cycles). |

## Data Flow: Source to Execution

The end-to-end pipeline for `st-cli run example.st`:

1. **Read source** -- `st-cli` reads the `.st` file into a `String`.
2. **Parse** -- `st_syntax::parse()` creates a tree-sitter `Parser`, parses
   the source into a concrete syntax tree, then calls `lower::lower()` to
   produce a typed `SourceFile` AST plus any `LowerError`s.
3. **Analyze** -- `st_semantics::analyze::analyze()` builds a `SymbolTable`,
   resolves types, checks type compatibility, and collects `Diagnostic`s.
   If any error-severity diagnostics exist, `st-cli` reports them and exits.
4. **Compile** -- `st_compiler::compile()` walks the AST and emits an
   `st_ir::Module` containing `Function`s (with instructions, label maps,
   memory layouts, and source maps) plus global variable storage.
5. **Execute** -- `st_runtime::Engine::new()` instantiates a `Vm` from the
   module. `engine.run()` enters the scan-cycle loop, calling the named
   `PROGRAM` once per cycle and tracking `CycleStats`.

## The VSCode Extension

The extension lives in `editors/vscode/` and is intentionally thin:

- Registers the `structured-text` language (`.st`, `.scl` files).
- Provides TextMate grammar for syntax highlighting (`syntaxes/structured-text.tmLanguage.json`).
- Launches `st-cli serve` as the language server subprocess.
- Configurable server path via `structured-text.serverPath`.

All intelligence (diagnostics, completions, semantic tokens) is implemented in
the Rust LSP crate, not in TypeScript. This keeps the extension simple and
allows the same analysis to power both the CLI and the editor.

## DAP Server (st-dap)

The Debug Adapter Protocol server enables interactive debugging of ST programs
in VSCode. It sits between the editor and the VM, translating DAP requests
(setBreakpoints, stepIn, continue, etc.) into VM control operations.

Key design decisions:

- **Scan-cycle-aware continue**: When the user presses Continue, execution does
  not stop at the end of the current scan cycle. Instead, it runs across
  multiple cycles until a breakpoint is hit. This matches PLC debugging
  expectations.
- **PROGRAM local retention**: The VM skips variable initialization on
  subsequent scan cycles (using `body_start_pc`) so that local variables retain
  their values, just like a real PLC.
- **Source mapping**: The compiler emits `SourceLocation` entries for every
  instruction, allowing the DAP server to map bytecode PCs back to source lines.

## Online Change Manager

The online change system allows hot-reloading a modified program into a running
engine without restarting. The pipeline is:

1. **`analyze_change(old_module, new_module)`** -- Compares two compiled modules
   and determines whether the change is compatible (same variable layout) or
   incompatible (structural changes requiring a full restart).
2. **`migrate_locals(old_vm, new_module)`** -- For compatible changes, copies
   local variable values from the old VM state into the new module's memory
   layout, preserving runtime state.
3. **`apply_online_change(engine, new_module)`** -- Performs an atomic swap of
   the running module, replacing bytecode while the engine is between scan
   cycles.

See [Online Change](./online-change.md) for full details.

## Monitor Server (st-monitor)

The monitor server exposes a WebSocket interface for live variable observation
and control. It runs alongside the scan-cycle engine and provides:

- **Real-time variable streaming** -- Connected clients receive variable values
  after each scan cycle.
- **Force/unforce variables** -- Override variable values from the dashboard,
  useful for testing and commissioning.
- **JSON-RPC protocol** -- All communication uses a simple JSON request/response
  format over WebSocket.
- **MonitorHandle API** -- A thread-safe handle that the engine uses to publish
  state and receive force commands without blocking the scan loop.

See [Monitor Server](./monitor-server.md) for the full protocol reference.

## Dependency Graph

```
st-cli
  |-- st-lsp
  |     |-- st-semantics
  |     |     |-- st-syntax
  |     |     |     |-- st-grammar
  |     |     |-- st-syntax (ast types)
  |     |-- st-grammar (incremental re-parse)
  |-- st-dap
  |     |-- st-runtime
  |     |-- st-compiler
  |     |-- st-ir
  |-- st-compiler
  |     |-- st-ir
  |     |-- st-syntax (ast types)
  |-- st-runtime
  |     |-- st-ir
  |-- st-monitor
  |     |-- st-runtime
  |     |-- st-ir
  |-- st-semantics
  |-- st-syntax
```

External dependencies are kept minimal: `tree-sitter` for parsing,
`tower-lsp` + `lsp-types` for the language server, `tokio` for async,
`serde` for IR serialization, and `thiserror`/`anyhow` for error handling.
