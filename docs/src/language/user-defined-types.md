# User-Defined Types

IEC 61131-3 allows you to define custom types using `TYPE ... END_TYPE` blocks. This
includes structures, enumerations, arrays, subrange types, and type aliases. User-defined
types improve readability, enforce constraints, and enable structured data modeling.

## TYPE Block Syntax

All user-defined types are declared inside `TYPE ... END_TYPE`:

```st
TYPE
  MyType : <type definition>;
END_TYPE
```

Multiple types can be declared in a single block.

## Structures (STRUCT)

Structures group related variables into a single composite type:

```st
TYPE
  MotorData : STRUCT
    speed    : REAL;
    current  : REAL;
    running  : BOOL;
    fault    : BOOL;
    op_hours : LREAL;
  END_STRUCT;
END_TYPE
```

### Using structures

```st
PROGRAM Main
  VAR
    pump_motor : MotorData;
    fan_motor  : MotorData;
  END_VAR

  pump_motor.speed := 1450.0;
  pump_motor.running := TRUE;

  IF pump_motor.fault THEN
    pump_motor.speed := 0.0;
    pump_motor.running := FALSE;
  END_IF;
END_PROGRAM
```

### Nested structures

Structures can contain other structures:

```st
TYPE
  Coordinate : STRUCT
    x : REAL;
    y : REAL;
    z : REAL;
  END_STRUCT;

  RobotArm : STRUCT
    position : Coordinate;
    target   : Coordinate;
    speed    : REAL;
    gripper  : BOOL;
  END_STRUCT;
END_TYPE

PROGRAM Main
  VAR
    arm : RobotArm;
  END_VAR

  arm.position.x := 100.0;
  arm.target.x := 200.0;
  arm.gripper := TRUE;
END_PROGRAM
```

### Initialization

Structure variables can be initialized with named field values:

```st
VAR
  origin : Coordinate := (x := 0.0, y := 0.0, z := 0.0);
  arm    : RobotArm := (speed := 50.0, gripper := FALSE);
END_VAR
```

Fields not explicitly initialized default to their zero value.

## Enumerations

Enumerations define a type with a fixed set of named values:

```st
TYPE
  MachineState : (IDLE, STARTING, RUNNING, STOPPING, FAULTED);
END_TYPE
```

### Using enumerations

```st
PROGRAM Main
  VAR
    state : MachineState := MachineState#IDLE;
  END_VAR

  CASE state OF
    MachineState#IDLE:
      IF start_button THEN
        state := MachineState#STARTING;
      END_IF;

    MachineState#STARTING:
      state := MachineState#RUNNING;

    MachineState#RUNNING:
      IF stop_button THEN
        state := MachineState#STOPPING;
      END_IF;

    MachineState#FAULTED:
      IF reset_button THEN
        state := MachineState#IDLE;
      END_IF;
  END_CASE;
END_PROGRAM
```

The qualified syntax `MachineState#IDLE` avoids ambiguity when multiple enumerations share
a value name.

### Enumerations with explicit values

You can assign integer values to enumeration members:

```st
TYPE
  AlarmPriority : (
    NONE     := 0,
    LOW      := 1,
    MEDIUM   := 2,
    HIGH     := 3,
    CRITICAL := 4
  );
END_TYPE
```

This is useful when the enumeration must map to specific protocol codes, register values,
or database entries.

```st
TYPE
  CommStatus : (
    OK            := 0,
    TIMEOUT       := 16#01,
    CRC_ERROR     := 16#02,
    FRAME_ERROR   := 16#04,
    DISCONNECTED  := 16#FF
  );
END_TYPE
```

## Arrays

Arrays are declared with index ranges using `ARRAY[low..high] OF type`:

```st
TYPE
  TenReals : ARRAY[1..10] OF REAL;
  SensorArray : ARRAY[0..7] OF INT;
END_TYPE
```

### Inline array declarations

Arrays can also be declared directly in variable sections without a separate `TYPE`:

```st
VAR
  temperatures : ARRAY[1..8] OF REAL;
  error_log    : ARRAY[0..99] OF INT;
END_VAR
```

### Multi-dimensional arrays

