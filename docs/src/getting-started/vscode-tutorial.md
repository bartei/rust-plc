# Editing, Running & Debugging in VSCode

This is a complete walkthrough of writing, running, and debugging an IEC 61131-3 Structured Text program in Visual Studio Code using the rust-plc toolchain.

---

## Prerequisites

Before starting, make sure you have:

- **rust-plc repository** cloned and built (`cargo build -p st-cli`)
- **VSCode extension** installed (see [VSCode Setup](./vscode-setup.md))
- Or simply use the **Devcontainer** — everything is pre-configured

> **Fastest way to start:** Open the repository in VSCode and click "Reopen in Container". After the container builds, everything is ready.

---

## Step 1: Create a New ST Program

Open VSCode with the `playground/` folder (or any folder with `.st` files).

Create a new file called `my_program.st`:

**File → New File → Save as `my_program.st`**

Paste this code:

```st
(*
 * My first ST program — a simple counter with threshold detection.
 *)

FUNCTION IsAboveThreshold : BOOL
VAR_INPUT
    value : INT;
    threshold : INT;
END_VAR
    IsAboveThreshold := value > threshold;
END_FUNCTION

PROGRAM Main
VAR
    counter   : INT := 0;
    limit     : INT := 50;
    exceeded  : BOOL := FALSE;
    message   : INT := 0;
END_VAR
    counter := counter + 1;

    exceeded := IsAboveThreshold(value := counter, threshold := limit);

    IF exceeded THEN
        message := 1;
    ELSE
        message := 0;
    END_IF;

    IF counter >= 100 THEN
        counter := 0;
    END_IF;
END_PROGRAM
```

### What you should see immediately

As soon as you save the file:

1. **Syntax highlighting** — Keywords (`PROGRAM`, `IF`, `THEN`, `END_IF`) appear in a distinct color. Types (`INT`, `BOOL`) are highlighted differently. Comments are dimmed. String and numeric literals have their own colors.

2. **No red squiggles** — If the code is correct, no error underlines appear. The Problems panel (View → Problems) should show no errors.

3. **Status bar** — The bottom-right of VSCode shows `Structured Text` as the language mode.

---

## Step 2: Explore Editor Features

### Hover for Type Information

Hold **Ctrl** (or **Cmd** on macOS) and hover over any variable or function name:

- Hover over `counter` → shows: `counter : INT` with `Var` kind
- Hover over `IsAboveThreshold` → shows the function signature: `FUNCTION(value: INT, threshold: INT) : BOOL`
- Hover over `exceeded` → shows: `exceeded : BOOL`

### Go to Definition

**Ctrl+Click** (or **Cmd+Click**) on any identifier to jump to its declaration:

- Click on `IsAboveThreshold` in the `exceeded :=` line → jumps to the `FUNCTION IsAboveThreshold` declaration at the top
- Click on `counter` in the `IF counter >= 100` line → jumps to the `VAR` block where `counter` is declared
- Click on `limit` → jumps to its declaration

### Code Completion

Start typing inside the program body. Completion suggestions appear automatically:

- Type `cou` → completion list shows `counter`, `count` (if any), and keywords starting with "COU"
- Type `IF` → completion offers the `IF...END_IF` snippet template
- After a struct variable, type `.` → field names appear (e.g., `myStruct.` shows `x`, `y`, `value`)

**Snippet completions** insert full control structures:

| Trigger | Expands to |
|---------|-----------|
| `IF` | `IF ${condition} THEN ... END_IF;` |
| `FOR` | `FOR ${i} := ${1} TO ${10} DO ... END_FOR;` |
| `WHILE` | `WHILE ${condition} DO ... END_WHILE;` |
| `CASE` | `CASE ${expression} OF ... END_CASE;` |
| `FUNCTION` | Full function template with VAR_INPUT |
| `FUNCTION_BLOCK` | Full FB template |
| `PROGRAM` | Full program template |

