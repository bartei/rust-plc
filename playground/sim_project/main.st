(*
 * Simulated I/O playground
 *
 * Two simulated devices are wired by `plc-project.yaml`:
 *   - io_rack    : Sim8DI4AI4DO2AO  (8 DI / 4 AI / 4 DO / 2 AO)
 *   - pump_vfd   : SimVfd            (VFD with run/speed/feedback)
 *
 * Each profile field is exposed as a flat global named `{device}_{field}`.
 * The CLI starts a web UI on http://localhost:8080 (io_rack) and
 * http://localhost:8081 (pump_vfd) where you can toggle inputs and watch
 * outputs update in real time.
 *
 * Run with debugging from VS Code, or from the terminal:
 *   cargo run -p st-cli -- run playground/sim_project -n 0
 *)

PROGRAM Main
VAR
    cycle      : DINT := 0;
END_VAR

    cycle := cycle + 1;

    (* --- Digital I/O passthrough on io_rack -------------------------- *)
    (* DI_0 controls DO_0 directly *)
    io_rack_DO_0 := io_rack_DI_0;

    (* DI_1 + DI_2 (AND) controls DO_1 *)
    io_rack_DO_1 := io_rack_DI_1 AND io_rack_DI_2;

    (* DI_3 OR DI_4 controls DO_2 *)
    io_rack_DO_2 := io_rack_DI_3 OR io_rack_DI_4;

    (* DO_3 blinks while DI_5 is held *)
    IF io_rack_DI_5 THEN
        io_rack_DO_3 := (cycle MOD 2) = 0;
    ELSE
        io_rack_DO_3 := FALSE;
    END_IF;

    (* --- Analog passthrough ------------------------------------------ *)
    (* AO_0 mirrors AI_0 *)
    io_rack_AO_0 := io_rack_AI_0;

    (* AO_1 = AI_1 + AI_2 *)
    io_rack_AO_1 := io_rack_AI_1 + io_rack_AI_2;

    (* --- VFD start/stop logic ---------------------------------------- *)
    (* DI_6 = start request, DI_7 = stop request *)
    IF io_rack_DI_6 AND pump_vfd_READY AND NOT pump_vfd_FAULT THEN
        pump_vfd_RUN := TRUE;
        pump_vfd_STOP := FALSE;
    END_IF;

    IF io_rack_DI_7 THEN
        pump_vfd_RUN := FALSE;
        pump_vfd_STOP := TRUE;
    END_IF;

    (* Speed reference tracks AI_3 (raw 0..10000 → 0..50 Hz) *)
    pump_vfd_SPEED_REF := INT_TO_REAL(IN1 := io_rack_AI_3) * 0.005;

END_PROGRAM