```st
TYPE
  Matrix3x3 : ARRAY[1..3, 1..3] OF REAL;
END_TYPE

PROGRAM Main
  VAR
    transform : Matrix3x3;
  END_VAR

  transform[1, 1] := 1.0;
  transform[2, 2] := 1.0;
  transform[3, 3] := 1.0;
  // identity matrix diagonal
END_PROGRAM
```

### Array initialization

```st
VAR
  weights : ARRAY[1..5] OF REAL := [1.0, 2.0, 3.0, 4.0, 5.0];
  flags   : ARRAY[0..3] OF BOOL := [TRUE, FALSE, FALSE, TRUE];
END_VAR
```

### Arrays of structures

```st
TYPE
  SensorReading : STRUCT
    channel : INT;
    value   : REAL;
    valid   : BOOL;
  END_STRUCT;
END_TYPE

VAR
  readings : ARRAY[1..16] OF SensorReading;
END_VAR
```

```st
FOR i := 1 TO 16 DO
  IF readings[i].valid THEN
    total := total + readings[i].value;
    count := count + 1;
  END_IF;
END_FOR;
```

## Subrange Types

A subrange type restricts an integer type to a specific range of values:

```st
TYPE
  Percentage : INT(0..100);
  DayOfWeek  : USINT(1..7);
  MotorRPM   : UINT(0..3600);
END_TYPE
```

Assigning a value outside the declared range is a runtime error:

```st
VAR
  level : Percentage;
END_VAR

level := 50;   // OK
level := 150;  // runtime error: out of range
```

Subrange types are useful for self-documenting code and catching logic errors early.

## Type Aliases

A type alias gives a new name to an existing type:

```st
TYPE
  Temperature : REAL;
  Pressure    : REAL;
  FlowRate    : REAL;
  ErrorCode   : DINT;
END_TYPE
```

Aliases improve readability and allow you to change the underlying type in one place:

```st
VAR
  tank_temp  : Temperature := 25.0;
  tank_press : Pressure := 101.3;
  flow       : FlowRate;
  error      : ErrorCode;
END_VAR
```

Note that aliases are **structurally typed** -- a `Temperature` and a `Pressure` are both
`REAL` and can be used interchangeably. They do not create distinct types for type-checking
purposes.

## Complete Example: Recipe System

```st
TYPE
  IngredientUnit : (GRAMS, MILLILITERS, UNITS);

  Ingredient : STRUCT
    name     : STRING[50];
    amount   : REAL;
    unit     : IngredientUnit;
    added    : BOOL;
  END_STRUCT;

  Recipe : STRUCT
    name        : STRING[100];
    ingredients : ARRAY[1..10] OF Ingredient;
    step_count  : INT;
    mix_time    : TIME;
    temperature : REAL;
  END_STRUCT;

  BatchState : (
    WAITING  := 0,
    LOADING  := 1,
    MIXING   := 2,
    HEATING  := 3,
    COMPLETE := 4,
    ERROR    := 99
  );
END_TYPE

PROGRAM BatchController
  VAR
    recipe : Recipe;
    state  : BatchState := BatchState#WAITING;
    step   : INT := 1;
  END_VAR

  CASE state OF
    BatchState#WAITING:
      IF start_cmd THEN
        state := BatchState#LOADING;
        step := 1;
      END_IF;

    BatchState#LOADING:
      IF step <= recipe.step_count THEN
        IF recipe.ingredients[step].added THEN
          step := step + 1;
        END_IF;
      ELSE
        state := BatchState#MIXING;
      END_IF;

    BatchState#MIXING:
      mixer := TRUE;
      // transition after mix_time elapsed

    BatchState#COMPLETE:
      mixer := FALSE;
      heater := FALSE;
  END_CASE;
END_PROGRAM
```

## Naming Caveat

When naming types or variables, be aware that all IEC 61131-3 keywords are
**case-insensitive**. A type or variable named `dt`, `Dt`, or `DT` conflicts with the `DT`
(Date and Time) keyword. Similarly, `tod`, `int`, or `real` as variable names will conflict
with their respective keywords. Always use descriptive names that avoid keyword collisions:

- Use `date_time` instead of `dt`
- Use `time_of_day` instead of `tod`
- Use `count` instead of `int`
