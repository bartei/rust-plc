(*
 * IEC 61131-3 Standard Library: Edge Detection
 *
 * R_TRIG — Rising edge trigger
 * F_TRIG — Falling edge trigger
 *)

FUNCTION_BLOCK R_TRIG
VAR_INPUT
    CLK : BOOL;   (* Signal to monitor *)
END_VAR
VAR_OUTPUT
    Q   : BOOL;   (* TRUE for one cycle on rising edge *)
END_VAR
VAR
    prev : BOOL := FALSE;
END_VAR
    Q := CLK AND NOT prev;
    prev := CLK;
END_FUNCTION_BLOCK

FUNCTION_BLOCK F_TRIG
VAR_INPUT
    CLK : BOOL;   (* Signal to monitor *)
END_VAR
VAR_OUTPUT
    Q   : BOOL;   (* TRUE for one cycle on falling edge *)
END_VAR
VAR
    prev : BOOL := TRUE;
END_VAR
    Q := NOT CLK AND prev;
    prev := CLK;
END_FUNCTION_BLOCK
