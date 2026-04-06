(*
 * Conveyor controller — manages belt movement.
 * Uses stdlib timers with TIME values.
 *)

FUNCTION_BLOCK ConveyorController
VAR_INPUT
    run_cmd : BOOL;
    speed_setpoint : INT;
END_VAR
VAR_OUTPUT
    motor_running : BOOL;
    current_speed : INT;
END_VAR
VAR
    ramp_timer : TON;
    ramp_step : INT := 0;
END_VAR
    IF run_cmd THEN
        motor_running := TRUE;
        // Ramp up speed gradually
        ramp_timer(IN1 := current_speed < speed_setpoint, PT := T#100ms);
        IF ramp_timer.Q THEN
            IF current_speed < speed_setpoint THEN
                current_speed := current_speed + 1;
            END_IF;
        END_IF;
    ELSE
        motor_running := FALSE;
        current_speed := 0;
    END_IF;
END_FUNCTION_BLOCK
