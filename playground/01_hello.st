(*
 * Hello World — Minimal ST program
 * Open this file to test diagnostics, hover, and go-to-definition.
 *)

PROGRAM HelloWorld
VAR
    counter : INT := 0;
    realCounter : REAL := 0;
    rad : REAL;
    running : BOOL := TRUE;
    seno : LREAL := 0;
END_VAR

    IF running THEN
        counter := counter + 1;
        realCounter := counter;
        rad := (realCounter/180.0) * 3.1415;
        seno := SIN(rad);
        IF counter > 1000 THEN
            running := FALSE;
        END_IF;
    END_IF;
END_PROGRAM
