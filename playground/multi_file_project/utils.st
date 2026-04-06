(*
 * Utility functions shared across the project.
 *)

FUNCTION ClampSpeed : INT
VAR_INPUT
    speed : INT;
    max_speed : INT;
END_VAR
    IF speed > max_speed THEN
        ClampSpeed := max_speed;
    ELSIF speed < 0 THEN
        ClampSpeed := 0;
    ELSE
        ClampSpeed := speed;
    END_IF;
END_FUNCTION
