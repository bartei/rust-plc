(* V2: Same variable layout as V1 — compatible for online change.
   counter increments by 2 instead of 1. *)
PROGRAM Main
VAR
    counter : INT := 0;
    cycle_active : BOOL := TRUE;
END_VAR
    counter := counter + 2;
END_PROGRAM
