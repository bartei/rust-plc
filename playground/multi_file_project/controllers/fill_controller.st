(*
 * Fill controller — manages the bottle filling process.
 * Uses CTU from stdlib and types from types/process_data.st.
 *)

FUNCTION_BLOCK FillController
VAR_INPUT
    start : BOOL;
    target_fill : INT;
END_VAR
VAR_OUTPUT
    valve_open : BOOL;
    fill_done : BOOL;
    fill_count : INT;
END_VAR
VAR
    counter : CTU;
    filling : BOOL := FALSE;
    edge : R_TRIG;
END_VAR
    edge(CLK := start);

    IF edge.Q AND NOT filling THEN
        filling := TRUE;
        fill_count := 0;
    END_IF;

    IF filling THEN
        valve_open := TRUE;
        counter(CU := TRUE, RESET := NOT filling, PV := target_fill);
        fill_count := counter.CV;

        IF counter.Q THEN
            filling := FALSE;
            valve_open := FALSE;
            fill_done := TRUE;
        END_IF;
    ELSE
        valve_open := FALSE;
    END_IF;
END_FUNCTION_BLOCK
