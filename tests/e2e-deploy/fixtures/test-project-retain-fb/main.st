FUNCTION_BLOCK Counter
VAR_INPUT
    cu : BOOL;
END_VAR
VAR
    cv : INT := 0;
END_VAR
    IF cu THEN
        cv := cv + 1;
    END_IF;
END_FUNCTION_BLOCK

PROGRAM Main
VAR RETAIN
    fb : Counter;
END_VAR
VAR
    tick : BOOL := FALSE;
END_VAR
    tick := NOT tick;
    fb(cu := tick);
END_PROGRAM
