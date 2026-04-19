(*
 * Native Function Block Demo
 *
 * Demonstrates the new native FB communication architecture.
 * Device types are auto-discovered from YAML profiles in the profiles/ directory.
 * Each profile becomes a callable function block type.
 *
 * Run:
 *   cargo run -p st-cli -- run playground/native_fb_demo -n 5
 *)

PROGRAM Main
VAR
    io : DemoIo;
    cycle : INT := 0;
    temperature_1 : Temperature;
END_VAR
    cycle := cycle + 1;

    (* Call the device FB — this triggers native execute() *)
    io(refresh_rate := T#10ms);

    (* Digital I/O passthrough *)
    io.DO_0 := io.DI_0;
    io.DO_1 := io.DI_1 AND io.DI_2;
    io.DO_1 := temperature_1.ALARM;
    
    (* Analog passthrough *)
    io.AO_0 := REAL_TO_INT(IN1 := temperature_1.TEMP);

END_PROGRAM
