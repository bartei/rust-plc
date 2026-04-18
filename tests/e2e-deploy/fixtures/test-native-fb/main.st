(* Native Function Block E2E test program.
   Declares a device FB instance, calls it each cycle, and
   copies values between fields. The cycle counter in a global
   variable confirms the program is running. *)

VAR_GLOBAL
    g_cycle : INT := 0;
    g_flag  : BOOL := FALSE;
END_VAR

PROGRAM Main
VAR
    io : SimpleIO;
END_VAR
    g_cycle := g_cycle + 1;

    (* Call the device FB — triggers native execute on host,
       no-op on deployed agent without runtime registry *)
    io(refresh_rate := T#10ms);

    (* Set outputs based on cycle count *)
    io.OUTPUT_1 := g_cycle * 10;
    io.FLAG := g_cycle > 5;
    g_flag := io.FLAG;
END_PROGRAM
