# Variables

Variables in Structured Text are declared inside `VAR` blocks within a POU. The kind of
`VAR` block determines visibility, direction, and lifetime. This chapter covers every
variable section, qualifiers, initialization, and declaration syntax.

## Variable Sections

### VAR -- Local Variables

Local variables are private to the POU. They persist across scan cycles in programs and
function blocks, but are re-initialized on every call in functions.

```st
PROGRAM Main
  VAR
    cycle_count : INT := 0;
    pressure    : REAL;
  END_VAR

  cycle_count := cycle_count + 1;
END_PROGRAM
```

### VAR_INPUT -- Input Parameters

Inputs are passed by value into the POU by the caller. The POU may read but should not
write to them.

```st
FUNCTION_BLOCK Heater
  VAR_INPUT
    enable   : BOOL;
    setpoint : REAL := 70.0;
  END_VAR

  IF enable THEN
    // regulate to setpoint
  END_IF;
END_FUNCTION_BLOCK
```

### VAR_OUTPUT -- Output Parameters

Outputs are values produced by the POU that the caller can read after invocation via dot
notation on function block instances.

```st
FUNCTION_BLOCK TemperatureSensor
  VAR_OUTPUT
    value : REAL;
    valid : BOOL;
  END_VAR

  value := ReadAnalogInput();
  valid := (value > -40.0) AND (value < 150.0);
END_FUNCTION_BLOCK
```

Reading outputs:

```st
PROGRAM Main
  VAR
    sensor : TemperatureSensor;
  END_VAR

  sensor();
  IF sensor.valid THEN
    // use sensor.value
  END_IF;
END_PROGRAM
```

### VAR_IN_OUT -- Pass by Reference

`VAR_IN_OUT` parameters are passed by reference. The caller must supply a variable (not a
literal), and any modification inside the POU is reflected in the caller's variable.

```st
FUNCTION_BLOCK Accumulator
  VAR_IN_OUT
    total : REAL;
  END_VAR
  VAR_INPUT
    increment : REAL;
  END_VAR

  total := total + increment;
END_FUNCTION_BLOCK
```

```st
PROGRAM Main
  VAR
    running_sum : REAL := 0.0;
    acc         : Accumulator;
  END_VAR

  acc(total := running_sum, increment := 1.5);
  // running_sum is now 1.5
END_PROGRAM
```

### VAR_GLOBAL -- Global Variables

Global variables are declared at the top level (outside any POU or in a dedicated global
block) and are visible to all POUs that reference them via `VAR_EXTERNAL`.

```st
VAR_GLOBAL
  system_mode : INT := 0;
  alarm_active : BOOL := FALSE;
END_VAR
```

### VAR_EXTERNAL -- Referencing Globals

A POU uses `VAR_EXTERNAL` to access a previously declared global variable. The type must
match exactly.

```st
PROGRAM Main
  VAR_EXTERNAL
    system_mode : INT;
  END_VAR

  IF system_mode = 1 THEN
    // run mode
  END_IF;
END_PROGRAM
```

### VAR_TEMP -- Temporary Variables

Temporary variables exist only for one execution of the POU body. They are re-initialized
on every scan cycle, even in programs and function blocks (unlike `VAR`, which persists).

```st
PROGRAM Main
  VAR_TEMP
    scratch : INT;
  END_VAR

  scratch := HeavyComputation();
  // scratch is gone after this cycle's execution ends
END_PROGRAM
```

## Qualifiers

Qualifiers appear after the `VAR` keyword (or its variant) and before the variable
declarations.

### CONSTANT

Declares read-only variables. The value must be set at declaration and cannot be changed.

```st
VAR CONSTANT
  MAX_SPEED     : REAL := 1500.0;
  SENSOR_COUNT  : INT  := 8;
END_VAR
```

### RETAIN

Retained variables survive a warm restart of the PLC. Their values are stored in
non-volatile memory.

```st
VAR RETAIN
  total_runtime : TIME := T#0s;
  boot_count    : DINT := 0;
END_VAR
```

### PERSISTENT

Persistent variables survive both warm and cold restarts. They are only reset by an
explicit user action.

```st
VAR PERSISTENT
  machine_serial : STRING := '';
  calibration    : REAL   := 1.0;
END_VAR
```

### Combining Qualifiers

Qualifiers can be combined:

```st
VAR RETAIN PERSISTENT
  lifetime_hours : LREAL := 0.0;
END_VAR
```

## Initialization

Every variable can have an initializer using `:=`. Without one, the variable is
zero-initialized (0, FALSE, empty string, T#0s, etc.).

```st
VAR
  a : INT;            // initialized to 0
  b : INT := 42;      // initialized to 42
  c : BOOL;           // initialized to FALSE
  d : STRING := 'OK'; // initialized to 'OK'
END_VAR
```

### Structured initialization

Arrays and structs can be initialized with parenthesized lists:

```st
VAR
  temps  : ARRAY[1..5] OF REAL := [20.0, 21.5, 22.0, 19.8, 20.5];
  origin : Point := (x := 0.0, y := 0.0);
END_VAR
```

## Multiple Variables on One Line

Multiple variables of the same type can be declared on a single line, separated by
commas:

```st
VAR
  x, y, z    : REAL := 0.0;
  a, b       : INT;
  run, stop  : BOOL;
END_VAR
```

All variables on the line share the same type and initial value. If you need different
initial values, use separate declarations.

## Practical Example: State Machine Variables

```st
PROGRAM ConveyorControl
  VAR
    state      : INT := 0;
    speed      : REAL := 0.0;
    item_count : DINT := 0;
  END_VAR
  VAR_INPUT
    start_btn  : BOOL;
    stop_btn   : BOOL;
    sensor     : BOOL;
  END_VAR
  VAR_OUTPUT
    motor_cmd  : REAL;
    running    : BOOL;
  END_VAR
  VAR CONSTANT
    MAX_SPEED  : REAL := 2.0;  // m/s
  END_VAR
  VAR RETAIN
    total_items : DINT := 0;
  END_VAR

  CASE state OF
    0: // Idle
      IF start_btn THEN
        state := 1;
      END_IF;
    1: // Running
      speed := MAX_SPEED;
      IF sensor THEN
        item_count := item_count + 1;
        total_items := total_items + 1;
      END_IF;
      IF stop_btn THEN
        state := 0;
        speed := 0.0;
      END_IF;
  END_CASE;

  motor_cmd := speed;
  running := (state = 1);
END_PROGRAM
```

## Summary

| Section        | Direction | Persistence     | Passed by |
|----------------|-----------|-----------------|-----------|
| `VAR`          | Local     | Per POU type    | N/A       |
| `VAR_INPUT`    | In        | N/A             | Value     |
| `VAR_OUTPUT`   | Out       | N/A             | Value     |
| `VAR_IN_OUT`   | In/Out    | N/A             | Reference |
| `VAR_GLOBAL`   | Global    | Program scope   | N/A       |
| `VAR_EXTERNAL` | Global ref| Program scope   | N/A       |
| `VAR_TEMP`     | Local     | Single execution| N/A       |
