(*
 * Main program — orchestrates the bottle filling line.
 *
 * This is a multi-file project example. It uses:
 * - Types from types/process_data.st
 * - Controllers from controllers/fill_controller.st and controllers/conveyor.st
 * - Utilities from utils.st
 * - Standard library: CTU, R_TRIG, TON
 *
 * Run with:
 *   cd playground/multi_file_project
 *   st-cli run                        # autodiscovers all .st files
 *   st-cli run -n 100                 # run 100 scan cycles
 *   st-cli check                      # check for errors
 *)

VAR_GLOBAL
    g_bottles_total : INT;
    g_line_running : INT;
END_VAR

PROGRAM Main
VAR
    filler : FillController;
    conveyor : ConveyorController;
    stats : ProcessData;

    cycle : INT := 0;
    bottle_sensor : BOOL := FALSE;
END_VAR
    cycle := cycle + 1;

    // Simulate bottle arriving every 20 cycles
    bottle_sensor := (cycle MOD 20) = 0;

    // Run conveyor
    conveyor(run_cmd := TRUE, speed_setpoint := ClampSpeed(speed := 50, max_speed := 100));

    // Fill when bottle detected
    filler(start := bottle_sensor, target_fill := 5);

    // Update stats
    IF filler.fill_done THEN
        stats.bottles_filled := stats.bottles_filled + 1;
    END_IF;
    stats.running := conveyor.motor_running;

    g_bottles_total := stats.bottles_filled;
    g_line_running := BOOL_TO_INT(IN1 := stats.running);
END_PROGRAM
