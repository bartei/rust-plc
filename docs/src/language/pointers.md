# Pointers & References

IEC 61131-3 supports typed pointers via `REF_TO`, the `REF()` function, and the `^` dereference operator. Pointers allow indirect access to variables — reading or writing a variable through a reference rather than by name.

## Declaring a Pointer

Use `REF_TO <type>` to declare a pointer variable:

```st
VAR
    x    : INT := 42;
    ptr  : REF_TO INT;     (* pointer to an INT variable *)
    bptr : REF_TO BOOL;    (* pointer to a BOOL variable *)
    rptr : REF_TO REAL;    (* pointer to a REAL variable *)
END_VAR
```

A pointer variable holds the **address** of another variable of the specified type. When first declared, a pointer is `NULL` (points to nothing).

## Taking a Reference — `REF()`

Use the `REF()` function to get a pointer to a variable:

```st
VAR
    x   : INT := 42;
    ptr : REF_TO INT;
END_VAR
    ptr := REF(x);    (* ptr now points to x *)
```

`REF()` works with both local and global variables:

```st
VAR_GLOBAL
    g_sensor : REAL;
END_VAR

PROGRAM Main
VAR
    ptr : REF_TO REAL;
END_VAR
    ptr := REF(g_sensor);    (* pointer to a global variable *)
END_PROGRAM
```

## Dereferencing — `^`

The `^` operator reads or writes through a pointer:

### Reading through a pointer

```st
VAR
    x   : INT := 42;
    ptr : REF_TO INT;
    y   : INT;
END_VAR
    ptr := REF(x);
    y := ptr^;         (* y is now 42 — read x through ptr *)
```

### Writing through a pointer

```st
VAR
    x   : INT := 42;
    ptr : REF_TO INT;
END_VAR
    ptr := REF(x);
    ptr^ := 99;        (* x is now 99 — written through ptr *)
```

### Read-modify-write

```st
    ptr^ := ptr^ + 1;  (* increment x through the pointer *)
```

## NULL Pointer

`NULL` is a built-in literal representing an empty pointer:

```st
VAR
    ptr : REF_TO INT;
END_VAR
    ptr := NULL;        (* explicitly set to null *)
```

**Default value:** All `REF_TO` variables are initialized to `NULL` unless assigned.

**Safe dereference:** Dereferencing a `NULL` pointer returns the default value for the type (0 for INT, 0.0 for REAL, FALSE for BOOL). It does **not** crash the program.

```st
VAR
    ptr : REF_TO INT;
    x   : INT;
END_VAR
    x := ptr^;          (* x = 0, because ptr is NULL *)
```

## Passing Pointers to Functions

Pointers are especially useful for functions that need to modify their caller's variables:

### Swap function

```st
FUNCTION SwapInt : INT
VAR_INPUT
    a : REF_TO INT;
    b : REF_TO INT;
END_VAR
VAR
    temp : INT;
END_VAR
    temp := a^;
    a^ := b^;
    b^ := temp;
    SwapInt := 0;
END_FUNCTION

PROGRAM Main
VAR
    x : INT := 10;
    y : INT := 20;
    dummy : INT;
END_VAR
    dummy := SwapInt(a := REF(x), b := REF(y));
    (* x is now 20, y is now 10 *)
END_PROGRAM
```

### Increment via pointer

```st
FUNCTION IncrementBy : INT
VAR_INPUT
    target : REF_TO INT;
    amount : INT;
END_VAR
    target^ := target^ + amount;
    IncrementBy := target^;
END_FUNCTION
```

## Reassigning Pointers

A pointer can be reassigned to point to different variables:

```st
VAR
    a   : INT := 10;
    b   : INT := 20;
    ptr : REF_TO INT;
END_VAR
    ptr := REF(a);
    ptr^ := ptr^ + 5;    (* a is now 15 *)

    ptr := REF(b);
    ptr^ := ptr^ * 2;    (* b is now 40 *)
```

## Comparison with `VAR_IN_OUT`

Both pointers and `VAR_IN_OUT` allow indirect variable access:

| Feature | `VAR_IN_OUT` | `REF_TO` |
|---------|-------------|----------|
| Syntax | `VAR_IN_OUT x : INT; END_VAR` | `ptr : REF_TO INT;` |
| Assignment | Bound at call site | Can be reassigned at any time |
| NULL | Never null | Can be NULL |
| Flexibility | Fixed for the duration of the call | Can point to different variables |
| Use case | Function parameters | Dynamic data structures, callbacks |

## Restrictions

- **No pointer arithmetic** — you cannot add or subtract from a pointer (unlike C)
- **Type-safe** — `REF_TO INT` can only point to `INT` variables
- **No nested pointers** — `REF_TO REF_TO INT` is not supported
- **No pointer comparison** — comparing two pointers is not currently supported (use `NULL` comparison via `<>`)

## Example: Playground

See `playground/09_pointers.st` for a complete example demonstrating:
- Basic pointer read/write
- Swap function using pointers
- Increment via pointer
- NULL pointer safety

```bash
st-cli run playground/09_pointers.st -n 10
```
