(*
 * IEC 61131-3 Standard Library: Timers
 *
 * TON  — On-delay timer: output goes TRUE after preset time while input is TRUE
 * TOF  — Off-delay timer: output stays TRUE for preset time after input goes FALSE
 * TP   — Pulse timer: output goes TRUE for preset time on rising edge
 *
 * All timers use TIME values (milliseconds). Use TIME literals for presets:
 *   timer(IN1 := enable, PT := T#5s);     (* 5 seconds *)
 *   timer(IN1 := enable, PT := T#500ms);  (* 500 milliseconds *)
 *   timer(IN1 := enable, PT := T#1m30s);  (* 1 minute 30 seconds *)
 *
 * Timers use SYSTEM_TIME() to track wall-clock elapsed time,
 * making them independent of scan cycle speed.
 *)

FUNCTION_BLOCK TON
VAR_INPUT
    IN1  : BOOL;   (* Timer input *)
    PT   : TIME;   (* Preset time *)
END_VAR
VAR_OUTPUT
    Q    : BOOL;   (* Timer output — TRUE after PT elapsed while IN1=TRUE *)
    ET   : TIME;   (* Elapsed time *)
END_VAR
VAR
    start_time : TIME := T#0ms;
    running : BOOL := FALSE;
END_VAR
    IF IN1 THEN
        IF NOT running THEN
            running := TRUE;
            start_time := SYSTEM_TIME();
        END_IF;
        ET := SYSTEM_TIME() - start_time;
        Q := ET >= PT;
    ELSE
        running := FALSE;
        ET := T#0ms;
        Q := FALSE;
    END_IF;
END_FUNCTION_BLOCK

FUNCTION_BLOCK TOF
VAR_INPUT
    IN1  : BOOL;   (* Timer input *)
    PT   : TIME;   (* Preset time *)
END_VAR
VAR_OUTPUT
    Q    : BOOL;   (* Timer output — stays TRUE for PT after IN1 goes FALSE *)
    ET   : TIME;   (* Elapsed time *)
END_VAR
VAR
    start_time : TIME := T#0ms;
    timing : BOOL := FALSE;
END_VAR
    IF IN1 THEN
        Q := TRUE;
        ET := T#0ms;
        timing := FALSE;
    ELSE
        IF Q AND NOT timing THEN
            timing := TRUE;
            start_time := SYSTEM_TIME();
        END_IF;
        IF timing THEN
            ET := SYSTEM_TIME() - start_time;
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
    PT   : TIME;   (* Pulse duration *)
END_VAR
VAR_OUTPUT
    Q    : BOOL;   (* Pulse output *)
    ET   : TIME;   (* Elapsed time *)
END_VAR
VAR
    start_time : TIME := T#0ms;
    pulsing  : BOOL := FALSE;
    prev_in  : BOOL := FALSE;
END_VAR
    IF IN1 AND NOT prev_in AND NOT pulsing THEN
        (* Rising edge — start pulse *)
        pulsing := TRUE;
        start_time := SYSTEM_TIME();
        Q := TRUE;
    END_IF;

    IF pulsing THEN
        ET := SYSTEM_TIME() - start_time;
        IF ET >= PT THEN
            pulsing := FALSE;
            Q := FALSE;
        END_IF;
    END_IF;

    prev_in := IN1;
END_FUNCTION_BLOCK
