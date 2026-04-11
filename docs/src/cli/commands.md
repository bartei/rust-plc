# CLI Commands

The `st-cli` tool provides commands for checking, compiling, running, formatting, and serving Structured Text programs.

## `st-cli check`

Parse and analyze source file(s), reporting all diagnostics.

```bash
st-cli check [path] [--json]
```

**Path modes:**

| Path | Behavior |
|------|----------|
| `st-cli check` | Autodiscover `.st` files from current directory |
| `st-cli check file.st` | Check a single file |
| `st-cli check dir/` | Autodiscover from directory |
| `st-cli check plc-project.yaml` | Use project file configuration |

**Flags:**

| Flag | Description |
|------|-------------|
| `--json` | Output diagnostics as structured JSON (for CI integration) |

**Examples:**
```bash
$ st-cli check program.st
program.st: OK

$ st-cli check broken.st
broken.st:5:10: error: undeclared variable 'x'
broken.st:8:8: warning: unused variable 'temp'

# Project mode
$ cd my_project/
$ st-cli check
Project 'MyProject': 4 source file(s)
  controllers/main.st
  types/data.st
  utils.st
  main.st
Project 'MyProject': OK

# JSON output for CI
$ st-cli check broken.st --json
[
  {
    "file": "broken.st",
    "line": 5,
    "column": 10,
    "severity": "error",
    "code": "UndeclaredVariable",
    "message": "undeclared variable 'x'"
  }
]
```

**Exit codes:**
- `0` — No errors (warnings are OK)
- `1` — One or more errors found

## `st-cli run`

Compile and execute a Structured Text program.

```bash
st-cli run [path] [-n <cycles>]
```

**Path modes:**

| Path | Behavior |
|------|----------|
| `st-cli run` | Autodiscover from current directory, run first PROGRAM found |
| `st-cli run file.st` | Compile and run a single file |
| `st-cli run dir/` | Autodiscover from directory |
| `st-cli run -n 1000` | Autodiscover + run 1000 scan cycles |

**Options:**

| Flag | Default | Description |
|------|---------|-------------|
| `-n <cycles>` | `1` | Number of scan cycles to execute |

**Examples:**
```bash
# Single file
$ st-cli run program.st
Executed 1 cycle(s) in 8.5µs (avg 8.5µs/cycle, 16 instructions)

# 10,000 scan cycles
$ st-cli run program.st -n 10000
Executed 10000 cycle(s) in 17.4ms (avg 1.74µs/cycle, 16 instructions)

# Project mode
$ cd my_project/
$ st-cli run -n 100
Project 'MyProject': 5 source file(s)
Executed 100 cycle(s) in 1.8ms (avg 18µs/cycle, 112 instructions)
```

**Pipeline:**
1. Discover source files (single file, directory, or project yaml)
2. Parse all sources with stdlib merged via `builtin_stdlib()`
3. Run semantic analysis — abort if errors
4. Compile to bytecode (intrinsics emitted as single instructions)
5. Execute in the VM for N cycles

**PLC behavior:**
- **PROGRAM locals persist** across scan cycles (like a real PLC)
- **Global variables persist** across scan cycles
- **FB instance state persists** across calls
- **Timers use wall-clock time** via `SYSTEM_TIME()`
- **Configurable scan cycle period** via `engine.cycle_time` in
  `plc-project.yaml` — see [Project Configuration](./project-configuration.md).
  When set, the engine sleeps after each cycle so the loop runs at the
  configured rate (10ms, 1ms, etc.) just like a real PLC. When omitted, runs
  as fast as the CPU allows.

## `st-cli compile`

Compile a Structured Text source file to a bytecode file.

```bash
st-cli compile <file> -o <output>
```

**Example:**
```bash
$ st-cli compile program.st -o program.json
Compiled to program.json (78047 bytes)
```

The output is a JSON-serialized `Module` containing all compiled functions, global variables, type definitions, and source maps. This can be used for offline analysis or loaded by external tools.

**Pipeline:**
1. Parse the source file with stdlib
2. Run semantic analysis — abort if errors
3. Compile to bytecode
4. Serialize module as JSON to the output file

## `st-cli fmt`

Format Structured Text source file(s) in place.

```bash
st-cli fmt [path]
```

