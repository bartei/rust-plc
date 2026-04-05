(*
 * Full Demo — Comprehensive example showcasing all language features.
 *
 * This program simulates a simple bottle-filling station:
 * - Bottles arrive on a conveyor
 * - A sensor detects each bottle
 * - A valve fills the bottle for a set number of cycles
 * - A counter tracks total bottles filled
 *
 * Run with: st-cli run playground/06_full_demo.st -n 100
 *)

// ── Type Definitions ─────────────────────────────────────────────────
TYPE
    MachineState : (Idle, Filling, Complete);

    ProductionStats : STRUCT
        total_filled : INT := 0;
        total_rejected : INT := 0;
        fill_cycles : INT := 0;
    END_STRUCT;
END_TYPE

// ── Utility Functions ────────────────────────────────────────────────
FUNCTION Clamp : INT
VAR_INPUT
    value : INT;
    min_val : INT;
    max_val : INT;
END_VAR
    IF value < min_val THEN
        Clamp := min_val;
    ELSIF value > max_val THEN
        Clamp := max_val;
    ELSE
        Clamp := value;
    END_IF;
END_FUNCTION

FUNCTION IsInRange : BOOL
VAR_INPUT
    value : INT;
    low : INT;
    high : INT;
END_VAR
    IsInRange := (value >= low) AND (value <= high);
END_FUNCTION

// ── Fill Controller Function Block ──────────────────────────────────
FUNCTION_BLOCK FillController
VAR_INPUT
    start_fill : BOOL;
    target_cycles : INT;
END_VAR
VAR_OUTPUT
    valve_open : BOOL;
    fill_done : BOOL;
    cycle_count : INT;
END_VAR
VAR
    filling : BOOL := FALSE;
    counter : INT := 0;
END_VAR
    IF start_fill AND NOT filling THEN
        // Start a new fill
        filling := TRUE;
        counter := 0;
        valve_open := TRUE;
        fill_done := FALSE;
    END_IF;

    IF filling THEN
        counter := counter + 1;
        cycle_count := counter;

        IF counter >= target_cycles THEN
            // Fill complete
            filling := FALSE;
            valve_open := FALSE;
            fill_done := TRUE;
        END_IF;
    END_IF;
END_FUNCTION_BLOCK

// ── Global Production Counter ────────────────────────────────────────
VAR_GLOBAL
    g_production_total : INT;
END_VAR

// ── Main Program ─────────────────────────────────────────────────────
PROGRAM BottleFiller
VAR
    state : INT := 0;
    filler : FillController;
    stats : ProductionStats;

    // Simulated inputs
    bottle_present : BOOL := FALSE;
    cycle_number : INT := 0;

    // Outputs
    conveyor_run : BOOL := TRUE;
    fill_valve : BOOL := FALSE;
    reject_gate : BOOL := FALSE;

    // Configuration
    fill_time : INT := 10;
    max_bottles : INT := 50;

    // Temporaries
    clamped_time : INT := 0;
    in_range : BOOL := FALSE;
END_VAR
    cycle_number := cycle_number + 1;

    // Simulate a bottle arriving every 15 cycles
    bottle_present := (cycle_number MOD 15) = 0;

    // Clamp fill time to valid range
    clamped_time := Clamp(value := fill_time, min_val := 5, max_val := 30);

    // State machine
    CASE state OF
        0: // IDLE — waiting for bottle
            conveyor_run := TRUE;
            fill_valve := FALSE;
            reject_gate := FALSE;

            IF bottle_present THEN
                state := 1;
                conveyor_run := FALSE;
            END_IF;

        1: // FILLING
            filler(start_fill := TRUE, target_cycles := clamped_time);
            fill_valve := filler.valve_open;

            IF filler.fill_done THEN
                stats.total_filled := stats.total_filled + 1;
                stats.fill_cycles := stats.fill_cycles + filler.cycle_count;
                g_production_total := g_production_total + 1;

                // Check if we should reject (every 7th bottle for demo)
                in_range := IsInRange(
                    value := stats.total_filled,
                    low := 1,
                    high := max_bottles
                );

                IF (stats.total_filled MOD 7) = 0 THEN
                    state := 2; // reject
                ELSE
                    state := 0; // back to idle
                END_IF;

                // Reset filler for next use
                filler(start_fill := FALSE, target_cycles := 0);
            END_IF;

        2: // REJECTING
            reject_gate := TRUE;
            stats.total_rejected := stats.total_rejected + 1;
            state := 0;
    END_CASE;

    // Safety: stop after max bottles
    IF stats.total_filled >= max_bottles THEN
        conveyor_run := FALSE;
        fill_valve := FALSE;
        state := 0;
    END_IF;
END_PROGRAM
