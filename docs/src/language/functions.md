# Functions

A `FUNCTION` is a stateless POU that accepts inputs, performs a computation, and returns
a single value. Functions have no persistent state -- local variables are initialized fresh
on every call. This makes functions ideal for pure computations: conversions, math helpers,
validation, and formatting.

## Declaration

A function declaration specifies the function name and its return type after the colon:

```st
FUNCTION FunctionName : ReturnType
  VAR_INPUT
    // input parameters
  END_VAR
  VAR
    // local variables
  END_VAR

  // body
END_FUNCTION
```

## Return Value

The return value is assigned by writing to the function name itself:

```st
FUNCTION CelsiusToFahrenheit : REAL
  VAR_INPUT
    celsius : REAL;
  END_VAR

  CelsiusToFahrenheit := celsius * 9.0 / 5.0 + 32.0;
END_FUNCTION
```

You may assign to the function name multiple times. The last assigned value before the
function returns is the result:

```st
FUNCTION Classify : INT
  VAR_INPUT
    value : REAL;
  END_VAR

  Classify := 0;  // default: nominal

  IF value > 100.0 THEN
    Classify := 2;  // high
  ELSIF value > 80.0 THEN
    Classify := 1;  // warning
  ELSIF value < 0.0 THEN
    Classify := -1; // underrange
  END_IF;
END_FUNCTION
```

Use `RETURN` to exit early after assigning the return value:

```st
FUNCTION SafeSqrt : REAL
  VAR_INPUT
    x : REAL;
  END_VAR

  IF x < 0.0 THEN
    SafeSqrt := -1.0;
    RETURN;
  END_IF;

  SafeSqrt := SQRT(x);
END_FUNCTION
```

## Calling with Named Arguments

Named (formal) argument syntax passes each parameter by name using `:=`:

```st
PROGRAM Main
  VAR
    temp_f : REAL;
  END_VAR

  temp_f := CelsiusToFahrenheit(celsius := 100.0);
  // temp_f = 212.0
END_PROGRAM
```

Named arguments can appear in any order:

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
    result : REAL;
  END_VAR

  // Order does not matter with named arguments
  result := Clamp(high := 100.0, value := 150.0, low := 0.0);
  // result = 100.0
END_PROGRAM
```

## Calling with Positional Arguments

Arguments can also be passed positionally, matching the declaration order of `VAR_INPUT`:

```st
result := Clamp(150.0, 0.0, 100.0);
// value=150.0, low=0.0, high=100.0
```

Do not mix positional and named arguments in the same call. Use one style consistently.

## Local Variables

Functions can declare local variables in a `VAR` block. These are initialized on every
call and do not persist:

```st
FUNCTION Average : REAL
  VAR_INPUT
    a, b, c : REAL;
  END_VAR
  VAR
    sum : REAL;
  END_VAR

  sum := a + b + c;
  Average := sum / 3.0;
END_FUNCTION
```

## Functions Without Inputs

A function may have no inputs, though this is uncommon:

```st
FUNCTION GetTimestamp : LINT
  GetTimestamp := ReadSystemClock();
END_FUNCTION
```

## Recursive Functions

Functions may call themselves recursively, but be cautious of stack depth in
resource-constrained PLC environments:

```st
FUNCTION Factorial : LINT
  VAR_INPUT
    n : LINT;
  END_VAR

  IF n <= 1 THEN
    Factorial := 1;
  ELSE
    Factorial := n * Factorial(n := n - 1);
  END_IF;
END_FUNCTION
```

## Practical Examples

### Linear Interpolation

```st
FUNCTION Lerp : REAL
  VAR_INPUT
    a : REAL;  // start value
    b : REAL;  // end value
    t : REAL;  // 0.0 to 1.0
  END_VAR

  Lerp := a + (b - a) * t;
END_FUNCTION
```

### Scaling an Analog Input

A common PLC task is converting a raw ADC count to engineering units:

```st
FUNCTION ScaleAnalog : REAL
  VAR_INPUT
    raw      : INT;    // 0..32767
    eng_low  : REAL;   // engineering low  (e.g., 0.0 PSI)
    eng_high : REAL;   // engineering high (e.g., 100.0 PSI)
  END_VAR
  VAR
    fraction : REAL;
  END_VAR

  fraction := INT_TO_REAL(raw) / 32767.0;
  ScaleAnalog := eng_low + fraction * (eng_high - eng_low);
END_FUNCTION

PROGRAM Main
  VAR
    raw_pressure : INT := 16384;
    pressure_psi : REAL;
  END_VAR

  pressure_psi := ScaleAnalog(
    raw := raw_pressure,
    eng_low := 0.0,
    eng_high := 100.0
  );
  // pressure_psi ~ 50.0
END_PROGRAM
```

### Bitfield Check

```st
FUNCTION IsBitSet : BOOL
  VAR_INPUT
    value : DWORD;
    bit   : INT;
  END_VAR

  IsBitSet := (value AND SHL(DWORD#1, bit)) <> 0;
END_FUNCTION
```

## Restrictions

Standard IEC 61131-3 places the following restrictions on functions:

- Functions must not instantiate function blocks.
- Functions must not write to global variables.
- Functions must not have side effects.

These restrictions ensure that a function's output depends only on its inputs, making
programs easier to reason about. This compiler may relax some of these constraints, but
adhering to them is strongly recommended for portable, maintainable code.
