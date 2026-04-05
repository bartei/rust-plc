(*
 * Hello World — Minimal ST program
 * Open this file to test diagnostics, hover, and go-to-definition.
 *)

PROGRAM HelloWorld
VAR
    counter : INT := 0;
    running : BOOL := TRUE;
END_VAR
    IF running THEN
        counter := counter + 1;
        IF counter > 1000 THEN
            running := FALSE;
        END_IF;
    END_IF;
END_PROGRAM
