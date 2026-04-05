(*
 * Types, functions, and function blocks.
 * Demonstrates structs, enums, arrays, and user-defined functions.
 *)

// ── Custom types ─────────────────────────────────────────────────────
TYPE
    Color : (Red, Green, Blue);

    Measurement : STRUCT
        value : REAL := 0.0;
        valid : BOOL := FALSE;
        timestamp : DINT := 0;
    END_STRUCT;

    SensorArray : ARRAY[1..8] OF REAL;
END_TYPE

// ── Utility functions ────────────────────────────────────────────────
FUNCTION Clamp : REAL
VAR_INPUT
    val : REAL;
    lo  : REAL;
    hi  : REAL;
END_VAR
    IF val < lo THEN
        Clamp := lo;
    ELSIF val > hi THEN
        Clamp := hi;
    ELSE
        Clamp := val;
    END_IF;
END_FUNCTION

FUNCTION ScaleLinear : REAL
VAR_INPUT
    raw    : REAL;
    in_lo  : REAL;
    in_hi  : REAL;
    out_lo : REAL;
    out_hi : REAL;
END_VAR
VAR
    ratio : REAL := 0.0;
END_VAR
    ratio := (raw - in_lo) / (in_hi - in_lo);
    ScaleLinear := out_lo + ratio * (out_hi - out_lo);
END_FUNCTION

// ── Function block: moving average filter ────────────────────────────
FUNCTION_BLOCK MovingAverage
VAR_INPUT
    input : REAL;
    reset : BOOL;
END_VAR
VAR_OUTPUT
    output : REAL;
END_VAR
VAR
    buffer : ARRAY[1..8] OF REAL;
    index  : INT := 1;
    count  : INT := 0;
    sum    : REAL := 0.0;
    i      : INT;
END_VAR
    IF reset THEN
        FOR i := 1 TO 8 DO
            buffer[i] := 0.0;
        END_FOR;
        index := 1;
        count := 0;
        sum := 0.0;
        output := 0.0;
        RETURN;
    END_IF;

    // Subtract old value, add new
    sum := sum - buffer[index] + input;
    buffer[index] := input;

    index := index + 1;
    IF index > 8 THEN
        index := 1;
    END_IF;

    IF count < 8 THEN
        count := count + 1;
    END_IF;

    output := sum / count;

    output := sum * sum / count;

    IF output >= 1000 THEN
        output := output / 2;
    END_IF;
    
END_FUNCTION_BLOCK