**Path modes:**

| Path | Behavior |
|------|----------|
| `st-cli fmt` | Format all `.st` files in current directory (autodiscover) |
| `st-cli fmt file.st` | Format a single file |
| `st-cli fmt dir/` | Format all files in directory |

**Example:**
```bash
$ st-cli fmt program.st
Formatted: program.st
Formatted 1 file(s)

# Format entire project
$ cd my_project/
$ st-cli fmt
Formatted: controllers/main.st
Formatted: utils.st
Formatted 2 file(s)

# Already formatted
$ st-cli fmt program.st
All 1 file(s) already formatted
```

The formatter normalizes indentation (4 spaces per level) for all ST block structures: PROGRAM, FUNCTION, VAR, IF, FOR, WHILE, CASE, STRUCT, TYPE, etc.

## `st-cli serve`

Start the Language Server Protocol (LSP) server for editor integration.

```bash
st-cli serve
```

The server communicates over **stdin/stdout** using the JSON-RPC protocol. This is typically invoked by the VSCode extension, not directly by users.

**Supported LSP capabilities (23 features):**

| Feature | Protocol Method |
|---------|----------------|
| Diagnostics | `textDocument/publishDiagnostics` |
| Hover | `textDocument/hover` |
| Go-to-definition | `textDocument/definition` |
| Go-to-type-definition | `textDocument/typeDefinition` |
| Completion | `textDocument/completion` (triggers: `.`) |
| Signature help | `textDocument/signatureHelp` (triggers: `(`, `,`) |
| Find references | `textDocument/references` |
| Rename | `textDocument/rename` |
| Document symbols | `textDocument/documentSymbol` |
| Workspace symbols | `workspace/symbol` |
| Document highlight | `textDocument/documentHighlight` |
| Folding ranges | `textDocument/foldingRange` |
| Selection ranges | `textDocument/selectionRange` |
| Inlay hints | `textDocument/inlayHint` |
| Call hierarchy | `textDocument/prepareCallHierarchy` + incoming/outgoing |
| Document links | `textDocument/documentLink` |
| Semantic tokens | `textDocument/semanticTokens/full` |
| Formatting | `textDocument/formatting` |
| On-type formatting | `textDocument/onTypeFormatting` (triggers: `\n`, `;`) |
| Linked editing | `textDocument/linkedEditingRange` |
| Code actions | `textDocument/codeAction` |

**Selection ranges** (`Shift+Alt+Right/Left`): Smart expand / shrink selection.
Steps outward through AST nesting levels — word → expression → statement → IF/FOR
body → VAR block → PROGRAM/FUNCTION → entire file. Based on the parsed AST, not
regex heuristics.

**Inlay hints**: Parameter-name hints shown inline at function/FB call sites for
positional arguments (e.g., `Add(`**`a:`**` 10,` **`b:`**` 20)`). Hints are
suppressed when arguments already use named syntax (`a := 10`) or when the
argument text matches the parameter name (`Add(a, b)` where the params are also
`a` and `b`). Hover the hint to see the full `paramName: Type` signature.

**Call hierarchy** (`Shift+Alt+H` or right-click → "Show Call Hierarchy"):
Navigate the call graph of your program. For any FUNCTION, FUNCTION_BLOCK,
PROGRAM, or CLASS METHOD:
- **Incoming calls** — show every POU that calls this function, with the exact
  call-site ranges highlighted. Navigate up the call chain to trace who triggers
  a given routine.
- **Outgoing calls** — show every function this POU calls. Navigate down the
  call chain to understand dependencies.
The call hierarchy resolves across all open documents and supports the full
POU type spectrum (functions, FBs, programs, class methods). Particularly
useful for understanding data flow in multi-file PLC projects and for impact
analysis when modifying a shared helper function.

**Linked editing**: When the cursor is on a block keyword like `IF`, `FOR`,
`PROGRAM`, `VAR`, etc., VS Code highlights the matching closing keyword
(`END_IF`, `END_FOR`, `END_PROGRAM`, `END_VAR`). Editing either keyword
updates both simultaneously. Covers all 19 IEC 61131-3 keyword pairs. Uses
AST-based nesting resolution so nested `IF`/`END_IF` blocks pair correctly
instead of matching the wrong level.

