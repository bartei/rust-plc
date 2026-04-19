(*
 * Simulated I/O playground
 *
 * Two simulated devices are used via native function blocks:
 *   - io_rack    : Sim8DI4AI4DO2AO  (8 DI / 4 AI / 4 DO / 2 AO)
 *   - pump_vfd   : SimVfd            (VFD with run/speed/feedback)
 *
 * Device profiles are auto-discovered from YAML files in the profiles/
 * directory. Each profile becomes a callable function block type.
 *
 * The CLI starts a web UI for each simulated device where you can
 * toggle inputs and watch outputs update in real time.
 *
 * Run with debugging from VS Code, or from the terminal:
 *   cargo run -p st-cli -- run playground/sim_project -n 0
 *)

PROGRAM Main
VAR
    cycle      : SINT := 0;
    io_rack    : Sim8DI4AI4DO2AO;
    pump_vfd   : SimVfd;
END_VAR

    cycle := cycle + 1;

    (* Call device FBs — triggers native execute() each scan *)
    io_rack(refresh_rate := T#10ms);
    pump_vfd(refresh_rate := T#10ms);

    (* --- Digital I/O passthrough on io_rack -------------------------- *)
    (* DI_0 controls DO_0 directly *)
    io_rack.DO_0 := io_rack.DI_0;

    (* DI_1 + DI_2 (AND) controls DO_1 *)
    io_rack.DO_1 := io_rack.DI_1 AND io_rack.DI_2;

    (* DI_3 OR DI_4 controls DO_2 *)
    io_rack.DO_2 := io_rack.DI_3 OR io_rack.DI_4;

    (* DO_3 blinks while DI_5 is held *)
    IF io_rack.DI_5 THEN
        io_rack.DO_3 := (cycle MOD 2) = 0;
    ELSE
        io_rack.DO_3 := FALSE;
    END_IF;

    (* --- Analog passthrough ------------------------------------------ *)
    (* AO_0 mirrors AI_0 *)
    io_rack.AO_0 := io_rack.AI_0;

    (* AO_1 = AI_1 + AI_2 *)
    io_rack.AO_1 := io_rack.AI_1 + io_rack.AI_2;

    (* --- VFD start/stop logic ---------------------------------------- *)
    (* DI_6 = start request, DI_7 = stop request *)
    IF io_rack.DI_6 AND pump_vfd.READY AND NOT pump_vfd.FAULT THEN
        pump_vfd.RUN := TRUE;
        pump_vfd.STOP := FALSE;
    END_IF;

    IF io_rack.DI_7 THEN
        pump_vfd.RUN := FALSE;
        pump_vfd.STOP := TRUE;
    END_IF;

    (* Speed reference tracks AI_3 (raw 0..10000 -> 0..50 Hz) *)
    pump_vfd.SPEED_REF := INT_TO_REAL(IN1 := io_rack.AI_3) * 0.005;

END_PROGRAM
