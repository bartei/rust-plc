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
 *
 * Cross-reference / Call Hierarchy:
 *   Open this project in VS Code, then right-click any function name
 *   and select "Show Call Hierarchy" (or press Shift+Alt+H). You'll
 *   see both incoming (who calls this?) and outgoing (what does it call?)
 *   links. For example:
 *     - Right-click on "ClampSpeed" → incoming calls show Main
 *     - Right-click on "Main"       → outgoing calls show FillController,
 *       ConveyorController, ClampSpeed, BOOL_TO_INT
 *     - Right-click on "FillController" → incoming calls show Main
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
    filling : BOOL := FALSE;
    moving : BOOL := TRUE;
END_VAR
VAR PERSISTENT RETAIN
    target_fill_preset: INT := 50;
END_VAR

    cycle := cycle + 1;

    bottle_sensor := ((cycle MOD 200) = 0) AND conveyor.motor_running;

    IF bottle_sensor AND moving THEN
        filling := TRUE;
        moving := FALSE;
    END_IF;

    IF NOT bottle_sensor AND NOT filling THEN
        moving := TRUE;
    END_IF;

    // Run conveyor
    conveyor(run_cmd := moving, speed_setpoint := ClampSpeed(speed := 50, max_speed := 100));

    // Fill when bottle detected
    filler(start := bottle_sensor, target_fill := target_fill_preset);

    IF filler.fill_done AND filling THEN
        filling := FALSE;
        moving := TRUE;
        stats.bottles_filled := stats.bottles_filled + 1;
    END_IF;

    // Update stats
    stats.running := conveyor.motor_running;

    g_bottles_total := stats.bottles_filled;
    g_line_running := BOOL_TO_INT(IN1 := stats.running);
END_PROGRAM
