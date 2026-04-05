# Program Structure

IEC 61131-3 Structured Text organizes code into **Program Organization Units** (POUs).
There are three kinds of POU: `PROGRAM`, `FUNCTION`, and `FUNCTION_BLOCK`. Each serves a
distinct role in a well-structured automation project.

## PROGRAM

A `PROGRAM` is the top-level entry point. Every ST file executed with `st-cli run` must
contain at least one program. A program can declare local variables, call functions and
function block instances, and perform I/O.

```st
PROGRAM Main
  VAR
    counter : INT := 0;
    running : BOOL := TRUE;
  END_VAR

  IF running THEN
    counter := counter + 1;
  END_IF;
END_PROGRAM
```

Run it:

```
st-cli run main.st
```

A program's variables **retain their values** between scan cycles. This is a key difference
from functions, whose locals are re-initialized on every call.

## FUNCTION

A `FUNCTION` is a stateless POU that computes a return value. It has no persistent local
state -- every invocation starts fresh. Functions are the right choice for pure
computations such as unit conversions, math helpers, or validation checks.

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
```

The return value is assigned by writing to the function's own name (`Clamp := ...`).

## FUNCTION_BLOCK

A `FUNCTION_BLOCK` is a stateful POU. You create **instances** of a function block, and
each instance maintains its own private copy of all internal variables across calls. This
is the standard building block for timers, counters, PID controllers, and state machines.

```st
FUNCTION_BLOCK PulseCounter
  VAR_INPUT
    pulse : BOOL;
  END_VAR
  VAR_OUTPUT
    count : INT;
  END_VAR
  VAR
    prev_pulse : BOOL := FALSE;
  END_VAR

  // Rising-edge detection
  IF pulse AND NOT prev_pulse THEN
    count := count + 1;
  END_IF;
  prev_pulse := pulse;
END_FUNCTION_BLOCK
```

Instantiate and use it inside a program:

```st
PROGRAM Main
  VAR
    sensor_counter : PulseCounter;
    sensor_input   : BOOL;
  END_VAR

  sensor_counter(pulse := sensor_input);

  IF sensor_counter.count > 100 THEN
    // threshold reached
  END_IF;
END_PROGRAM
```

## How POUs Relate

| Feature              | PROGRAM          | FUNCTION         | FUNCTION_BLOCK    |
|----------------------|------------------|------------------|-------------------|
| Has persistent state | Yes              | No               | Yes (per instance)|
| Return value         | No               | Yes (one)        | No (use outputs)  |
| Can be instantiated  | No (singleton)   | No (called)      | Yes               |
| Can call functions   | Yes              | Yes              | Yes               |
| Can call FB instances| Yes              | No               | Yes               |

Functions must remain side-effect-free in standard IEC 61131-3: they cannot instantiate
function blocks or write to global variables. This compiler relaxes some of those rules,
but keeping functions pure is strongly recommended.

## POU Lifecycle

1. **Programs** are instantiated once when the runtime starts. Their `VAR` sections are
   initialized at that point. On each scan cycle the program body executes, and variables
   persist until the next cycle.

2. **Functions** are called, execute, and return. Local variables exist only for the
   duration of the call. No state carries over between invocations.

3. **Function blocks** are instantiated as variables (typically inside a program or another
   function block). Their internal state is initialized when the instance is created and
   persists across every subsequent call. Each instance is independent.

## Nesting and Composition

A program can instantiate multiple function blocks, and function blocks can instantiate
other function blocks internally. This enables hierarchical composition:

```st
FUNCTION_BLOCK MotorController
  VAR_INPUT
    enable   : BOOL;
    set_rpm  : REAL;
  END_VAR
  VAR_OUTPUT
    at_speed : BOOL;
  END_VAR
  VAR
    ramp : RampGenerator;
    pid  : PID_Controller;
  END_VAR

  ramp(target := set_rpm, enable := enable);
  pid(setpoint := ramp.output, enable := enable);
  at_speed := ABS(pid.error) < 5.0;
END_FUNCTION_BLOCK
```

## Minimal Complete Example

```st
FUNCTION DoubleIt : INT
  VAR_INPUT
    x : INT;
  END_VAR
  DoubleIt := x * 2;
END_FUNCTION

PROGRAM Main
  VAR
    result : INT;
  END_VAR

  result := DoubleIt(x := 10);
  // result is now 20
END_PROGRAM
```

```
st-cli run example.st
```

Programs, functions, and function blocks can all coexist in the same source file or be
split across multiple files depending on project conventions.
