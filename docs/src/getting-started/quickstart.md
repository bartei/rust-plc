# Quick Start

This guide walks you through writing and running your first Structured Text program.

## 1. Create a Program

Create a file called `hello.st`:

```st
PROGRAM HelloWorld
VAR
    counter : INT := 0;
    running : BOOL := TRUE;
END_VAR
    IF running THEN
        counter := counter + 1;
        IF counter > 100 THEN
            running := FALSE;
        END_IF;
    END_IF;
END_PROGRAM
```

## 2. Check for Errors

```bash
st-cli check hello.st
```

Output:
```
hello.st: OK
```

If there are errors, you'll see them with file, line, and column:
```
hello.st:5:10: error: undeclared variable 'x'
hello.st:8:8: error: condition must be BOOL, found 'INT'
```

## 3. Run the Program

```bash
# Run a single scan cycle
st-cli run hello.st

# Run 1000 scan cycles (like a real PLC)
st-cli run hello.st -n 1000
```

Output:
```
Executed 1000 cycle(s) in 1.74ms (avg 1.74µs/cycle, 16 instructions)
```

## 4. Write a Function

Functions compute and return a value:

```st
FUNCTION Clamp : REAL
VAR_INPUT
    value : REAL;
    low   : REAL;
    high  : REAL;
END_VAR
    IF value < low THEN
        Clamp := low;
    ELSIF value > high THEN
        Clamp := high;
    ELSE
        Clamp := value;
    END_IF;
END_FUNCTION

PROGRAM Main
VAR
    sensor : REAL := 150.0;
    output : REAL := 0.0;
END_VAR
    output := Clamp(value := sensor, low := 0.0, high := 100.0);
END_PROGRAM
```

## 5. Write a Function Block

Function blocks maintain state between calls:

```st
FUNCTION_BLOCK Counter
VAR_INPUT
    reset : BOOL;
END_VAR
VAR_OUTPUT
    count : INT;
END_VAR
VAR
    internal : INT := 0;
END_VAR
    IF reset THEN
        internal := 0;
    ELSE
        internal := internal + 1;
    END_IF;
    count := internal;
END_FUNCTION_BLOCK

PROGRAM Main
VAR
    cnt : Counter;
    value : INT := 0;
END_VAR
    cnt(reset := FALSE);
    value := cnt.count;
END_PROGRAM
```

## 6. Use Global Variables

Global variables persist across scan cycles:

```st
VAR_GLOBAL
    total_cycles : INT;
END_VAR

PROGRAM Main
VAR
    local_var : INT := 0;
END_VAR
    total_cycles := total_cycles + 1;
    local_var := total_cycles;
END_PROGRAM
```

```bash
# After 1000 cycles, total_cycles = 1000
st-cli run program.st -n 1000
```

## Next Steps

- [VSCode Setup](./vscode-setup.md) — Get editor support with diagnostics, hover, and completion
- [Language Reference](../language/program-structure.md) — Full language documentation
- [CLI Reference](../cli/commands.md) — All available commands
