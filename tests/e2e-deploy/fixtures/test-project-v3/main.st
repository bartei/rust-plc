(* V3: Different variable layout — incompatible with V1/V2.
   Forces full restart instead of online change. *)
PROGRAM Main
VAR
    counter : INT := 0;
    cycle_active : BOOL := TRUE;
    new_sensor_value : REAL := 0.0;
    alarm_flag : BOOL := FALSE;
END_VAR
    counter := counter + 3;
    new_sensor_value := 42.0;
END_PROGRAM
