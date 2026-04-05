# Function Blocks

A `FUNCTION_BLOCK` is a stateful Program Organisation Unit. Unlike plain
functions, function blocks retain their internal variables across calls,
making them the primary building block for timers, counters, PID loops,
state machines, and communication handlers.

## Declaration

```st
FUNCTION_BLOCK BlockName
  VAR_INPUT
    enable : BOOL;         (* supplied by caller *)
  END_VAR
  VAR_OUTPUT
    result : INT;          (* readable by caller after the call *)
  END_VAR
  VAR_IN_OUT
    shared : REAL;         (* passed by reference *)
  END_VAR
  VAR
    internal : INT := 0;   (* private persistent state *)
  END_VAR

  (* body *)
END_FUNCTION_BLOCK
```

### Variable Sections

| Section       | Direction | Semantics                                |
|---------------|-----------|------------------------------------------|
| `VAR_INPUT`   | In        | Copied from caller at invocation         |
| `VAR_OUTPUT`  | Out       | Readable by caller via dot notation      |
| `VAR_IN_OUT`  | In/Out    | Passed by reference; caller must supply a variable, not a literal |
| `VAR`         | Private   | Internal state, invisible to caller      |

## Instantiation

Function blocks are used by declaring **instances** as variables. Each
instance owns an independent copy of all internal state.

```st
PROGRAM Main
  VAR
    counter1 : UpCounter;
    counter2 : UpCounter;
  END_VAR

  counter1(increment := TRUE);
  counter2(increment := FALSE);
  (* counter1 and counter2 have completely separate state *)
END_PROGRAM
```

## Calling with Named Parameters

Invoke an instance by writing its name followed by parenthesised named
arguments using the `:=` assignment syntax:

```st
delay(IN := start_signal, PT := T#5s);
```

All `VAR_INPUT` parameters that have no default must be supplied.
Parameters with defaults may be omitted.

## Accessing Outputs via Dot Notation

After calling an instance, read its `VAR_OUTPUT` fields with
`instance.output`:

```st
delay(IN := start_signal, PT := T#5s);

IF delay.Q THEN
  (* 5 seconds have elapsed *)
END_IF;

elapsed := delay.ET;
```

You may also read outputs without calling first, which returns the value
from the previous cycle.

## State Persistence Across Calls

`VAR` and `VAR_OUTPUT` variables keep their values between calls. This
is the key difference from a plain `FUNCTION`:

```st
FUNCTION_BLOCK UpCounter
  VAR_INPUT
    increment : BOOL;
    reset     : BOOL;
  END_VAR
  VAR_OUTPUT
    count : INT := 0;
  END_VAR
  VAR
    prev_increment : BOOL := FALSE;
  END_VAR

  IF reset THEN
    count := 0;
  ELSIF increment AND NOT prev_increment THEN
    count := count + 1;   (* rising edge detection *)
  END_IF;

  prev_increment := increment;
END_FUNCTION_BLOCK
```

Each scan cycle picks up exactly where the last one left off.

## Realistic Example: Timer-Like Block

```st
FUNCTION_BLOCK CycleTimer
  VAR_INPUT
    IN : BOOL;
    PT : INT;        (* preset in scan cycles *)
  END_VAR
  VAR_OUTPUT
    Q  : BOOL;
    ET : INT := 0;
  END_VAR
  VAR
    running : BOOL := FALSE;
  END_VAR

  IF IN THEN
    IF NOT running THEN
      ET := 0;
      running := TRUE;
    END_IF;
    ET := ET + 1;
    Q := ET >= PT;
  ELSE
    running := FALSE;
    Q := FALSE;
    ET := 0;
  END_IF;
END_FUNCTION_BLOCK

PROGRAM Main
  VAR
    btn     : BOOL;
    tmr     : CycleTimer;
    motor   : BOOL;
  END_VAR

  tmr(IN := btn, PT := 100);
  motor := tmr.Q;
END_PROGRAM
```

## Nesting Function Blocks

A function block can instantiate other function blocks in its `VAR`
section:

```st
FUNCTION_BLOCK Controller
  VAR
    filter  : LowPass;
    limiter : Clamp;
  END_VAR
  (* ... *)
  filter(input := raw_value, alpha := 0.1);
  limiter(value := filter.output, lo := 0.0, hi := 100.0);
  result := limiter.clamped;
END_FUNCTION_BLOCK
```

## Summary

| Aspect             | FUNCTION           | FUNCTION_BLOCK            |
|--------------------|--------------------|---------------------------|
| State persistence  | None               | Yes, per instance         |
| Return value       | One, via name      | None (use `VAR_OUTPUT`)   |
| Instantiation      | Called directly     | Declared as a variable    |
| Use cases          | Pure computation   | Timers, counters, state   |