**On-type formatting** (automatic): Triggers on **Enter** and **`;`**. After
pressing Enter, the new line is automatically indented to the correct level
based on the previous line's context — one level deeper after `THEN`, `DO`,
`VAR`, `PROGRAM`, `FUNCTION`, `ELSE`, `CASE ... OF`, etc.; same level after
normal statements; and no extra indent after `END_*` blocks. After typing
`;` on a line that starts with `END_IF`, `END_FOR`, etc., the line is
reindented to match its opening keyword's level (useful when you've been
typing at the wrong indent depth). Tab size is respected from VS Code's
editor settings.

## `st-cli debug`

Start a Debug Adapter Protocol (DAP) session for a Structured Text file.

```bash
st-cli debug <file>
```

This is typically invoked by the VSCode extension when you press F5, not called directly by users.

**DAP capabilities:**

| Capability | Description |
|-----------|-------------|
| Breakpoints | Set/clear breakpoints on executable lines |
| Step In | Step into function/FB calls (`F11`) |
| Step Over | Step over one statement (`F10`) |
| Step Out | Run until current function returns (`Shift+F11`) |
| Continue | Run scan cycles until a breakpoint is hit or user pauses (`F5`). The toolbar switches to a Pause button while running. |
| Stack Trace | View the full call stack including nested POU calls |
| Scopes | Inspect Locals and Globals scopes |
| Variables | View all variables with types and current values |
| Evaluate | Evaluate variable names or PLC commands |

**PLC-specific debug commands** (type in the Debug Console):

| Expression | Description |
|-----------|-------------|
| `force x = 42` | Force variable `x` to value 42 |
| `unforce x` | Remove force from variable `x` |
| `listForced` | List all forced variables |
| `scanCycleInfo` | Show cycle statistics |

**Key behaviors:**
- Continue runs across scan cycles indefinitely until the user pauses, sets a
  breakpoint, or disconnects — same as a real PLC. A 10-million-cycle safety
  cap guards against runaway loops in tests and CI.
- Step at end of cycle wraps to the next cycle
- PROGRAM locals and FB state persist across cycles
- The DAP run loop is interruptible: Pause, Disconnect, and SetBreakpoints
  take effect mid-run; new breakpoints become active on the next cycle.
- If `engine.cycle_time` is set in `plc-project.yaml`, the DAP loop honors it
  (sleeps in interruptible 10ms chunks between cycles)
- 4 VSCode debug toolbar buttons: Force, Unforce, List Forced, Cycle Info

## `st-cli bundle`

Create a `.st-bundle` archive for deployment to remote targets. See [Bundle Modes & IP Protection](../deployment/bundles.md) for details.

```bash
st-cli bundle [path] [--release | --release-debug] [-o <output>]
```

## `st-cli bundle inspect`

Show metadata and file listing of a bundle.

```bash
st-cli bundle inspect <bundle-path>
```

## `st-cli target list`

Show deployment targets configured in `plc-project.yaml`.

```bash
st-cli target list [path]
```

## `st-cli target install`

Install the PLC runtime on a remote Linux target. See [Target Management](../deployment/targets.md) for the full reference.

```bash
st-cli target install user@host [--key <path>] [--port <n>] [--upgrade]
```

## `st-cli target uninstall`

Remove the PLC runtime from a remote target.

```bash
st-cli target uninstall user@host [--purge]
```

> For detailed deployment command documentation, see [Deployment Commands](../deployment/commands.md).

## `st-cli help`

Show usage information.

```bash
$ st-cli help
st-cli: IEC 61131-3 Structured Text toolchain

Usage: st-cli <command> [options]

Commands:
  serve             Start the LSP server (stdio)
  check [path]      Parse and analyze, report diagnostics
  run [path] [-n N] Compile and execute (N cycles, default 1)
  compile <path> -o <output>  Compile to bytecode file
  fmt [path]        Format source file(s) in place
  bundle [path]     Create a .st-bundle for deployment
  target list       List configured deployment targets
  target install    Install PLC runtime on a target device
  target uninstall  Remove PLC runtime from a target
  comm-gen [path]   Regenerate _io_map.st from plc-project.yaml
  debug <file>      Start DAP debug server (stdin/stdout)
  help              Show this help message
```
