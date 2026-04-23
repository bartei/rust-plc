(*
 * Modbus TCP demo
 *
 * Connects to a Modbus TCP device over Ethernet and performs I/O.
 * Uses array fields: DO is ARRAY[0..7] OF BOOL defined with count: 8
 * in the device profile.
 *
 * Run:
 *   cargo run -p st-cli -- run playground/modbus_tcp_demo -n 10
 *)

VAR_GLOBAL
    g_connected : BOOL := FALSE;
    g_cycles    : INT := 0;
END_VAR

PROGRAM Main
VAR
    io : Waveshare8RelayOutput;
    i  : INT;
    active : INT;
END_VAR
    g_cycles := g_cycles + 1;

    (* Connect to a Modbus TCP device *)
    io(
        host := '10.1.2.133',
        port := 502,
        unit_id := 1,
        refresh_rate := T#50ms
    );

    (* Walking relay pattern using array access *)
    active := g_cycles MOD 8;
    FOR i := 0 TO 7 DO
        io.DO[i] := (i = active);
    END_FOR;

    g_connected := io.connected;
END_PROGRAM
