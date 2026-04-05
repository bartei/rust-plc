(*
 * Custom Module Example
 *
 * Shows how to create your own reusable function blocks,
 * following the same pattern as the standard library.
 *
 * To add custom modules to the standard library:
 * 1. Create a .st file in the stdlib/ directory
 * 2. Define your FUNCTION or FUNCTION_BLOCK
 * 3. They'll be automatically available in all programs
 *
 * Run with: st-cli run playground/08_custom_module.st -n 100
 *)

// ── Custom Function Block: Hysteresis Controller ─────────────────────
// Turns output ON when input exceeds high threshold,
// OFF when input drops below low threshold.
// Prevents oscillation around a single setpoint.

FUNCTION_BLOCK Hysteresis
VAR_INPUT
    input_val : INT;
    high_threshold : INT;
    low_threshold : INT;
END_VAR
VAR_OUTPUT
    output : BOOL;
END_VAR
    IF input_val >= high_threshold THEN
        output := TRUE;
    ELSIF input_val <= low_threshold THEN
        output := FALSE;
    END_IF;
    // Between thresholds: output holds previous state
END_FUNCTION_BLOCK

// ── Custom Function Block: Moving Average ────────────────────────────
// Computes a running average over the last N samples (simplified).

FUNCTION_BLOCK Averager
VAR_INPUT
    new_sample : INT;
    reset : BOOL;
END_VAR
VAR_OUTPUT
    average : INT;
END_VAR
VAR
    sum : INT := 0;
    count : INT := 0;
END_VAR
    IF reset THEN
        sum := 0;
        count := 0;
        average := 0;
    ELSE
        count := count + 1;
        sum := sum + new_sample;
        average := sum / count;
    END_IF;
END_FUNCTION_BLOCK

// ── Custom Function: Scale with Dead Band ────────────────────────────

FUNCTION ScaleWithDeadBand : INT
VAR_INPUT
    raw_value : INT;
    dead_band : INT;
    scale_factor : INT;
END_VAR
VAR
    effective : INT := 0;
END_VAR
    IF raw_value > dead_band THEN
        effective := raw_value - dead_band;
    ELSIF raw_value < -dead_band THEN
        effective := raw_value + dead_band;
    ELSE
        effective := 0;
    END_IF;
    ScaleWithDeadBand := effective * scale_factor;
END_FUNCTION

// ── Main Program ─────────────────────────────────────────────────────

VAR_GLOBAL
    g_heater : INT;
    g_average : INT;
    g_scaled : INT;
END_VAR

PROGRAM CustomModuleDemo
VAR
    temp_hyst : Hysteresis;
    temp_avg : Averager;
    cycle : INT := 0;
    simulated_temp : INT := 0;
END_VAR
    cycle := cycle + 1;

    // Simulate temperature: ramps up then down
    IF cycle <= 50 THEN
        simulated_temp := cycle * 2;
    ELSE
        simulated_temp := (100 - cycle) * 2;
    END_IF;

    // Hysteresis: heater ON above 60, OFF below 40
    temp_hyst(input_val := simulated_temp, high_threshold := 60, low_threshold := 40);
    g_heater := BOOL_TO_INT(IN1 := temp_hyst.output);

    // Running average
    temp_avg(new_sample := simulated_temp, reset := FALSE);
    g_average := temp_avg.average;

    // Scale with dead band
    g_scaled := ScaleWithDeadBand(raw_value := simulated_temp - 50, dead_band := 5, scale_factor := 2);
END_PROGRAM
