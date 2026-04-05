# Standard Library

The IEC 61131-3 standard library provides a set of reusable function blocks and functions commonly needed in PLC programming. The library is implemented as plain Structured Text source files in the `stdlib/` directory and is automatically loaded by the compiler -- all standard library functions and function blocks are available in every program without any import statements.

The library includes:

| Module | Source File | Contents |
|--------|------------|----------|
| [Counters](#counters) | `stdlib/counters.st` | CTU, CTD, CTUD |
| [Edge Detection](#edge-detection) | `stdlib/edge_detection.st` | R_TRIG, F_TRIG |
| [Timers](#timers) | `stdlib/timers.st` | TON, TOF, TP |
| [Math & Selection](#math--selection) | `stdlib/math.st` | MAX, MIN, LIMIT, ABS, SEL |
| [Conversions](#type-conversions) | `stdlib/conversions.st` | BOOL_TO_INT, INT_TO_BOOL |

---

## Counters

Source: `stdlib/counters.st`

Counters are function blocks that track rising edges on their count inputs. Because they are function blocks (not functions), each instance retains its internal state across scan cycles.

### CTU -- Count Up

Increments `CV` on each rising edge of `CU`. Sets `Q` to TRUE when `CV` reaches or exceeds the preset value `PV`. The `RESET` input sets `CV` back to 0.

**Inputs**

| Name | Type | Description |
|------|------|-------------|
| `CU` | BOOL | Count up -- increments on rising edge |
| `RESET` | BOOL | Reset counter to 0 |
| `PV` | INT | Preset value -- Q goes TRUE when CV >= PV |

**Outputs**

| Name | Type | Description |
|------|------|-------------|
| `Q` | BOOL | TRUE when CV >= PV |
| `CV` | INT | Current counter value |

**Example**

```st
PROGRAM CounterExample
VAR
    my_counter : CTU;
    pulse : BOOL;
END_VAR
    my_counter(CU := pulse, RESET := FALSE, PV := 10);

    // my_counter.Q is TRUE after 10 rising edges on pulse
    // my_counter.CV holds the current count
END_PROGRAM
```

### CTD -- Count Down

Decrements `CV` on each rising edge of `CD`. The `LOAD` input sets `CV` to `PV`. Sets `Q` to TRUE when `CV` drops to 0 or below.

**Inputs**

| Name | Type | Description |
|------|------|-------------|
| `CD` | BOOL | Count down -- decrements on rising edge |
| `LOAD` | BOOL | Load preset value into CV |
| `PV` | INT | Preset value |

**Outputs**

| Name | Type | Description |
|------|------|-------------|
| `Q` | BOOL | TRUE when CV <= 0 |
| `CV` | INT | Current counter value |

**Example**

```st
PROGRAM CountdownExample
VAR
    parts_left : CTD;
    part_sensor : BOOL;
END_VAR
    parts_left(CD := part_sensor, LOAD := FALSE, PV := 100);

    // parts_left.Q goes TRUE when all 100 parts consumed
END_PROGRAM
```

### CTUD -- Count Up/Down

Combines up-counting and down-counting in a single block. `RESET` sets `CV` to 0; `LOAD` sets `CV` to `PV`. When neither reset nor load is active, rising edges on `CU` increment and rising edges on `CD` decrement.

**Inputs**

| Name | Type | Description |
|------|------|-------------|
| `CU` | BOOL | Count up -- increments on rising edge |
| `CD` | BOOL | Count down -- decrements on rising edge |
| `RESET` | BOOL | Reset counter to 0 |
| `LOAD` | BOOL | Load preset value into CV |
| `PV` | INT | Preset value |

**Outputs**

| Name | Type | Description |
|------|------|-------------|
| `QU` | BOOL | TRUE when CV >= PV |
| `QD` | BOOL | TRUE when CV <= 0 |
| `CV` | INT | Current counter value |

**Example**

```st
PROGRAM UpDownExample
VAR
    inventory : CTUD;
    item_in : BOOL;
    item_out : BOOL;
END_VAR
    inventory(CU := item_in, CD := item_out,
              RESET := FALSE, LOAD := FALSE, PV := 50);

    // inventory.QU = TRUE when stock is full (>= 50)
    // inventory.QD = TRUE when stock is empty (<= 0)
END_PROGRAM
```

> **Note:** All counters detect rising edges internally. They only count on the FALSE-to-TRUE transition of the count input, not while it is held TRUE.

---

## Edge Detection

Source: `stdlib/edge_detection.st`

Edge detection function blocks produce a single-cycle pulse when a signal changes state. They are the building blocks used internally by counters and timers, and are useful on their own for detecting button presses, sensor transitions, and other discrete events.

### R_TRIG -- Rising Edge

`Q` is TRUE for exactly one scan cycle when `CLK` transitions from FALSE to TRUE.

**Inputs**

| Name | Type | Description |
|------|------|-------------|
| `CLK` | BOOL | Signal to monitor |

**Outputs**

| Name | Type | Description |
|------|------|-------------|
| `Q` | BOOL | TRUE for one cycle on rising edge |

### F_TRIG -- Falling Edge

`Q` is TRUE for exactly one scan cycle when `CLK` transitions from TRUE to FALSE.

**Inputs**

| Name | Type | Description |
|------|------|-------------|
| `CLK` | BOOL | Signal to monitor |

**Outputs**

| Name | Type | Description |
|------|------|-------------|
| `Q` | BOOL | TRUE for one cycle on falling edge |

**Example**

```st
PROGRAM EdgeExample
VAR
    start_btn_edge : R_TRIG;
    stop_btn_edge  : F_TRIG;
    start_button : BOOL;
    stop_button  : BOOL;
    motor_on     : BOOL;
END_VAR
    start_btn_edge(CLK := start_button);
    stop_btn_edge(CLK := stop_button);

    IF start_btn_edge.Q THEN
        motor_on := TRUE;   // Start on button press
    END_IF;
    IF stop_btn_edge.Q THEN
        motor_on := FALSE;  // Stop on button release
    END_IF;
END_PROGRAM
```

---

## Timers

Source: `stdlib/timers.st`

Timers count **scan cycles**, not wall-clock time. The preset value `PT` is the number of cycles to delay. To convert from real time, divide the desired duration by your scan cycle time:

```
PT := desired_ms / cycle_time_ms
```

> **Note:** The timer input is named `IN1` (not `IN`) to avoid a keyword conflict in Structured Text.

All three timer blocks share the same input/output signature:

**Inputs**

| Name | Type | Description |
|------|------|-------------|
| `IN1` | BOOL | Timer input |
| `PT` | INT | Preset time in scan cycles |

**Outputs**

| Name | Type | Description |
|------|------|-------------|
| `Q` | BOOL | Timer output |
| `ET` | INT | Elapsed time in scan cycles |

### TON -- On-Delay Timer

`Q` goes TRUE after `IN1` has been TRUE for `PT` consecutive scan cycles. When `IN1` goes FALSE, both `Q` and `ET` reset immediately.

```
IN1:  _____|‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾|_____
ET:   00000 1 2 3 4 5 5 5 5 5 00000
Q:    _____|          ‾‾‾‾‾‾‾|_____
              ^-- PT=5 reached
```

### TOF -- Off-Delay Timer

`Q` goes TRUE immediately when `IN1` goes TRUE. When `IN1` goes FALSE, `Q` stays TRUE for `PT` additional scan cycles before turning FALSE.

```
IN1:  _____|‾‾‾‾‾|________________________
Q:    _____|‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾|___________
ET:   00000 00000 1 2 3 4 5 5 5 00000
                          ^-- PT=5, Q goes FALSE
```

### TP -- Pulse Timer

On a rising edge of `IN1`, `Q` goes TRUE for exactly `PT` scan cycles, regardless of what `IN1` does during the pulse. A new pulse cannot be triggered while the current one is active.

```
IN1:  _____|‾‾‾‾‾‾‾‾‾‾‾‾‾|________
Q:    _____|‾‾‾‾‾‾‾‾‾|____________
ET:   00000 1 2 3 4 5 000000000000
                  ^-- PT=5, pulse ends
```

**Example**

```st
PROGRAM TimerExample
VAR
    debounce : TON;
    raw_input : BOOL;
    clean_input : BOOL;
END_VAR
    // Debounce: require input to be stable for 5 cycles
    debounce(IN1 := raw_input, PT := 5);
    clean_input := debounce.Q;
END_PROGRAM
```

---

## Math & Selection

Source: `stdlib/math.st`

Math functions are pure functions (not function blocks) -- they have no internal state and return a value directly.

### Integer Functions

| Function | Signature | Description |
|----------|-----------|-------------|
| `MAX_INT` | `MAX_INT(IN1: INT, IN2: INT) : INT` | Returns the larger of two values |
| `MIN_INT` | `MIN_INT(IN1: INT, IN2: INT) : INT` | Returns the smaller of two values |
| `ABS_INT` | `ABS_INT(IN1: INT) : INT` | Returns the absolute value |
| `LIMIT_INT` | `LIMIT_INT(MN: INT, IN1: INT, MX: INT) : INT` | Clamps IN1 to range [MN, MX] |

### REAL Functions

| Function | Signature | Description |
|----------|-----------|-------------|
| `MAX_REAL` | `MAX_REAL(IN1: REAL, IN2: REAL) : REAL` | Returns the larger of two values |
| `MIN_REAL` | `MIN_REAL(IN1: REAL, IN2: REAL) : REAL` | Returns the smaller of two values |
| `ABS_REAL` | `ABS_REAL(IN1: REAL) : REAL` | Returns the absolute value |
| `LIMIT_REAL` | `LIMIT_REAL(MN: REAL, IN1: REAL, MX: REAL) : REAL` | Clamps IN1 to range [MN, MX] |

### Selection

| Function | Signature | Description |
|----------|-----------|-------------|
| `SEL` | `SEL(G: BOOL, IN0: INT, IN1: INT) : INT` | Returns IN0 when G=FALSE, IN1 when G=TRUE |

**Example**

```st
PROGRAM MathExample
VAR
    sensor_val : INT := -42;
    clamped : INT;
    bigger : INT;
    mode : BOOL := TRUE;
    chosen : INT;
END_VAR
    clamped := LIMIT_INT(MN := 0, IN1 := sensor_val, MX := 100);
    // clamped = 0 (sensor_val is below MN)

    bigger := MAX_INT(IN1 := 10, IN2 := 20);
    // bigger = 20

    chosen := SEL(G := mode, IN0 := 50, IN1 := 75);
    // chosen = 75 (mode is TRUE, so IN1 selected)
END_PROGRAM
```

---

## Type Conversions

Source: `stdlib/conversions.st`

Explicit type conversion functions. The VM handles implicit widening coercions automatically; these functions are for explicit conversion and narrowing cases.

| Function | Signature | Description |
|----------|-----------|-------------|
| `BOOL_TO_INT` | `BOOL_TO_INT(IN1: BOOL) : INT` | Returns 1 if TRUE, 0 if FALSE |
| `INT_TO_BOOL` | `INT_TO_BOOL(IN1: INT) : BOOL` | Returns TRUE if nonzero, FALSE if 0 |

**Example**

```st
PROGRAM ConversionExample
VAR
    flag : BOOL := TRUE;
    flag_as_int : INT;
    value : INT := 42;
    value_as_bool : BOOL;
END_VAR
    flag_as_int := BOOL_TO_INT(IN1 := flag);
    // flag_as_int = 1

    value_as_bool := INT_TO_BOOL(IN1 := value);
    // value_as_bool = TRUE
END_PROGRAM
```

---

## Creating Custom Modules

You can extend the standard library by adding your own `.st` files to the `stdlib/` directory. Any `FUNCTION` or `FUNCTION_BLOCK` defined there will be automatically available in all programs.

### Steps

1. Create a new `.st` file in the `stdlib/` directory (e.g., `stdlib/my_blocks.st`).
2. Define your functions or function blocks using standard ST syntax.
3. They are immediately available in all programs -- no import needed.

### Guidelines

- Use `FUNCTION_BLOCK` when you need to retain state across scan cycles (e.g., filters, controllers, state machines).
- Use `FUNCTION` for stateless computations (e.g., math, conversions, scaling).
- Follow the naming conventions of the existing library: uppercase names for standard-style blocks, descriptive `VAR_INPUT`/`VAR_OUTPUT` names.
- Add a comment header describing what the module provides.

### Example

The file `playground/08_custom_module.st` demonstrates the pattern with three custom blocks:

- **Hysteresis** -- a function block that turns an output ON when an input exceeds a high threshold and OFF when it drops below a low threshold, preventing oscillation around a single setpoint.
- **Averager** -- a function block that computes a running average of input samples.
- **ScaleWithDeadBand** -- a function that applies a dead band and scale factor to a raw value.

```st
// Define a custom function block
FUNCTION_BLOCK Hysteresis
VAR_INPUT
    input_val      : INT;
    high_threshold : INT;
    low_threshold  : INT;
END_VAR
VAR_OUTPUT
    output : BOOL;
END_VAR
    IF input_val >= high_threshold THEN
        output := TRUE;
    ELSIF input_val <= low_threshold THEN
        output := FALSE;
    END_IF;
    // Between thresholds: output holds previous state
END_FUNCTION_BLOCK

// Use it in a program
PROGRAM Main
VAR
    ctrl : Hysteresis;
    temperature : INT;
END_VAR
    ctrl(input_val := temperature,
         high_threshold := 60,
         low_threshold := 40);

    // ctrl.output is TRUE when temp >= 60,
    // FALSE when temp <= 40,
    // unchanged in between
END_PROGRAM
```

To make this available globally, move the `FUNCTION_BLOCK Hysteresis` definition into a file under `stdlib/` (e.g., `stdlib/controllers.st`) and it will be loaded automatically for every program.
