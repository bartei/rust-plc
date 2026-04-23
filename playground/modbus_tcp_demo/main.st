(*
 * Modbus TCP demo
 *
 * Connects to a Modbus TCP device over Ethernet and performs I/O.
 * Unlike RTU, no separate SerialLink is needed — the device FB
 * manages its own TCP connection directly.
 *
 * Run:
 *   cargo run -p st-cli -- run playground/modbus_tcp_demo -n 10
 *)

VAR_GLOBAL
    g_connected : BOOL := FALSE;
    g_ai1       : UINT := 0;
    g_ai2       : UINT := 0;
    g_cycles    : INT := 0;
    i : INT;
END_VAR

PROGRAM Main
VAR
    io : Waveshare8RelayOutput;
    // flags : ARRAY[0..7] OF BOOL;
END_VAR
    g_cycles := g_cycles + 1;

    (* Connect to a Modbus TCP device *)
    io(
        host := '10.1.2.133',
        port := 502,
        unit_id := 1,
        refresh_rate := T#50ms
    );
    CASE (g_cycles MOD 8) OF
        0: 
            io.DO0 := TRUE;
            io.DO1 := FALSE;
            io.DO2 := FALSE;
            io.DO3 := FALSE;
            io.DO4 := FALSE;
            io.DO5 := FALSE;
            io.DO6 := FALSE;
            io.DO7 := FALSE;
        1: 
            io.DO0 := FALSE;
            io.DO1 := TRUE;
            io.DO2 := FALSE;
            io.DO3 := FALSE;
            io.DO4 := FALSE;
            io.DO5 := FALSE;
            io.DO6 := FALSE;
            io.DO7 := FALSE;
        2: 
            io.DO0 := FALSE;
            io.DO1 := FALSE;
            io.DO2 := TRUE;
            io.DO3 := FALSE;
            io.DO4 := FALSE;
            io.DO5 := FALSE;
            io.DO6 := FALSE;
            io.DO7 := FALSE;
        3: 
            io.DO0 := FALSE;
            io.DO1 := FALSE;
            io.DO2 := FALSE;
            io.DO3 := TRUE;
            io.DO4 := FALSE;
            io.DO5 := FALSE;
            io.DO6 := FALSE;
            io.DO7 := FALSE;
        4: 
            io.DO0 := FALSE;
            io.DO1 := FALSE;
            io.DO2 := FALSE;
            io.DO3 := FALSE;
            io.DO4 := TRUE;
            io.DO5 := FALSE;
            io.DO6 := FALSE;
            io.DO7 := FALSE;
        5: 
            io.DO0 := FALSE;
            io.DO1 := FALSE;
            io.DO2 := FALSE;
            io.DO3 := FALSE;
            io.DO4 := FALSE;
            io.DO5 := TRUE;
            io.DO6 := FALSE;
            io.DO7 := FALSE;
        6: 
            io.DO0 := FALSE;
            io.DO1 := FALSE;
            io.DO2 := FALSE;
            io.DO3 := FALSE;
            io.DO4 := FALSE;
            io.DO5 := FALSE;
            io.DO6 := TRUE;
            io.DO7 := FALSE;
        7: 
            io.DO0 := TRUE;
            io.DO1 := TRUE;
            io.DO2 := TRUE;
            io.DO3 := TRUE;
            io.DO4 := TRUE;
            io.DO5 := TRUE;
            io.DO6 := TRUE;
            io.DO7 := TRUE;
            
    END_CASE;

    (* Copy device state to globals for monitoring *)
    g_connected := io.connected;
END_PROGRAM
