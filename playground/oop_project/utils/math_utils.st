(*
 * Utility functions used across the project.
 *)

FUNCTION Clamp : INT
VAR_INPUT
    val : INT;
    lo  : INT;
    hi  : INT;
END_VAR
    IF val < lo THEN
        Clamp := lo;
    ELSIF val > hi THEN
        Clamp := hi;
    ELSE
        Clamp := val;
    END_IF;
END_FUNCTION

FUNCTION ScalePercent : INT
VAR_INPUT
    raw     : INT;
    rawMax  : INT;
END_VAR
    IF rawMax > 0 THEN
        ScalePercent := (raw * 100) / rawMax;
    ELSE
        ScalePercent := 0;
    END_IF;
END_FUNCTION
