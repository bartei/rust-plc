# Statements

Statements are the executable instructions that make up a POU body. Structured Text
provides assignment, conditional branching, looping, and flow control. Every statement
ends with a semicolon.

## Assignment

The assignment operator is `:=`. The left side must be a variable; the right side is any
expression of a compatible type.

```st
VAR
  speed    : REAL;
  counter  : INT;
  running  : BOOL;
END_VAR

speed   := 1500.0;
counter := counter + 1;
running := speed > 0.0;
```

## IF / ELSIF / ELSE / END_IF

The `IF` statement executes blocks conditionally. `ELSIF` and `ELSE` branches are optional.

```st
IF temperature > 100.0 THEN
  alarm := TRUE;
  heater := FALSE;
ELSIF temperature < 60.0 THEN
  heater := TRUE;
ELSE
  // in range, maintain
  heater := heater;
END_IF;
```

Multiple `ELSIF` branches are allowed:

```st
IF level = 0 THEN
  state_name := 'IDLE';
ELSIF level = 1 THEN
  state_name := 'LOW';
ELSIF level = 2 THEN
  state_name := 'MEDIUM';
ELSIF level = 3 THEN
  state_name := 'HIGH';
ELSE
  state_name := 'UNKNOWN';
END_IF;
```

For many discrete values, prefer `CASE` (below).

## CASE / OF / END_CASE

The `CASE` statement selects among branches based on an integer or enumeration value.

```st
CASE machine_state OF
  0:
    // Idle
    motor := FALSE;
    valve := FALSE;

  1:
    // Starting
    motor := TRUE;
    valve := FALSE;

  2:
    // Running
    motor := TRUE;
    valve := TRUE;

  3:
    // Stopping
    motor := FALSE;
    valve := TRUE;

ELSE
  // Unknown state, emergency stop
  motor := FALSE;
  valve := FALSE;
  alarm := TRUE;
END_CASE;
```

### Multiple values per branch

A single branch can match several values using commas or ranges:

```st
CASE error_code OF
  0:
    status := 'OK';

  1, 2, 3:
    status := 'WARNING';

  10..19:
    status := 'SENSOR_FAULT';

  20..29:
    status := 'COMM_FAULT';

ELSE
  status := 'UNKNOWN';
END_CASE;
```

## FOR / TO / BY / DO / END_FOR

The `FOR` loop iterates a counter from a start value to an end value with an optional step.

```st
VAR
  i     : INT;
  total : INT := 0;
  data  : ARRAY[1..10] OF INT;
END_VAR

FOR i := 1 TO 10 DO
  total := total + data[i];
END_FOR;
```

### BY clause

The `BY` keyword specifies the step. It defaults to 1 if omitted.

```st
// Count down from 10 to 0
FOR i := 10 TO 0 BY -1 DO
  // process in reverse
END_FOR;
```

```st
// Step by 2 (even indices only)
FOR i := 0 TO 100 BY 2 DO
  even_sum := even_sum + i;
END_FOR;
```

The loop variable should not be modified inside the loop body. Doing so leads to
undefined behavior in standard IEC 61131-3.

## WHILE / DO / END_WHILE

The `WHILE` loop repeats as long as its condition is `TRUE`. The condition is checked
before each iteration, so the body may never execute.

```st
VAR
  pressure : REAL;
END_VAR

WHILE pressure < 100.0 DO
  pressure := pressure + ReadPressureIncrement();
END_WHILE;
```

A practical example -- draining a buffer:

```st
VAR
  buffer_count : INT;
END_VAR

WHILE buffer_count > 0 DO
  ProcessNextItem();
  buffer_count := buffer_count - 1;
END_WHILE;
```

## REPEAT / UNTIL / END_REPEAT

The `REPEAT` loop is like `WHILE` but checks its condition **after** the body, so the body
always executes at least once.

```st
VAR
  attempts : INT := 0;
  success  : BOOL := FALSE;
END_VAR

REPEAT
  attempts := attempts + 1;
  success := TryConnect();
UNTIL success OR (attempts >= 3)
END_REPEAT;
```

Note: the condition follows `UNTIL` without a `THEN` or `DO`. The loop exits when the
condition becomes `TRUE`.

## RETURN

`RETURN` exits the current POU immediately. In a function, it returns whatever value has
been assigned to the function name so far. In a program or function block, it ends the
current scan cycle execution for that POU.

```st
FUNCTION SafeDivide : REAL
  VAR_INPUT
    numerator   : REAL;
    denominator : REAL;
  END_VAR

  IF denominator = 0.0 THEN
    SafeDivide := 0.0;
    RETURN;
  END_IF;

  SafeDivide := numerator / denominator;
END_FUNCTION
```

## EXIT

`EXIT` breaks out of the innermost `FOR`, `WHILE`, or `REPEAT` loop.

```st
VAR
  i     : INT;
  found : INT := -1;
  data  : ARRAY[1..100] OF INT;
END_VAR

FOR i := 1 TO 100 DO
  IF data[i] = 42 THEN
    found := i;
    EXIT;
  END_IF;
END_FOR;
// If found <> -1, data[found] = 42
```

`EXIT` only affects the innermost loop. In nested loops, the outer loop continues:

```st
VAR
  row, col : INT;
END_VAR

FOR row := 1 TO 10 DO
  FOR col := 1 TO 10 DO
    IF matrix[row, col] = 0 THEN
      EXIT;  // exits inner loop only
    END_IF;
  END_FOR;
  // continues with next row
END_FOR;
```

## Empty Statement

A lone semicolon is a valid empty statement. It does nothing and can be useful as a
placeholder:

```st
IF condition THEN
  ;  // intentionally empty, to be implemented
END_IF;
```

## Complete Example: Simple State Machine

```st
PROGRAM BatchMixer
  VAR
    state     : INT := 0;
    timer     : INT := 0;
    fill_done : BOOL;
    mix_time  : INT := 100;
  END_VAR

  CASE state OF
    0: // IDLE
      IF start_cmd THEN
        state := 1;
        timer := 0;
      END_IF;

    1: // FILLING
      fill_valve := TRUE;
      IF fill_done THEN
        fill_valve := FALSE;
        state := 2;
        timer := 0;
      END_IF;

    2: // MIXING
      mixer_motor := TRUE;
      timer := timer + 1;
      IF timer >= mix_time THEN
        mixer_motor := FALSE;
        state := 3;
      END_IF;

    3: // DRAINING
      drain_valve := TRUE;
      IF level <= 0 THEN
        drain_valve := FALSE;
        state := 0;
      END_IF;

  ELSE
    // fault recovery
    state := 0;
  END_CASE;
END_PROGRAM
```

```
st-cli run batch_mixer.st
```
