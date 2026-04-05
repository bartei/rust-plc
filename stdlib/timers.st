(*
 * IEC 61131-3 Standard Library: Timers
 *
 * TON  — On-delay timer (output goes TRUE after preset time while input is TRUE)
 * TOF  — Off-delay timer (output stays TRUE for preset time after input goes FALSE)
 * TP   — Pulse timer (output goes TRUE for preset time on rising edge)
 *
 * Note: These timers count scan cycles, not real time. The preset value (PT)
 * represents the number of scan cycles to delay. For real-time behavior,
 * set PT based on your scan cycle time:
 *   PT := desired_ms / cycle_time_ms
 *)

FUNCTION_BLOCK TON
VAR_INPUT
    IN1  : BOOL;   (* Timer input — named IN1 to avoid keyword conflict *)
    PT   : INT;    (* Preset time in scan cycles *)
END_VAR
VAR_OUTPUT
    Q    : BOOL;   (* Timer output *)
    ET   : INT;    (* Elapsed time in scan cycles *)
END_VAR
VAR
    running : BOOL := FALSE;
END_VAR
    IF IN1 THEN
        IF NOT running THEN
            running := TRUE;
            ET := 0;
        END_IF;
        IF running AND ET < PT THEN
            ET := ET + 1;
        END_IF;
        Q := ET >= PT;
    ELSE
        running := FALSE;
        ET := 0;
        Q := FALSE;
    END_IF;
END_FUNCTION_BLOCK

FUNCTION_BLOCK TOF
VAR_INPUT
    IN1  : BOOL;   (* Timer input *)
    PT   : INT;    (* Preset time in scan cycles *)
END_VAR
VAR_OUTPUT
    Q    : BOOL;   (* Timer output *)
    ET   : INT;    (* Elapsed time in scan cycles *)
END_VAR
VAR
    timing : BOOL := FALSE;
END_VAR
    IF IN1 THEN
        Q := TRUE;
        ET := 0;
        timing := FALSE;
    ELSE
        IF Q AND NOT timing THEN
            timing := TRUE;
            ET := 0;
        END_IF;
        IF timing THEN
            ET := ET + 1;
            IF ET >= PT THEN
                Q := FALSE;
                timing := FALSE;
            END_IF;
        END_IF;
    END_IF;
END_FUNCTION_BLOCK

FUNCTION_BLOCK TP
VAR_INPUT
    IN1  : BOOL;   (* Trigger input *)
    PT   : INT;    (* Pulse duration in scan cycles *)
END_VAR
VAR_OUTPUT
    Q    : BOOL;   (* Pulse output *)
    ET   : INT;    (* Elapsed time in scan cycles *)
END_VAR
VAR
    pulsing  : BOOL := FALSE;
    prev_in  : BOOL := FALSE;
END_VAR
    IF IN1 AND NOT prev_in AND NOT pulsing THEN
        (* Rising edge — start pulse *)
        pulsing := TRUE;
        ET := 0;
        Q := TRUE;
    END_IF;

    IF pulsing THEN
        ET := ET + 1;
        IF ET >= PT THEN
            pulsing := FALSE;
            Q := FALSE;
        END_IF;
    END_IF;

    prev_in := IN1;
END_FUNCTION_BLOCK
