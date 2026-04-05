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
1. Parse the source file
2. Run semantic analysis — abort if errors
3. Compile to bytecode
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
  help              Show this help message
```
