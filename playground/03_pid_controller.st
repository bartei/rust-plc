(*
 * PID Controller — realistic industrial control example.
 * Demonstrates FB design, struct field access, and function calls.
 *)

FUNCTION_BLOCK PID
VAR_INPUT
    setpoint      : REAL;
    process_value : REAL;
    kp            : REAL;
    ki            : REAL;
    kd            : REAL;
    cycle_time    : REAL;  // cycle time in seconds
    out_min       : REAL;
    out_max       : REAL;
    manual_mode   : BOOL;
    manual_output : REAL;
END_VAR
VAR_OUTPUT
    output   : REAL;
    error    : REAL;
    integral : REAL;
END_VAR
VAR
    prev_error : REAL := 0.0;
    derivative : REAL := 0.0;
    raw_output : REAL := 0.0;
END_VAR
    IF manual_mode THEN
        output := manual_output;
        integral := 0.0;
        prev_error := 0.0;
        RETURN;
    END_IF;

    error := setpoint - process_value;

    // Anti-windup: only integrate if output is not saturated
    IF raw_output > out_min AND raw_output < out_max THEN
        integral := integral + error * cycle_time;
    END_IF;

    derivative := (error - prev_error) / cycle_time;

    raw_output := kp * error + ki * integral + kd * derivative;

    // Clamp output
    IF raw_output < out_min THEN
        output := out_min;
    ELSIF raw_output > out_max THEN
        output := out_max;
    ELSE
        output := raw_output;
    END_IF;

    prev_error := error;
END_FUNCTION_BLOCK

// ── Main program using the PID ───────────────────────────────────────
PROGRAM TemperatureControl
VAR
    pid1          : PID;
    temp_sensor   : REAL := 22.0;
    temp_setpoint : REAL := 50.0;
    heater_output : REAL := 0.0;
    cycle_count   : DINT := 0;
END_VAR
    cycle_count := cycle_count + 1;

    // Simulate sensor noise
    temp_sensor := temp_sensor + heater_output * 0.01 - 0.005;

    pid1(
        setpoint      := temp_setpoint,
        process_value := temp_sensor,
        kp            := 2.0,
        ki            := 0.5,
        kd            := 0.1,
        cycle_time    := 0.1,
        out_min       := 0.0,
        out_max       := 100.0,
        manual_mode   := FALSE,
        manual_output := 0.0
    );

    heater_output := pid1.output;
END_PROGRAM