### Document Outline

Open the **Outline** panel (View → Open View → Outline):

```
▼ Main (PROGRAM)
    counter : Var : INT
    limit : Var : INT
    exceeded : Var : BOOL
    message : Var : INT
▼ IsAboveThreshold (FUNCTION : BOOL)
    value : VarInput : INT
    threshold : VarInput : INT
```

This shows all POUs and their variables in a navigable tree.

### Diagnostics (Error Detection)

Try introducing an error — change `counter := counter + 1;` to:

```st
counter := counter + TRUE;
```

Immediately you'll see:

- A **red squiggly underline** under `TRUE`
- The Problems panel shows: `left operand of '+' must be numeric, found 'BOOL'`
- A red circle appears on the file tab and in the Explorer

Fix the error to clear the diagnostic.

**Common diagnostics the LSP catches:**

| Error | Example |
|-------|---------|
| Undeclared variable | `x := unknown_var;` |
| Type mismatch | `int_var := TRUE;` |
| Wrong condition type | `IF int_var THEN` (needs BOOL) |
| Missing parameters | `MyFunc()` when params are required |
| Unused variables | Variable declared but never read |
| EXIT outside loop | `EXIT;` in program body |
| Duplicate declarations | Two variables with the same name |

---

## Step 3: Run the Program

### From the Terminal

