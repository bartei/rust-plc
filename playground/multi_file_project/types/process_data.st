(*
 * Process data types shared across all controllers.
 * Demonstrates cross-file type declarations.
 *)

TYPE
    MachineState : (Idle, Running, Filling, Complete, Fault);

    ProcessData : STRUCT
        bottles_filled : INT := 0;
        bottles_rejected : INT := 0;
        line_speed : REAL := 0.0;
        running : BOOL := FALSE;
    END_STRUCT;
END_TYPE
