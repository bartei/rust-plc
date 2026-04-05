(*
 * IEC 61131-3 Standard Library: Math & Selection Functions
 *
 * These are pure ST implementations of standard math and selection functions.
 *)

// ── Selection functions ──────────────────────────────────────────────

FUNCTION SEL : INT
VAR_INPUT
    G   : BOOL;   (* Selector: FALSE=IN0, TRUE=IN1 *)
    IN0 : INT;    (* Value when G=FALSE *)
    IN1 : INT;    (* Value when G=TRUE *)
END_VAR
    IF G THEN
        SEL := IN1;
    ELSE
        SEL := IN0;
    END_IF;
END_FUNCTION

FUNCTION MAX_INT : INT
VAR_INPUT
    IN1 : INT;
    IN2 : INT;
END_VAR
    IF IN1 >= IN2 THEN
        MAX_INT := IN1;
    ELSE
        MAX_INT := IN2;
    END_IF;
END_FUNCTION

FUNCTION MIN_INT : INT
VAR_INPUT
    IN1 : INT;
    IN2 : INT;
END_VAR
    IF IN1 <= IN2 THEN
        MIN_INT := IN1;
    ELSE
        MIN_INT := IN2;
    END_IF;
END_FUNCTION

FUNCTION LIMIT_INT : INT
VAR_INPUT
    MN  : INT;    (* Minimum *)
    IN1 : INT;    (* Input value *)
    MX  : INT;    (* Maximum *)
END_VAR
    IF IN1 < MN THEN
        LIMIT_INT := MN;
    ELSIF IN1 > MX THEN
        LIMIT_INT := MX;
    ELSE
        LIMIT_INT := IN1;
    END_IF;
END_FUNCTION

FUNCTION ABS_INT : INT
VAR_INPUT
    IN1 : INT;
END_VAR
    IF IN1 < 0 THEN
        ABS_INT := -IN1;
    ELSE
        ABS_INT := IN1;
    END_IF;
END_FUNCTION

// ── REAL variants ────────────────────────────────────────────────────

FUNCTION MAX_REAL : REAL
VAR_INPUT
    IN1 : REAL;
    IN2 : REAL;
END_VAR
    IF IN1 >= IN2 THEN
        MAX_REAL := IN1;
    ELSE
        MAX_REAL := IN2;
    END_IF;
END_FUNCTION

FUNCTION MIN_REAL : REAL
VAR_INPUT
    IN1 : REAL;
    IN2 : REAL;
END_VAR
    IF IN1 <= IN2 THEN
        MIN_REAL := IN1;
    ELSE
        MIN_REAL := IN2;
    END_IF;
END_FUNCTION

FUNCTION LIMIT_REAL : REAL
VAR_INPUT
    MN  : REAL;
    IN1 : REAL;
    MX  : REAL;
END_VAR
    IF IN1 < MN THEN
        LIMIT_REAL := MN;
    ELSIF IN1 > MX THEN
        LIMIT_REAL := MX;
    ELSE
        LIMIT_REAL := IN1;
    END_IF;
END_FUNCTION

FUNCTION ABS_REAL : REAL
VAR_INPUT
    IN1 : REAL;
END_VAR
    IF IN1 < 0.0 THEN
        ABS_REAL := -IN1;
    ELSE
        ABS_REAL := IN1;
    END_IF;
END_FUNCTION
