VAR_GLOBAL RETAIN
    g_retain_counter : REAL := 0.0;
    g_retain_flag : BOOL := FALSE;
END_VAR
VAR_GLOBAL PERSISTENT
    g_persistent_total : DINT := 0;
END_VAR
VAR_GLOBAL RETAIN PERSISTENT
    g_durable : INT := 0;
END_VAR

PROGRAM Main
VAR
    cycle_count : INT := 0;
END_VAR
    cycle_count := cycle_count + 1;
    g_retain_counter := g_retain_counter + 1.0;
    g_persistent_total := g_persistent_total + 1;
    g_durable := g_durable + 1;
    IF cycle_count > 0 THEN
        g_retain_flag := TRUE;
    END_IF;
END_PROGRAM
