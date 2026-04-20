(*
 * Modbus RTU demo
 *
 * Connects to Modbus RTU slaves over a serial port and performs I/O.
 * Uses the two-layer model: SerialLink manages the transport,
 * device FBs use the link for protocol I/O.
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
    values : ARRAY[1..10] OF UINT;
    SupplyVoltage : UINT;
    SupplyVoltage2 : UINT;
END_VAR

PROGRAM Main
VAR
    serial : SerialLink;
    input  : WaveshareAnalogInput;
    output : WaveshareAnalogOutput;
END_VAR
    g_cycles := g_cycles + 1;

    (* Open the serial port — shared by all devices on this bus *)
    serial(
        port := '/dev/ttyACM0',
        baud := 9600,
        parity := 'N',
        data_bits := 8,
        stop_bits := 1
    );

    (* Connect to Modbus slaves via the serial link *)
    input(
        link := serial.port,
        slave_id := 20,
        refresh_rate := T#50ms
    );

    output(
        link := serial.port,
        slave_id := 21,
        refresh_rate := T#50ms
    );

    output.AO2 := input.AI1 * 10;
    SupplyVoltage := input.AI1;
    SupplyVoltage2 := input.AI2;

    (* Copy device state to globals for monitoring *)
    g_connected := input.connected;
END_PROGRAM
