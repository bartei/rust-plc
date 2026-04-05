# Expressions

Expressions in Structured Text combine variables, literals, operators, and function calls
to produce values. This chapter covers operator precedence, grouping with parentheses, and
using function calls within expressions.

## Operator Precedence

Operators are listed from highest to lowest precedence. Operators on the same row have
equal precedence and are evaluated left-to-right.

| Precedence | Operator(s)            | Description                 |
|------------|------------------------|-----------------------------|
| 1 (highest)| `**`                   | Exponentiation              |
| 2          | `-` (unary), `NOT`     | Negation, bitwise/logical NOT|
| 3          | `*`, `/`, `MOD`        | Multiplication, division, modulo |
| 4          | `+`, `-`               | Addition, subtraction       |
| 5          | `<`, `>`, `<=`, `>=`   | Relational comparisons      |
| 6          | `=`, `<>`              | Equality, inequality        |
| 7          | `AND`, `&`             | Logical/bitwise AND         |
| 8          | `XOR`                  | Logical/bitwise XOR         |
| 9 (lowest) | `OR`                   | Logical/bitwise OR          |

When in doubt, use parentheses to make intent explicit.

## Arithmetic Operators

Standard arithmetic works on numeric types (`ANY_NUM`):

```st
VAR
  a, b, result : INT;
END_VAR

a := 10;
b := 3;

result := a + b;    // 13
result := a - b;    // 7
result := a * b;    // 30
result := a / b;    // 3 (integer division)
result := a MOD b;  // 1
```

### Exponentiation

The `**` operator raises the left operand to the power of the right operand:

```st
VAR
  x : REAL;
END_VAR

x := 2.0 ** 10.0;   // 1024.0
x := 3.0 ** 0.5;    // square root of 3
```

### Unary Negation

```st
VAR
  speed : REAL := 50.0;
  reverse_speed : REAL;
END_VAR

reverse_speed := -speed;  // -50.0
```

## Comparison Operators

Comparisons return `BOOL`:

```st
VAR
  temp    : REAL := 85.0;
  too_hot : BOOL;
  in_range: BOOL;
END_VAR

too_hot  := temp > 100.0;
in_range := (temp >= 60.0) AND (temp <= 100.0);
```

The full set:

| Operator | Meaning                |
|----------|------------------------|
| `=`      | Equal to               |
| `<>`     | Not equal to           |
| `<`      | Less than              |
| `>`      | Greater than           |
| `<=`     | Less than or equal     |
| `>=`     | Greater than or equal  |

Note that `=` is the equality comparison operator, not assignment. Assignment uses `:=`.

## Logical and Bitwise Operators

`AND`, `OR`, `XOR`, and `NOT` operate on `BOOL` values (logical) or bit-string types
(bitwise), depending on the operand types:

```st
VAR
  a, b   : BOOL;
  result : BOOL;
END_VAR

result := a AND b;
result := a OR b;
result := a XOR b;
result := NOT a;
```

The `&` symbol is an alternative for `AND`:

```st
result := a & b;  // same as a AND b
```

### Bitwise example

```st
VAR
  flags  : WORD := 16#FF00;
  mask   : WORD := 16#0F0F;
  masked : WORD;
END_VAR

masked := flags AND mask;  // 16#0F00
```

## Parentheses

Parentheses override the default precedence:

```st
VAR
  a : INT := 2;
  b : INT := 3;
  c : INT := 4;
  r : INT;
END_VAR

r := a + b * c;     // 14  (multiplication first)
r := (a + b) * c;   // 20  (addition first)
```

A practical example -- checking a sensor range with debounce:

```st
VAR
  pressure     : REAL;
  pump_active  : BOOL;
  override     : BOOL;
  should_run   : BOOL;
END_VAR

should_run := (pressure < 50.0 OR override) AND NOT pump_active;
```

Without parentheses this expression would bind differently due to `AND` having higher
precedence than `OR`. Always parenthesize mixed `AND`/`OR` expressions.

## Function Calls in Expressions

Functions that return a value can be used directly inside expressions:

```st
FUNCTION ABS_REAL : REAL
  VAR_INPUT
    x : REAL;
  END_VAR
  IF x < 0.0 THEN
    ABS_REAL := -x;
  ELSE
    ABS_REAL := x;
  END_IF;
END_FUNCTION

PROGRAM Main
  VAR
    error    : REAL := -3.5;
    clamped  : REAL;
  END_VAR

  // Function call as part of a larger expression
  clamped := ABS_REAL(x := error) * 2.0 + 1.0;
  // result: 8.0
END_PROGRAM
```

Nested function calls are also valid:

```st
VAR
  angle : REAL;
  dist  : REAL;
END_VAR

dist := SQRT(SIN(angle) ** 2.0 + COS(angle) ** 2.0);
// Always 1.0, but demonstrates nesting
```

## Precedence in Practice

Consider a PID error calculation:

```st
VAR
  setpoint     : REAL := 100.0;
  measured     : REAL := 95.0;
  deadband     : REAL := 2.0;
  error        : REAL;
  needs_action : BOOL;
END_VAR

error := setpoint - measured;
needs_action := error > deadband OR error < -deadband;
```

Because `>` and `<` bind tighter than `OR`, this evaluates as:

```
(error > deadband) OR (error < -deadband)
```

which is the intended behavior. Still, explicit parentheses make the intent clearer for
anyone reading the code.

## Common Pitfalls

**Confusing `=` and `:=`.** The single `=` is comparison, not assignment. Writing
`x = 5` in an `IF` condition compares; writing `x := 5` assigns.

**Integer division truncates.** `7 / 2` yields `3`, not `3.5`. Use `REAL` operands for
fractional results: `7.0 / 2.0` yields `3.5`.

**MOD with negative operands.** The sign of the result follows the dividend:
`-7 MOD 3` yields `-1`.

**NOT precedence.** `NOT a AND b` means `(NOT a) AND b`, not `NOT (a AND b)`. Parenthesize
when in doubt.
