(*
 * IEC 61131-3 Standard Library: Type Conversions
 *
 * Note: The VM handles implicit widening coercions. These functions
 * provide explicit conversion for clarity and narrowing cases.
 *)

FUNCTION BOOL_TO_INT : INT
VAR_INPUT
    IN1 : BOOL;
END_VAR
    IF IN1 THEN
        BOOL_TO_INT := 1;
    ELSE
        BOOL_TO_INT := 0;
    END_IF;
END_FUNCTION

FUNCTION INT_TO_BOOL : BOOL
VAR_INPUT
    IN1 : INT;
END_VAR
    INT_TO_BOOL := IN1 <> 0;
END_FUNCTION
