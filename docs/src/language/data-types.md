# Data Types

IEC 61131-3 defines a rich set of elementary data types designed for industrial automation.
This chapter covers every supported type, the type hierarchy, and the literal formats used
to write constant values in code.

## Boolean

| Type   | Size   | Values         |
|--------|--------|----------------|
| `BOOL` | 1 bit  | `TRUE`, `FALSE`|

```st
VAR
  motor_on : BOOL := TRUE;
  fault    : BOOL := FALSE;
END_VAR
```

## Integer Types

Signed and unsigned integers come in four widths:

| Signed  | Unsigned | Size    | Range                                      |
|---------|----------|---------|--------------------------------------------|
| `SINT`  | `USINT`  | 8-bit   | -128..127 / 0..255                         |
| `INT`   | `UINT`   | 16-bit  | -32768..32767 / 0..65535                   |
| `DINT`  | `UDINT`  | 32-bit  | -2^31..2^31-1 / 0..2^32-1                 |
| `LINT`  | `ULINT`  | 64-bit  | -2^63..2^63-1 / 0..2^64-1                 |

```st
VAR
  temperature : INT  := -40;
  rpm         : UINT := 3600;
  big_count   : LINT := 1_000_000_000;
END_VAR
```

Underscores in numeric literals are allowed for readability.

## Floating-Point Types

| Type    | Size    | Precision          |
|---------|---------|--------------------|
| `REAL`  | 32-bit  | ~7 decimal digits  |
| `LREAL` | 64-bit  | ~15 decimal digits |

```st
VAR
  setpoint : REAL  := 72.5;
  pi       : LREAL := 3.14159265358979;
END_VAR
```

## Bit-String Types

These types are used for bitwise operations and direct bit access:

| Type    | Size    |
|---------|---------|
| `BYTE`  | 8-bit   |
| `WORD`  | 16-bit  |
| `DWORD` | 32-bit  |
| `LWORD` | 64-bit  |

```st
VAR
  status_flags : WORD  := 16#00FF;
  mask         : DWORD := 2#11110000_11110000_11110000_11110000;
END_VAR
```

Bit-string types are not interchangeable with integers. Use explicit conversions when
mixing arithmetic and bitwise operations.

## Time and Duration Types

| Type   | Represents                  | Example literal               |
|--------|-----------------------------|-------------------------------|
| `TIME` | Duration                    | `T#5s`, `T#1h30m`            |
| `DATE` | Calendar date               | `D#2024-01-15`               |
| `TOD`  | Time of day                 | `TOD#14:30:00`               |
| `DT`   | Date and time combined      | `DT#2024-01-15-14:30:00`     |

```st
VAR
  cycle_time  : TIME := T#100ms;
  start_date  : DATE := D#2024-01-15;
  shift_start : TOD  := TOD#06:00:00;
  timestamp   : DT   := DT#2024-01-15-08:00:00;
END_VAR
```

> **Caveat:** The keyword `DT` is a reserved type name. If you name a variable `dt`, it
> will conflict with the `DT` keyword. Since keywords are case-insensitive, `dt`, `Dt`,
> and `DT` all refer to the type. Use descriptive names like `date_time` or `my_dt`
> instead.

### TIME literal components

A `TIME` literal begins with `T#` or `TIME#` followed by one or more components:

- `d` -- days
- `h` -- hours
- `m` -- minutes
- `s` -- seconds
- `ms` -- milliseconds
- `us` -- microseconds
- `ns` -- nanoseconds

```st
VAR
  short_delay : TIME := T#250ms;
  work_shift  : TIME := T#8h;
  precise     : TIME := T#1m30s500ms;
END_VAR
```

## String Types

| Type      | Encoding   | Default max length |
|-----------|------------|--------------------|
| `STRING`  | Single-byte| 80 characters      |
| `WSTRING` | UTF-16     | 80 characters      |

```st
VAR
  name    : STRING      := 'Hello, PLC!';
  label   : STRING[200] := 'Extended length string';
  unicode : WSTRING     := "Wide string literal";
END_VAR
```

Single-byte strings use single quotes. Wide strings use double quotes.

## Type Hierarchy

The IEC 61131-3 type system is organized hierarchically:

```
ANY
├── ANY_BIT
│   ├── BOOL
│   ├── BYTE
│   ├── WORD
│   ├── DWORD
│   └── LWORD
├── ANY_NUM
│   ├── ANY_INT
│   │   ├── ANY_SIGNED (SINT, INT, DINT, LINT)
│   │   └── ANY_UNSIGNED (USINT, UINT, UDINT, ULINT)
│   └── ANY_REAL (REAL, LREAL)
├── ANY_STRING (STRING, WSTRING)
├── ANY_DATE (DATE, TOD, DT)
└── ANY_DURATION (TIME)
```

The `ANY_*` groups are used in function signatures to accept a range of types. For
example, an `ADD` function accepts `ANY_NUM` inputs.

## Literal Formats

### Integer literals

| Base     | Prefix | Example        | Decimal value |
|----------|--------|----------------|---------------|
| Decimal  | (none) | `255`          | 255           |
| Hex      | `16#`  | `16#FF`        | 255           |
| Octal    | `8#`   | `8#77`         | 63            |
| Binary   | `2#`   | `2#1010`       | 10            |

```st
VAR
  dec_val : INT   := 100;
  hex_val : INT   := 16#64;
  oct_val : INT   := 8#144;
  bin_val : INT   := 2#01100100;
END_VAR
// All four variables hold the value 100.
```

### Real literals

Real numbers use a decimal point and optional exponent:

```st
VAR
  a : REAL := 3.14;
  b : REAL := 3.14e2;    // 314.0
  c : REAL := 1.0E-3;    // 0.001
  d : REAL := -2.5e+10;
END_VAR
```

### Typed literals

A typed literal forces a specific type on a constant value:

```st
VAR
  x : INT  := INT#42;
  y : REAL := REAL#3.14;
  z : BYTE := BYTE#16#FF;
  w : BOOL := BOOL#1;
END_VAR
```

This is especially useful in function calls where overload resolution needs a hint, or
when assigning to a generic (`ANY_NUM`) parameter.

## Implicit Conversions

Narrowing conversions (e.g., `DINT` to `INT`) are not implicit and require an explicit
conversion function like `DINT_TO_INT()`. Widening conversions (e.g., `INT` to `DINT`) are
generally safe and may be performed implicitly by the compiler.

```st
VAR
  small : INT  := 42;
  big   : DINT;
  back  : INT;
END_VAR

big  := small;              // OK: widening, implicit
back := DINT_TO_INT(big);   // Required: narrowing, explicit
```
