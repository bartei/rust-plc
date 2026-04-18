VAR_GLOBAL
    source_val   : INT := 42;      (* initial value the client reads *)
    written_val  : INT := 0;       (* client writes here via OPC-UA *)
    echo_val     : INT := 0;       (* PLC copies written_val here *)
    bool_source  : BOOL := TRUE;
    bool_written : BOOL := FALSE;
    bool_echo    : BOOL := FALSE;
    real_source  : REAL := 3.14;
    real_written : REAL := 0.0;
    real_echo    : REAL := 0.0;
END_VAR

PROGRAM Main
    (* Echo: copy written values to echo variables *)
    echo_val    := written_val;
    bool_echo   := bool_written;
    real_echo   := real_written;
END_PROGRAM
