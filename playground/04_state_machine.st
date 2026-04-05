(*
 * Conveyor Belt State Machine — typical PLC application.
 * Demonstrates CASE, FOR, WHILE, REPEAT, EXIT, and multiple FBs.
 *)

FUNCTION_BLOCK Timer
VAR_INPUT
    enable : BOOL;
    preset : INT;  // ticks
END_VAR
VAR_OUTPUT
    done    : BOOL;
    elapsed : INT;
END_VAR
    IF NOT enable THEN
        elapsed := 0;
        done := FALSE;
        RETURN;
    END_IF;

    IF NOT done THEN
        elapsed := elapsed + 1;
        IF elapsed >= preset THEN
            done := TRUE;
        END_IF;
    END_IF;
END_FUNCTION_BLOCK

PROGRAM ConveyorControl
VAR
    state      : INT := 0;
    part_count : INT := 0;
    batch_size : INT := 10;
    error_code : INT := 0;

    start_btn  : BOOL := FALSE;
    stop_btn   : BOOL := FALSE;
    sensor_in  : BOOL := FALSE;
    sensor_out : BOOL := FALSE;

    motor_run  : BOOL := FALSE;
    alarm      : BOOL := FALSE;

    delay      : Timer;
    watchdog   : Timer;

    i          : INT;
    total      : INT := 0;
    parts      : ARRAY[1..100] OF INT;
END_VAR
    // Watchdog — detect stuck states
    watchdog(enable := state > 0 AND state < 99, preset := 5000);
    IF watchdog.done THEN
        state := 99;
        error_code := 1;
    END_IF;

    CASE state OF
        0: // IDLE
            motor_run := FALSE;
            alarm := FALSE;
            IF start_btn AND NOT stop_btn THEN
                state := 1;
                part_count := 0;
            END_IF;

        1: // RUNNING — wait for part at input sensor
            motor_run := TRUE;
            IF sensor_in THEN
                state := 2;
                delay(enable := FALSE, preset := 0);
            END_IF;

        2: // PART DETECTED — debounce delay
            delay(enable := TRUE, preset := 50);
            IF delay.done THEN
                part_count := part_count + 1;
                parts[part_count] := part_count;
                IF part_count >= batch_size THEN
                    state := 3;
                ELSE
                    state := 1;
                END_IF;
            END_IF;

        3: // BATCH COMPLETE — stop and report
            motor_run := FALSE;

            // Sum all part IDs (exercise FOR/WHILE/EXIT)
            total := 0;
            FOR i := 1 TO part_count DO
                total := total + parts[i];
                IF total > 10000 THEN
                    EXIT; // overflow guard
                END_IF;
            END_FOR;

            // Wait for operator acknowledgment
            WHILE NOT start_btn DO
                // Blink alarm
                alarm := NOT alarm;
                IF stop_btn THEN
                    state := 0;
                    EXIT;
                END_IF;
            END_WHILE;

            IF state = 3 THEN
                state := 0;
            END_IF;

        99: // FAULT
            motor_run := FALSE;
            alarm := TRUE;
            IF stop_btn THEN
                state := 0;
                error_code := 0;
            END_IF;
    END_CASE;
END_PROGRAM
