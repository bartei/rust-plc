# Standard Library

The IEC 61131-3 standard library provides a set of reusable function blocks and functions commonly needed in PLC programming. The library is implemented as plain Structured Text source files in the `stdlib/` directory and is automatically loaded by the compiler via `builtin_stdlib()` -- all standard library functions and function blocks are available in every program without any import statements.

The library includes:

| Module | Source File | Contents |
|--------|------------|----------|
| [Counters](#counters) | `stdlib/counters.st` | CTU, CTD, CTUD |
| [Edge Detection](#edge-detection) | `stdlib/edge_detection.st` | R_TRIG, F_TRIG |
| [Timers](#timers) | `stdlib/timers.st` | TON, TOF, TP |
| [Math & Selection](#math--selection) | `stdlib/math.st` | MAX, MIN, LIMIT, ABS, SEL |
| [Type Conversions](#type-conversions) | Compiler intrinsics | 60+ *_TO_* functions including TIME conversions and TO_*/ANY_TO_* generics |
| [Trig & Math Intrinsics](#trigonometric--math-intrinsics) | Compiler intrinsics | SQRT, SIN, COS, TAN, ASIN, ACOS, ATAN, LN, LOG, EXP |
| [System Time](#system-time) | Compiler intrinsic | SYSTEM_TIME() |

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

Timers use **real-time TIME values** and the `SYSTEM_TIME()` intrinsic to measure wall-clock elapsed time. The preset value `PT` is a `TIME` type specified using TIME literals (e.g., `T#5s`, `T#100ms`, `T#1m30s`). This makes timers independent of scan cycle speed.

> **Note:** The timer input is named `IN1` (not `IN`) to avoid a keyword conflict in Structured Text.

All three timer blocks share the same input/output signature:

**Inputs**

| Name | Type | Description |
|------|------|-------------|
| `IN1` | BOOL | Timer input |
| `PT` | TIME | Preset time (e.g., `T#5s`, `T#500ms`) |

**Outputs**

| Name | Type | Description |
|------|------|-------------|
| `Q` | BOOL | Timer output |
| `ET` | TIME | Elapsed time |

### TON -- On-Delay Timer

`Q` goes TRUE after `IN1` has been TRUE for at least the duration `PT`. When `IN1` goes FALSE, both `Q` and `ET` reset immediately. Internally, the timer records the start time via `SYSTEM_TIME()` when `IN1` first goes TRUE, and computes `ET` as the difference on each scan.

```
IN1:  _____|‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾|_____
ET:   T#0s  ... increasing ... T#0s
Q:    _____|          ‾‾‾‾‾‾‾|_____
              ^-- ET >= PT reached
```

### TOF -- Off-Delay Timer

`Q` goes TRUE immediately when `IN1` goes TRUE. When `IN1` goes FALSE, `Q` stays TRUE for the duration `PT` before turning FALSE.

```
IN1:  _____|‾‾‾‾‾|________________________
Q:    _____|‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾‾|___________
ET:   T#0s  T#0s  ... increasing ... T#0s
                          ^-- ET >= PT, Q goes FALSE
```

### TP -- Pulse Timer

On a rising edge of `IN1`, `Q` goes TRUE for exactly the duration `PT`, regardless of what `IN1` does during the pulse. A new pulse cannot be triggered while the current one is active.

```
IN1:  _____|‾‾‾‾‾‾‾‾‾‾‾‾‾|________
Q:    _____|‾‾‾‾‾‾‾‾‾|____________
ET:   T#0s  ... increasing ... T#0s
                  ^-- ET >= PT, pulse ends
```

**Example**

```st
PROGRAM TimerExample
VAR
    debounce : TON;
    raw_input : BOOL;
    clean_input : BOOL;
END_VAR
    // Debounce: require input to be stable for 5 seconds
    debounce(IN1 := raw_input, PT := T#5s);
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

Type conversion functions are implemented as **compiler intrinsics**. The compiler recognizes `*_TO_*` function name patterns and emits dedicated VM instructions (`ToInt`, `ToReal`, `ToBool`, `ToTime`) directly, rather than calling a user-defined function. The file `stdlib/conversions.st` serves as documentation for all available conversions.

All conversion functions take a single parameter `IN1` and return the converted value.

### To REAL / LREAL

| Function | Description |
|----------|-------------|
| `INT_TO_REAL` | Integer to REAL |
| `SINT_TO_REAL` | Short integer to REAL |
| `DINT_TO_REAL` | Double integer to REAL |
| `LINT_TO_REAL` | Long integer to REAL |
| `UINT_TO_REAL` | Unsigned integer to REAL |
| `USINT_TO_REAL` | Unsigned short integer to REAL |
| `UDINT_TO_REAL` | Unsigned double integer to REAL |
| `ULINT_TO_REAL` | Unsigned long integer to REAL |
| `BOOL_TO_REAL` | Boolean to REAL (FALSE=0.0, TRUE=1.0) |
| `TIME_TO_REAL` | TIME to REAL (milliseconds as float) |
| `TIME_TO_LREAL` | TIME to LREAL (milliseconds as float) |
| `INT_TO_LREAL` | Integer to LREAL |
| `SINT_TO_LREAL` | Short integer to LREAL |
| `DINT_TO_LREAL` | Double integer to LREAL |
| `LINT_TO_LREAL` | Long integer to LREAL |
| `REAL_TO_LREAL` | REAL to LREAL |

### To INT / DINT / LINT / SINT (and unsigned variants)

| Function | Description |
|----------|-------------|
| `REAL_TO_INT` | REAL to integer (truncates) |
| `LREAL_TO_INT` | LREAL to integer (truncates) |
| `REAL_TO_DINT` | REAL to double integer |
| `LREAL_TO_DINT` | LREAL to double integer |
| `REAL_TO_LINT` | REAL to long integer |
| `LREAL_TO_LINT` | LREAL to long integer |
| `REAL_TO_SINT` | REAL to short integer |
| `LREAL_TO_SINT` | LREAL to short integer |
| `BOOL_TO_INT` | Boolean to integer (FALSE=0, TRUE=1) |
| `BOOL_TO_DINT` | Boolean to double integer |
| `BOOL_TO_LINT` | Boolean to long integer |
| `UINT_TO_INT` | Unsigned integer to signed integer |
| `UDINT_TO_DINT` | Unsigned double integer to signed |
| `ULINT_TO_LINT` | Unsigned long integer to signed |
| `INT_TO_DINT` | Integer to double integer |
| `INT_TO_LINT` | Integer to long integer |
| `DINT_TO_LINT` | Double integer to long integer |
| `SINT_TO_INT` | Short integer to integer |
| `SINT_TO_DINT` | Short integer to double integer |
| `SINT_TO_LINT` | Short integer to long integer |
| `TIME_TO_INT` | TIME to integer (milliseconds) |
| `TIME_TO_SINT` | TIME to short integer (milliseconds, may truncate) |
| `TIME_TO_DINT` | TIME to double integer (milliseconds) |
| `TIME_TO_LINT` | TIME to long integer (milliseconds) |
| `TIME_TO_UINT` | TIME to unsigned integer (milliseconds) |
| `TIME_TO_USINT` | TIME to unsigned short integer (milliseconds) |
| `TIME_TO_UDINT` | TIME to unsigned double integer (milliseconds) |
| `TIME_TO_ULINT` | TIME to unsigned long integer (milliseconds) |

### To BOOL

| Function | Description |
|----------|-------------|
| `INT_TO_BOOL` | Integer to boolean (0=FALSE, nonzero=TRUE) |
| `REAL_TO_BOOL` | REAL to boolean |
| `DINT_TO_BOOL` | Double integer to boolean |
| `LINT_TO_BOOL` | Long integer to boolean |
| `TIME_TO_BOOL` | TIME to boolean (T#0ms=FALSE, nonzero=TRUE) |

### To TIME

TIME values are stored internally as milliseconds (i64). When converting a numeric value to TIME, the integer is interpreted as milliseconds. When converting TIME to a numeric value, the result is the millisecond count.

| Function | Description |
|----------|-------------|
| `INT_TO_TIME` | Integer (ms) to TIME |
| `SINT_TO_TIME` | Short integer (ms) to TIME |
| `DINT_TO_TIME` | Double integer (ms) to TIME |
| `LINT_TO_TIME` | Long integer (ms) to TIME |
| `UINT_TO_TIME` | Unsigned integer (ms) to TIME |
| `USINT_TO_TIME` | Unsigned short integer (ms) to TIME |
| `UDINT_TO_TIME` | Unsigned double integer (ms) to TIME |
| `ULINT_TO_TIME` | Unsigned long integer (ms) to TIME |
| `REAL_TO_TIME` | REAL (ms, truncated) to TIME |
| `LREAL_TO_TIME` | LREAL (ms, truncated) to TIME |
| `BOOL_TO_TIME` | Boolean to TIME (FALSE=T#0ms, TRUE=T#1ms) |

### Overloaded Generic Conversions (TO_\* / ANY_TO_\*)

These accept **any input type** and convert to the target type. The compiler resolves the appropriate conversion at compile time. `TO_<target>` and `ANY_TO_<target>` are functionally identical -- the `ANY_TO_` prefix follows IEC 61131-3 Edition 3 naming, while `TO_` follows the CODESYS convention.

| Function | Equivalent to |
|----------|---------------|
| `TO_INT` / `ANY_TO_INT` | `*_TO_INT` (returns INT) |
| `TO_SINT` / `ANY_TO_SINT` | `*_TO_SINT` (returns INT, narrowed at store) |
| `TO_DINT` / `ANY_TO_DINT` | `*_TO_DINT` (returns INT) |
| `TO_LINT` / `ANY_TO_LINT` | `*_TO_LINT` (returns INT) |
| `TO_UINT` / `ANY_TO_UINT` | `*_TO_UINT` (returns INT, unsigned) |
| `TO_USINT` / `ANY_TO_USINT` | `*_TO_USINT` (returns INT, unsigned) |
| `TO_UDINT` / `ANY_TO_UDINT` | `*_TO_UDINT` (returns INT, unsigned) |
| `TO_ULINT` / `ANY_TO_ULINT` | `*_TO_ULINT` (returns INT, unsigned) |
| `TO_REAL` / `ANY_TO_REAL` | `*_TO_REAL` (returns REAL) |
| `TO_LREAL` / `ANY_TO_LREAL` | `*_TO_LREAL` (returns LREAL) |
| `TO_BOOL` / `ANY_TO_BOOL` | `*_TO_BOOL` (returns BOOL) |
| `TO_TIME` / `ANY_TO_TIME` | `*_TO_TIME` (returns TIME) |

**Example**

```st
PROGRAM ConversionExample
VAR
    flag : BOOL := TRUE;
    flag_as_int : INT;
    my_real : REAL;
    my_int : INT := 42;
    value_as_bool : BOOL;
    delay : TIME := T#5s;
    delay_ms : INT;
    custom_time : TIME;
END_VAR
    // Typed conversions
    flag_as_int := BOOL_TO_INT(IN1 := flag);        // 1
    my_real := INT_TO_REAL(IN1 := my_int);           // 42.0
    value_as_bool := INT_TO_BOOL(IN1 := my_int);     // TRUE

    // TIME conversions
    delay_ms := TIME_TO_INT(IN1 := delay);           // 5000
    custom_time := DINT_TO_TIME(IN1 := 2500);        // T#2s500ms

    // Generic conversions (accept any input type)
    delay_ms := TO_INT(IN1 := delay);                // 5000
    custom_time := TO_TIME(IN1 := 3000);             // T#3s
    my_real := ANY_TO_REAL(IN1 := delay);            // 5000.0
END_PROGRAM
```

---

## DATE, TOD, and DT Conversions

DATE, TOD (TIME_OF_DAY), and DT (DATE_AND_TIME) types are stored internally as milliseconds (i64), the same representation as TIME:

- **DATE** -- milliseconds since Unix epoch (1970-01-01)
- **TOD** -- milliseconds since midnight
- **DT** -- milliseconds since Unix epoch (includes time of day)

> **Note:** DATE and DT values in milliseconds exceed the DINT (32-bit) range. Use LINT for numeric conversions of DATE/DT values.

> **TOD wrapping:** TOD values are always wrapped modulo 86,400,000 ms (24 hours), matching CODESYS behavior. This applies to `*_TO_TOD` conversions, `ADD_TOD_TIME`, and `SUB_TOD_TIME`. For example, `ADD_TOD_TIME(TOD#23:00:00, T#2h)` wraps to `TOD#01:00:00`.

### DATE / TOD / DT to Numeric

| Function | Description |
|----------|-------------|
| `DATE_TO_INT` / `DATE_TO_DINT` / `DATE_TO_LINT` | DATE to integer (ms since epoch) |
| `DATE_TO_REAL` / `DATE_TO_LREAL` | DATE to float (ms since epoch) |
| `DATE_TO_BOOL` | DATE to boolean (epoch=FALSE, nonzero=TRUE) |
| `TOD_TO_INT` / `TOD_TO_DINT` / `TOD_TO_LINT` | TOD to integer (ms since midnight) |
| `TOD_TO_REAL` / `TOD_TO_LREAL` | TOD to float (ms since midnight) |
| `TOD_TO_BOOL` | TOD to boolean (midnight=FALSE, nonzero=TRUE) |
| `DT_TO_INT` / `DT_TO_DINT` / `DT_TO_LINT` | DT to integer (ms since epoch) |
| `DT_TO_REAL` / `DT_TO_LREAL` | DT to float (ms since epoch) |
| `DT_TO_BOOL` | DT to boolean (epoch=FALSE, nonzero=TRUE) |

### Numeric to DATE / TOD / DT

| Function | Description |
|----------|-------------|
| `INT_TO_DATE` / `DINT_TO_DATE` / `LINT_TO_DATE` | Integer (ms since epoch) to DATE |
| `REAL_TO_DATE` / `LREAL_TO_DATE` | Float (ms, truncated) to DATE |
| `INT_TO_TOD` / `DINT_TO_TOD` / `LINT_TO_TOD` | Integer (ms since midnight) to TOD (wraps modulo 24h) |
| `REAL_TO_TOD` / `LREAL_TO_TOD` | Float (ms, truncated) to TOD (wraps modulo 24h) |
| `INT_TO_DT` / `DINT_TO_DT` / `LINT_TO_DT` | Integer (ms since epoch) to DT |
| `REAL_TO_DT` / `LREAL_TO_DT` | Float (ms, truncated) to DT |

### Cross-Type Extraction and Combination

| Function | Description |
|----------|-------------|
| `DT_TO_DATE` | Extract date portion from DT (truncates to day boundary) |
| `DT_TO_TOD` | Extract time-of-day portion from DT |
| `CONCAT_DATE_TOD(IN1, IN2)` | Combine DATE + TOD into DT |
| `DATE_TO_DT` / `TIME_TO_DATE` / `TIME_TO_DT` | Cross-type cast (reinterpret ms value) |
| `DATE_TO_TIME` / `TOD_TO_TIME` / `DT_TO_TIME` | Cross-type cast (reinterpret ms value) |

### Date/Time Arithmetic

| Function | Signature | Description |
|----------|-----------|-------------|
| `ADD_TOD_TIME` | `(IN1: TOD, IN2: TIME) : TOD` | Add duration to time-of-day |
| `ADD_DT_TIME` | `(IN1: DT, IN2: TIME) : DT` | Add duration to date-and-time |
| `SUB_TOD_TIME` | `(IN1: TOD, IN2: TIME) : TOD` | Subtract duration from TOD |
| `SUB_DATE_DATE` | `(IN1: DATE, IN2: DATE) : TIME` | Difference between two dates |
| `SUB_TOD_TOD` | `(IN1: TOD, IN2: TOD) : TIME` | Difference between two TODs |
| `SUB_DT_TIME` | `(IN1: DT, IN2: TIME) : DT` | Subtract duration from DT |
| `SUB_DT_DT` | `(IN1: DT, IN2: DT) : TIME` | Difference between two DTs |
| `MULTIME` | `(IN1: TIME, IN2: INT) : TIME` | Multiply duration by integer |
| `DIVTIME` | `(IN1: TIME, IN2: INT) : TIME` | Divide duration by integer |
| `DAY_OF_WEEK` | `(IN1: DATE) : INT` | Day of week (0=Sun, 1=Mon, ..., 6=Sat) |

### Generic Conversions

| Function | Description |
|----------|-------------|
| `TO_DATE` / `ANY_TO_DATE` | Any type to DATE |
| `TO_TOD` / `ANY_TO_TOD` | Any type to TOD |
| `TO_DT` / `ANY_TO_DT` | Any type to DT |

**Example**

```st
PROGRAM DateTimeExample
VAR
    my_date  : DATE := D#2024-01-15;
    my_tod   : TOD  := TOD#12:30:00;
    my_dt    : DT   := DT#2024-01-15-12:30:00;
    combined : DT;
    date_part : DATE;
    tod_part  : TOD;
    diff     : TIME;
    dow      : INT;
    scaled   : TIME;
END_VAR
    // Combine date + time-of-day into DT
    combined := CONCAT_DATE_TOD(IN1 := my_date, IN2 := my_tod);

    // Extract components from DT
    date_part := DT_TO_DATE(IN1 := my_dt);   // D#2024-01-15
    tod_part  := DT_TO_TOD(IN1 := my_dt);    // TOD#12:30:00

    // Date arithmetic
    diff := SUB_DATE_DATE(IN1 := D#2024-01-15, IN2 := D#2024-01-01);
    // diff = T#14d (14 days in ms)

    // Day of week
    dow := DAY_OF_WEEK(IN1 := my_date);      // 1 = Monday

    // Scale a duration
    scaled := MULTIME(IN1 := T#1s, IN2 := 5); // T#5s
    scaled := DIVTIME(IN1 := T#10s, IN2 := 2); // T#5s
END_PROGRAM
```

> **Not yet implemented:** `TIME_TO_STRING`, `STRING_TO_TIME`, `SPLIT_DATE`, `SPLIT_TOD`, `SPLIT_DT`, `CONCAT_DATE` (from components), `CONCAT_TOD`, `CONCAT_DT`. These require string formatting and multi-output function infrastructure.

---

## Trigonometric & Math Intrinsics

These functions are **VM intrinsic instructions** -- the compiler recognizes the function name and emits a dedicated bytecode instruction. They operate on REAL values.

| Function | Signature | Description |
|----------|-----------|-------------|
| `SQRT` | `SQRT(IN1: REAL) : REAL` | Square root |
| `SIN` | `SIN(IN1: REAL) : REAL` | Sine (radians) |
| `COS` | `COS(IN1: REAL) : REAL` | Cosine (radians) |
| `TAN` | `TAN(IN1: REAL) : REAL` | Tangent (radians) |
| `ASIN` | `ASIN(IN1: REAL) : REAL` | Arc sine |
| `ACOS` | `ACOS(IN1: REAL) : REAL` | Arc cosine |
| `ATAN` | `ATAN(IN1: REAL) : REAL` | Arc tangent |
| `LN` | `LN(IN1: REAL) : REAL` | Natural logarithm |
| `LOG` | `LOG(IN1: REAL) : REAL` | Base-10 logarithm |
| `EXP` | `EXP(IN1: REAL) : REAL` | Exponential (e^x) |

**Example**

```st
PROGRAM TrigExample
VAR
    angle : REAL := 1.5708;   // approx pi/2
    result : REAL;
    root : REAL;
END_VAR
    result := SIN(IN1 := angle);
    // result ~ 1.0

    root := SQRT(IN1 := 144.0);
    // root = 12.0
END_PROGRAM
```

---

## System Time

`SYSTEM_TIME()` is a compiler intrinsic that returns the elapsed time in milliseconds since the engine started, as a `TIME` value. It is used internally by the standard library timers (TON, TOF, TP) and can also be called directly from user programs.

**Example**

```st
PROGRAM TimestampExample
VAR
    now : TIME;
END_VAR
    now := SYSTEM_TIME();
    // now contains the elapsed time since engine start
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