Open the integrated terminal (**Ctrl+`**) and run:

```bash
# Check for errors (no execution)
st-cli check my_program.st

# Run a single scan cycle
st-cli run my_program.st

# Run 1000 scan cycles (like a real PLC)
st-cli run my_program.st -n 1000
```

**Expected output for 1000 cycles:**
```
Executed 1000 cycle(s) in 1.2ms (avg 1.2µs/cycle, 28 instructions)
```

This tells you:
- **1000 cycles** were executed (like a PLC running for 1000 scans)
- **1.2µs per cycle** — the average execution time
- **28 instructions** — bytecode instructions per cycle

### Understanding Scan Cycles

In a real PLC, programs execute in a continuous loop called the **scan cycle**:

```
┌─────────────┐
│ Read Inputs │ ← from sensors, switches
├─────────────┤
│ Execute     │ ← your ST program runs here
│ Program     │
├─────────────┤
│ Write       │ ← to motors, valves, lights
│ Outputs     │
└─────┬───────┘
      │ repeat
      └───────→ back to top
```

The `-n 1000` flag simulates 1000 iterations of this loop. Global variables (`VAR_GLOBAL`) persist across cycles, so a counter increments each time.

---

## Step 4: Debug the Program

### Start a Debug Session

1. **Open** `my_program.st` in the editor
2. **Set a breakpoint** — click in the gutter (left margin) next to line `counter := counter + 1;`. A red dot appears.
3. **Press F5** or click **Run → Start Debugging**
4. If prompted, select **"Debug Current ST File"**

### What Happens

The debugger:
1. Compiles `my_program.st` to bytecode
2. Starts the VM paused on the first instruction
3. Shows the **Debug toolbar** at the top of the editor:

```
  ▶ Continue  ⏭ Step Over  ⏬ Step Into  ⏫ Step Out  🔄 Restart  ⏹ Stop
```

The editor highlights the current line (typically the first executable statement) with a yellow background.

### Debug Controls

| Button | Keyboard | Action |
|--------|----------|--------|
| ▶ Continue | `F5` | Run until next breakpoint or end |
| ⏭ Step Over | `F10` | Execute one statement, skip into function calls |
| ⏬ Step Into | `F11` | Execute one statement, enter function calls |
| ⏫ Step Out | `Shift+F11` | Run until current function returns |
| ⏹ Stop | `Shift+F5` | End debug session |

### Inspect Variables

While paused, look at the **Variables** panel on the left (Debug sidebar):

```
▼ Locals
    counter    0    INT
    limit      50   INT
    exceeded   FALSE BOOL
    message    0    INT
▼ Globals
    (empty — no VAR_GLOBAL in this program)
```

The values update as you step through the code.

### Step Through Code

1. Press **F10** (Step Over) — the highlighted line advances to the next statement
2. After stepping past `counter := counter + 1;`, check the Variables panel:
   - `counter` now shows `1`
3. Press **F10** again — steps to the `exceeded := IsAboveThreshold(...)` line
4. Press **F11** (Step Into) — enters the `IsAboveThreshold` function body
5. The **Call Stack** panel shows:

```
▼ PLC Scan Cycle
    IsAboveThreshold    line 10
    Main                line 24
```

6. Press **Shift+F11** (Step Out) — returns to `Main`
7. Press **F5** (Continue) — runs until the breakpoint is hit again (next scan cycle)

### Watch Expressions

In the **Watch** panel, click `+` and type a variable name:

- Type `counter` → shows the current value
- Type `exceeded` → shows `TRUE` or `FALSE`

The watch panel evaluates variable names against the current scope (locals first, then globals).

### Breakpoint Features

- **Toggle breakpoint**: Click the gutter or press **F9** on a line
- **Remove all breakpoints**: Run → Remove All Breakpoints
- **Conditional breakpoints** are not yet supported (future feature)

### Debug a Program with Global Variables

Create `counter_demo.st`:

```st
VAR_GLOBAL
    total_cycles : INT;
END_VAR

PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    total_cycles := total_cycles + 1;
    x := total_cycles * 2;
END_PROGRAM
```

Debug this file and use **Continue (F5)** multiple times. Watch `total_cycles` increment in the Globals scope each time the program completes a cycle and restarts.

---

## Step 5: Debug a Multi-POU Program

The debugger supports stepping into function calls across POUs.

Open `playground/06_full_demo.st` and set a breakpoint inside the `CASE state OF` block. Press F5 to start debugging:

1. The program stops on entry
2. Press **F5** to continue — it hits your breakpoint
3. Check the **Variables** panel to see all local variables and their current values
4. Step through the state machine logic
5. Use **Step Into (F11)** when a function like `Clamp(...)` is called to enter it

### Call Stack Navigation

When stopped inside a nested function call, the **Call Stack** panel shows all active frames:

```
▼ PLC Scan Cycle
    Clamp              line 32    ← current position
    BottleFiller       line 112   ← caller
```

Click on `BottleFiller` in the call stack to view the caller's local variables and source position.

---

## Troubleshooting

### "Failed to start ST language server"
- Build the CLI: `cargo build -p st-cli`
- Check the setting `structured-text.serverPath` points to the built binary

### Breakpoints appear as gray circles (unverified)
- The line may not correspond to any executable bytecode instruction
- Try setting the breakpoint on an assignment or function call line instead of a `VAR` declaration or `END_IF`

### No syntax highlighting
- Check the status bar shows "Structured Text" (not "Plain Text")
- If not, click the language mode and select "Structured Text"
- Reload the window: **Ctrl+Shift+P → "Developer: Reload Window"**

### Debug session ends immediately
- Ensure the file has a `PROGRAM` POU (not just functions/FBs)
- Check the terminal for compilation errors

### Variables show `<unknown>`
- The variable may be out of scope
- Step into the function where the variable is declared

---

## Quick Reference

| Action | How |
|--------|-----|
| Check file | `st-cli check file.st` |
| Run program | `st-cli run file.st -n 100` |
| Start debugging | Open `.st` file → F5 |
| Set breakpoint | Click gutter or F9 |
| Step over | F10 |
| Step into | F11 |
| Step out | Shift+F11 |
| Continue | F5 |
| Stop debugging | Shift+F5 |
| Hover for type | Ctrl+hover on identifier |
| Go to definition | Ctrl+click on identifier |
| Code completion | Start typing or Ctrl+Space |
| Document outline | View → Outline |
| Problems panel | View → Problems |
