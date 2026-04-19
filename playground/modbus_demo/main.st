(*
 * Modbus RTU demo
 *
 * Connects to a Modbus RTU slave over a serial port and performs I/O.
 * The serial port path is configured as a parameter to the device FB.
 *
 * For testing without hardware, use socat to create a virtual serial pair
 * and run a Modbus slave simulator on the other end.
 *
 * Run:
 *   cargo run -p st-cli -- run playground/modbus_demo -n 10
 *)

VAR_GLOBAL
    g_connected : BOOL := FALSE;
    g_di0       : BOOL := FALSE;
    g_ai0       : INT := 0;
    g_cycles    : INT := 0;
END_VAR

PROGRAM Main
VAR
    io : TestModbusIO;
END_VAR
    g_cycles := g_cycles + 1;

    (* Connect to Modbus slave at address 1 on /dev/ttyUSB0 *)
    io(
        port := '/dev/ttyUSB0',
        baud := 9600,
        parity := 'N',
        data_bits := 8,
        stop_bits := 1,
        slave_id := 1,
        refresh_rate := T#50ms
    );

    (* Copy device state to globals for monitoring *)
    g_connected := io.connected;
    g_di0 := io.DI_0;
    g_ai0 := io.AI_0;

    (* Simple logic: mirror DI_0 to DO_0 *)
    io.DO_0 := io.DI_0;

    (* Copy analog input to output *)
    io.AO_0 := io.AI_0;

END_PROGRAM
