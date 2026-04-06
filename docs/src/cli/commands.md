# CLI Commands

The `st-cli` tool provides commands for checking, compiling, running, and serving Structured Text programs.

## `st-cli check`

Parse and analyze a file, reporting all diagnostics (errors and warnings).

```bash
st-cli check <file>
```

**Example:**
```bash
$ st-cli check program.st
program.st: OK

$ st-cli check broken.st
broken.st:5:10: error: undeclared variable 'undefined_var'
broken.st:8:8: warning: unused variable 'temp'
```

**Exit codes:**
- `0` — No errors (warnings are OK)
- `1` — One or more errors found

**Diagnostic format:**
```
<file>:<line>:<column>: <severity>: <message>
```

## `st-cli run`

Compile and execute a Structured Text program.

```bash
st-cli run <file> [-n <cycles>]
```

**Options:**

| Flag | Default | Description |
|------|---------|-------------|
| `-n <cycles>` | `1` | Number of scan cycles to execute |

**Examples:**
```bash
# Run a single scan cycle
$ st-cli run program.st
Executed 1 cycle(s) in 8.5µs (avg 8.5µs/cycle, 16 instructions)

# Run 10,000 scan cycles (PLC-style continuous execution)
$ st-cli run program.st -n 10000
Executed 10000 cycle(s) in 17.4ms (avg 1.74µs/cycle, 16 instructions)
```

**Pipeline:**
1. Parse the source file (with stdlib merged via `builtin_stdlib()`)
2. Run semantic analysis — abort if errors
3. Compile to bytecode (intrinsics emitted as single instructions)
4. Execute in the VM for N cycles

**Global variables** persist across scan cycles, simulating PLC behavior:
```st
VAR_GLOBAL
    counter : INT;
END_VAR

PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    counter := counter + 1;  (* increments each cycle *)
END_PROGRAM
```

**Error handling:**
- Parse errors → reported and exits with code 1
- Semantic errors → reported and exits with code 1
- Runtime errors (division by zero, infinite loop) → reported and exits with code 1

## `st-cli serve`

Start the Language Server Protocol (LSP) server for editor integration.

```bash
st-cli serve
```

The server communicates over **stdin/stdout** using the JSON-RPC protocol. This is typically invoked by the VSCode extension, not directly by users.

**Supported LSP capabilities:**
- `textDocument/publishDiagnostics`
- `textDocument/hover`
- `textDocument/definition`
- `textDocument/completion` (with `.` trigger for struct fields)
- `textDocument/documentSymbol`
- `textDocument/semanticTokens/full`

## `st-cli debug`

Start a Debug Adapter Protocol (DAP) session for a Structured Text file.

```bash
st-cli debug <file>
```

The debug command compiles the source file and launches the DAP server over stdin/stdout. This is typically invoked by the VSCode extension when you press F5, not called directly by users.

**Example:**
```bash
$ st-cli debug my_program.st
```

**DAP capabilities supported:**

| Capability | Description |
|-----------|-------------|
| Breakpoints | Set/clear breakpoints on executable lines |
| Step In | Step into function/FB calls (`F11`) |
| Step Over | Step over one statement (`F10`) |
| Step Out | Run until current function returns (`Shift+F11`) |
| Continue | Run across scan cycles until a breakpoint is hit (`F5`, up to 100,000 cycles) |
| Stack Trace | View the full call stack including nested POU calls |
| Scopes | Inspect Locals and Globals scopes |
| Variables | View all variables with types and current values |
| Evaluate | Evaluate variable names in the current scope |

**PLC-specific extensions via evaluate:**

The debugger supports PLC-specific commands entered as evaluate expressions in the Debug Console:

| Expression | Description |
|-----------|-------------|
| `force x = 42` | Force variable `x` to value 42 (overrides program logic) |
| `force y = true` | Force a BOOL variable to TRUE |
| `unforce x` | Remove force override from variable `x` |
| `listForced` | List all currently forced variables and their values |
| `scanCycleInfo` | Show scan cycle statistics (count, timing) |

These commands are also available via **4 VSCode debug toolbar buttons** (Force, Unforce, List Forced, Cycle Info) when a debug session is active.

**Key behaviors:**

- **Continue runs across scan cycles.** When you press Continue (F5), execution proceeds through the remainder of the current scan cycle and into subsequent cycles (up to 100,000) until a breakpoint is hit. This matches the expected PLC debugging experience where the program loops continuously.
- **Step at end of cycle wraps.** When stepping reaches the end of a scan cycle, the VM wraps to the next cycle instead of terminating.
- **PROGRAM locals are retained across scan cycles.** The VM uses `body_start_pc` to skip variable initialization on subsequent cycles, preserving local variable state just like a real PLC.
- **FB instance state persisted.** Function block instance state is maintained across scan cycles via the `fb_instances` HashMap.
- **Variable names that conflict with ST keywords** (e.g., a variable named `dt` conflicting with the `DT` date-time keyword) are handled correctly by the debugger's evaluate command.

**Pipeline:**
1. Parse the source file (with stdlib merged)
2. Run semantic analysis -- abort if errors
3. Compile to bytecode with source map entries
4. Launch DAP server, awaiting `initialize` and `launch` requests
5. Execute in the VM under debugger control

## `st-cli help`

Show usage information.

```bash
$ st-cli help
st-cli: IEC 61131-3 Structured Text toolchain

Usage: st-cli <command> [options]

Commands:
  serve [--stdio]   Start the LSP server (default: stdio)
  check <file>      Parse and analyze a file, report diagnostics
  run <file> [-n N] Compile and execute a program (N cycles, default 1)
  debug <file>      Start a DAP debug session over stdio
  help              Show this help message
```
