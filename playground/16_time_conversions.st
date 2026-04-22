(*
 * Playground 16: Time & Type Conversion Functions
 *
 * Demonstrates all TIME_TO_*, *_TO_TIME, and generic TO_*/ANY_TO_*
 * conversion functions. TIME values are stored as milliseconds (i64).
 *
 * Run with: st-cli run playground/16_time_conversions.st
 *)

VAR_GLOBAL
    (* TIME_TO_* results *)
    g_time_to_int    : INT;
    g_time_to_dint   : INT;
    g_time_to_lint   : INT;
    g_time_to_sint   : INT;    (* SINT narrowing at VM level *)
    g_time_to_real   : REAL;
    g_time_to_lreal  : LREAL;
    g_time_to_bool_t : INT;   (* BOOL as INT for output *)
    g_time_to_bool_f : INT;

    (* *_TO_TIME results — stored as INT for easy verification *)
    g_int_to_time    : INT;
    g_dint_to_time   : INT;
    g_lint_to_time   : INT;
    g_real_to_time   : INT;
    g_bool_to_time_t : INT;
    g_bool_to_time_f : INT;

    (* Overloaded TO_* results *)
    g_to_int_from_time  : INT;
    g_to_real_from_time : REAL;
    g_to_bool_from_time : INT;
    g_to_time_from_int  : INT;

    (* ANY_TO_* results *)
    g_any_to_int_from_time  : INT;
    g_any_to_real_from_time : REAL;
    g_any_to_bool_from_time : INT;
    g_any_to_time_from_int  : INT;

    (* Round-trip verification *)
    g_roundtrip_int  : INT;    (* TIME -> INT -> TIME -> INT *)
    g_roundtrip_real : INT;    (* TIME -> REAL -> TIME -> INT *)

    (* Arithmetic with converted values *)
    g_time_sum : INT;
    g_time_diff : INT;

    (* Edge cases *)
    g_zero_time  : INT;
    g_large_time : DINT;
END_VAR

PROGRAM Main
VAR
    t5s       : TIME := T#5s;          (* 5000 ms *)
    t100ms    : TIME := T#100ms;       (* 100 ms *)
    t0        : TIME := T#0ms;         (* 0 ms *)
    t1d2h     : TIME := T#1d2h3m4s5ms; (* 93784005 ms *)
    ms_val    : INT;
    t_from_ms : TIME;
    t_sum     : TIME;
END_VAR
    (* ── TIME_TO_* ─────────────────────────────────────────────── *)
    g_time_to_int   := TIME_TO_INT(IN1 := t5s);         (* 5000 *)
    g_time_to_dint  := TIME_TO_DINT(IN1 := t5s);        (* 5000 *)
    g_time_to_lint  := TIME_TO_LINT(IN1 := t5s);        (* 5000 *)
    g_time_to_sint  := TIME_TO_SINT(IN1 := t100ms);     (* 100 *)
    g_time_to_real  := TIME_TO_REAL(IN1 := t5s);        (* 5000.0 *)
    g_time_to_lreal := TIME_TO_LREAL(IN1 := t5s);       (* 5000.0 *)
    g_time_to_bool_t := BOOL_TO_INT(IN1 := TIME_TO_BOOL(IN1 := t5s));   (* TRUE -> 1 *)
    g_time_to_bool_f := BOOL_TO_INT(IN1 := TIME_TO_BOOL(IN1 := t0));    (* FALSE -> 0 *)

    (* ── *_TO_TIME ─────────────────────────────────────────────── *)
    g_int_to_time    := TIME_TO_INT(IN1 := INT_TO_TIME(IN1 := 3000));    (* 3000 *)
    g_dint_to_time   := TIME_TO_INT(IN1 := DINT_TO_TIME(IN1 := 7500));   (* 7500 *)
    g_lint_to_time   := TIME_TO_INT(IN1 := LINT_TO_TIME(IN1 := 12000));  (* 12000 *)
    g_real_to_time   := TIME_TO_INT(IN1 := REAL_TO_TIME(IN1 := 2500.0)); (* 2500 *)
    g_bool_to_time_t := TIME_TO_INT(IN1 := BOOL_TO_TIME(IN1 := TRUE));   (* 1 *)
    g_bool_to_time_f := TIME_TO_INT(IN1 := BOOL_TO_TIME(IN1 := FALSE));  (* 0 *)

    (* ── Overloaded TO_* ───────────────────────────────────────── *)
    g_to_int_from_time  := TO_INT(IN1 := t100ms);           (* 100 *)
    g_to_real_from_time := TO_REAL(IN1 := t5s);              (* 5000.0 *)
    g_to_bool_from_time := BOOL_TO_INT(IN1 := TO_BOOL(IN1 := t5s));  (* 1 *)
    g_to_time_from_int  := TIME_TO_INT(IN1 := TO_TIME(IN1 := 4000)); (* 4000 *)

    (* ── ANY_TO_* ──────────────────────────────────────────────── *)
    g_any_to_int_from_time  := ANY_TO_INT(IN1 := t100ms);           (* 100 *)
    g_any_to_real_from_time := ANY_TO_REAL(IN1 := t5s);              (* 5000.0 *)
    g_any_to_bool_from_time := BOOL_TO_INT(IN1 := ANY_TO_BOOL(IN1 := t5s));  (* 1 *)
    g_any_to_time_from_int  := TIME_TO_INT(IN1 := ANY_TO_TIME(IN1 := 6000)); (* 6000 *)

    (* ── Round-trip verification ───────────────────────────────── *)
    (* TIME -> INT -> TIME -> INT should preserve value *)
    ms_val    := TIME_TO_INT(IN1 := t5s);                   (* 5000 *)
    t_from_ms := INT_TO_TIME(IN1 := ms_val);                (* T#5s *)
    g_roundtrip_int := TIME_TO_INT(IN1 := t_from_ms);       (* 5000 *)

    (* TIME -> REAL -> TIME -> INT should be close (float precision) *)
    g_roundtrip_real := TIME_TO_INT(IN1 := REAL_TO_TIME(IN1 := TIME_TO_REAL(IN1 := t5s))); (* 5000 *)

    (* ── Arithmetic with converted values ──────────────────────── *)
    t_sum := t5s + t100ms;
    g_time_sum := TIME_TO_INT(IN1 := t_sum);                (* 5100 *)
    g_time_diff := TIME_TO_INT(IN1 := t5s - t100ms);        (* 4900 *)

    (* ── Edge cases ────────────────────────────────────────────── *)
    g_zero_time := TIME_TO_INT(IN1 := t0);                   (* 0 *)
    g_large_time := TIME_TO_DINT(IN1 := t1d2h);              (* 93784005 *)
END_PROGRAM
