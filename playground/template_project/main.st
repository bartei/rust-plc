(*
 * Template PLC Program
 *
 * A starting point for new IEC 61131-3 Structured Text projects.
 * This program demonstrates:
 *   - Variable declarations with types and initial values
 *   - Scan cycle counter
 *   - Digital I/O passthrough (using simulated devices)
 *   - VFD speed control
 *   - Basic logic patterns
 *
 * Run with:
 *   st-cli run playground/template_project -n 100
 *
 * Debug in VS Code:
 *   F5 with launch.json pointing to this project
 *
 * Deploy to a target:
 *   st-cli bundle playground/template_project
 *   curl -X POST -F "file=@MyProject.st-bundle" http://target:4840/api/v1/program/upload
 *)

PROGRAM Main
VAR
    (* Scan cycle counter — wraps at SINT max (127 → -128) *)
    cycle : SINT := 0;

    (* Application state *)
    motor_running : BOOL := FALSE;
    speed_setpoint : REAL := 0.0;
END_VAR

    (* ── Cycle counter ──────────────────────────────────────────── *)
    cycle := cycle + 1;

    (* ── Digital I/O: DI_0 controls DO_0 directly ───────────────── *)
    io_rack_DO_0 := io_rack_DI_0;

    (* ── Logic: DI_1 AND DI_2 controls DO_1 ─────────────────────── *)
    io_rack_DO_1 := io_rack_DI_1 AND io_rack_DI_2;

    (* ── Analog: mirror AI_0 to AO_0 ────────────────────────────── *)
    io_rack_AO_0 := io_rack_AI_0;

    (* ── VFD start/stop ──────────────────────────────────────────── *)
    (* DI_6 = start request, DI_7 = stop request *)
    IF io_rack_DI_6 AND pump_vfd_READY AND NOT pump_vfd_FAULT THEN
        pump_vfd_RUN := TRUE;
        pump_vfd_STOP := FALSE;
        motor_running := TRUE;
    END_IF;

    IF io_rack_DI_7 THEN
        pump_vfd_RUN := FALSE;
        pump_vfd_STOP := TRUE;
        motor_running := FALSE;
    END_IF;

    (* ── Speed reference from analog input ───────────────────────── *)
    (* AI_3 range 0..10000 → speed 0..50.0 Hz *)
    speed_setpoint := INT_TO_REAL(IN1 := io_rack_AI_3) * 0.005;
    pump_vfd_SPEED_REF := speed_setpoint;

END_PROGRAM
