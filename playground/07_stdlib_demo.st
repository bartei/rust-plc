(*
 * Standard Library Demo
 *
 * Demonstrates the built-in standard library modules:
 * - CTU (count up), CTD (count down), CTUD (count up/down)
 * - R_TRIG (rising edge), F_TRIG (falling edge)
 * - TON (on-delay timer), TOF (off-delay timer), TP (pulse timer)
 * - Math: MAX_INT, MIN_INT, LIMIT_INT, ABS_INT, SEL
 * - Conversion: BOOL_TO_INT, INT_TO_BOOL
 *
 * Run with: st-cli run playground/07_stdlib_demo.st -n 50
 *)

VAR_GLOBAL
    g_bottle_count : INT;
    g_timer_done : INT;
    g_edge_count : INT;
END_VAR

PROGRAM StdlibDemo
VAR
    // ── Counters ──
    bottle_counter : CTU;       // Counts bottles on conveyor
    reject_counter : CTD;       // Counts down rejects remaining

    // ── Edge detection ──
    sensor_edge : R_TRIG;       // Detects new bottle arrival
    clear_edge : F_TRIG;        // Detects sensor clear

    // ── Timers ──
    fill_timer : TON;           // Delay before fill starts
    hold_timer : TP;            // Pulse output for gripper

    // ── Process variables ──
    cycle : INT := 0;
    sensor : BOOL := FALSE;
    filling : BOOL := FALSE;
    clamped_speed : INT := 0;
    edge_events : INT := 0;
END_VAR
    cycle := cycle + 1;

    // Simulate sensor: bottle arrives every 10 cycles, stays for 3 cycles
    sensor := (cycle MOD 10) >= 0 AND (cycle MOD 10) < 3;

    // ── Edge detection: count sensor transitions ──
    sensor_edge(CLK := sensor);
    clear_edge(CLK := sensor);
    IF sensor_edge.Q THEN
        edge_events := edge_events + 1;
    END_IF;

    // ── Counter: count bottles ──
    bottle_counter(CU := sensor_edge.Q, RESET := FALSE, PV := 100);

    // ── Timer: delay filling by 5 cycles after sensor ──
    fill_timer(IN1 := sensor, PT := 5);
    filling := fill_timer.Q;

    // ── Pulse: gripper activates for 3 cycles on each bottle ──
    hold_timer(IN1 := sensor_edge.Q, PT := 3);

    // ── Math: clamp conveyor speed ──
    clamped_speed := LIMIT_INT(MN := 10, IN1 := cycle, MX := 100);

    // ── Selection: choose mode based on counter ──
    g_bottle_count := bottle_counter.CV;
    g_timer_done := BOOL_TO_INT(IN1 := filling);
    g_edge_count := edge_events;
END_PROGRAM
