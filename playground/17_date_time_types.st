(*
 * Playground 17: DATE, TOD, DT Types & Conversions
 *
 * Demonstrates DATE/TOD/DT literal parsing, conversions to/from
 * numeric types, cross-type extraction (DT_TO_DATE, DT_TO_TOD),
 * date/time arithmetic, CONCAT_DATE_TOD, MULTIME, DIVTIME,
 * DAY_OF_WEEK, and TO_*/ANY_TO_* generic conversions.
 *
 * All date/time types are stored internally as milliseconds (i64).
 * DATE = ms since epoch, TOD = ms since midnight, DT = ms since epoch.
 * Use LINT for date/DT values as ms since epoch exceeds DINT range.
 *)

VAR_GLOBAL
    (* DATE literal parsing — LINT needed, values exceed DINT range *)
    g_date_ms       : LINT;   (* D#2024-01-15 → ms since epoch *)
    g_date_epoch    : LINT;   (* D#1970-01-01 → 0 *)

    (* TOD literal parsing — DINT is sufficient (max ~86M ms) *)
    g_tod_ms        : DINT;   (* TOD#12:30:00 → 45000000 ms *)
    g_tod_midnight  : DINT;   (* TOD#00:00:00 → 0 *)
    g_tod_frac      : DINT;   (* TOD#12:30:00.500 → 45000500 ms *)

    (* DT literal parsing *)
    g_dt_ms         : LINT;   (* DT#2024-01-15-12:30:00 *)

    (* DATE_TO_* *)
    g_date_to_int   : LINT;
    g_date_to_real  : REAL;
    g_date_to_bool  : INT;

    (* TOD_TO_* *)
    g_tod_to_int    : DINT;
    g_tod_to_real   : REAL;

    (* DT extraction *)
    g_dt_to_date    : LINT;   (* date portion *)
    g_dt_to_tod     : DINT;   (* tod portion *)

    (* CONCAT_DATE_TOD *)
    g_concat_dt     : LINT;

    (* Date/time arithmetic *)
    g_add_tod_time  : DINT;   (* TOD + TIME *)
    g_sub_tod_time  : DINT;   (* TOD - TIME *)
    g_sub_date_date : LINT;   (* DATE - DATE → TIME ms *)
    g_sub_dt_dt     : DINT;   (* DT - DT → TIME ms, small diff *)
    g_add_dt_time   : LINT;   (* DT + TIME *)

    (* MULTIME / DIVTIME *)
    g_multime       : DINT;
    g_divtime       : DINT;

    (* DAY_OF_WEEK *)
    g_dow_thu       : INT;    (* 1970-01-01 = Thursday = 4 *)
    g_dow_sun       : INT;    (* 1970-01-04 = Sunday = 0 *)
    g_dow_mon       : INT;    (* 2024-01-15 = Monday = 1 *)

    (* Generic TO_*/ANY_TO_* *)
    g_to_date       : LINT;
    g_any_to_tod    : DINT;
    g_any_to_dt     : LINT;

    (* Round-trip: DT → DATE + TOD → CONCAT → DT *)
    g_roundtrip_dt  : LINT;
END_VAR

PROGRAM Main
VAR
    d1    : DATE := D#2024-01-15;
    d2    : DATE := D#1970-01-01;
    d3    : DATE := D#1970-01-04;  (* Sunday *)
    t1    : TOD  := TOD#12:30:00;
    t2    : TOD  := TOD#00:00:00;
    t3    : TOD  := TOD#12:30:00.500;
    dt1   : DT   := DT#2024-01-15-12:30:00;
    dur   : TIME := T#1h;
    d_part : DATE;
    t_part : TOD;
END_VAR
    (* ── Literal parsing verification ──────────────────────────── *)
    g_date_ms     := DATE_TO_LINT(IN1 := d1);
    g_date_epoch  := DATE_TO_LINT(IN1 := d2);
    g_tod_ms      := TOD_TO_DINT(IN1 := t1);
    g_tod_midnight := TOD_TO_DINT(IN1 := t2);
    g_tod_frac    := TOD_TO_DINT(IN1 := t3);
    g_dt_ms       := DT_TO_LINT(IN1 := dt1);

    (* ── DATE_TO_* / TOD_TO_* ─────────────────────────────────── *)
    g_date_to_int  := DATE_TO_LINT(IN1 := d1);
    g_date_to_real := DATE_TO_REAL(IN1 := d1);
    g_date_to_bool := BOOL_TO_INT(IN1 := DATE_TO_BOOL(IN1 := d1));
    g_tod_to_int   := TOD_TO_DINT(IN1 := t1);
    g_tod_to_real  := TOD_TO_REAL(IN1 := t1);

    (* ── DT extraction ────────────────────────────────────────── *)
    g_dt_to_date := DATE_TO_LINT(IN1 := DT_TO_DATE(IN1 := dt1));
    g_dt_to_tod  := TOD_TO_DINT(IN1 := DT_TO_TOD(IN1 := dt1));

    (* ── CONCAT_DATE_TOD ──────────────────────────────────────── *)
    g_concat_dt := DT_TO_LINT(IN1 := CONCAT_DATE_TOD(IN1 := d1, IN2 := t1));

    (* ── Date/time arithmetic ─────────────────────────────────── *)
    g_add_tod_time  := TOD_TO_DINT(IN1 := ADD_TOD_TIME(IN1 := t1, IN2 := dur));
    g_sub_tod_time  := TOD_TO_DINT(IN1 := SUB_TOD_TIME(IN1 := t1, IN2 := dur));
    g_sub_date_date := TIME_TO_LINT(IN1 := SUB_DATE_DATE(IN1 := d1, IN2 := d2));
    g_sub_dt_dt     := TIME_TO_DINT(IN1 := SUB_DT_DT(IN1 := dt1, IN2 :=
                            CONCAT_DATE_TOD(IN1 := d1, IN2 := t2)));
    g_add_dt_time   := DT_TO_LINT(IN1 := ADD_DT_TIME(IN1 := dt1, IN2 := dur));

    (* ── MULTIME / DIVTIME ────────────────────────────────────── *)
    g_multime := TIME_TO_DINT(IN1 := MULTIME(IN1 := T#1s, IN2 := 5));
    g_divtime := TIME_TO_DINT(IN1 := DIVTIME(IN1 := T#10s, IN2 := 2));

    (* ── DAY_OF_WEEK ──────────────────────────────────────────── *)
    g_dow_thu := DAY_OF_WEEK(IN1 := d2);        (* 1970-01-01 = Thursday = 4 *)
    g_dow_sun := DAY_OF_WEEK(IN1 := d3);        (* 1970-01-04 = Sunday = 0 *)
    g_dow_mon := DAY_OF_WEEK(IN1 := d1);        (* 2024-01-15 = Monday = 1 *)

    (* ── Generic TO_*/ANY_TO_* ────────────────────────────────── *)
    g_to_date   := DATE_TO_LINT(IN1 := TO_DATE(IN1 := 0));
    g_any_to_tod := TOD_TO_DINT(IN1 := ANY_TO_TOD(IN1 := 45000000));
    g_any_to_dt  := DT_TO_LINT(IN1 := ANY_TO_DT(IN1 := 1000));

    (* ── Round-trip: DT → split → concat → verify ─────────────── *)
    d_part := DT_TO_DATE(IN1 := dt1);
    t_part := DT_TO_TOD(IN1 := dt1);
    g_roundtrip_dt := DT_TO_LINT(IN1 := CONCAT_DATE_TOD(IN1 := d_part, IN2 := t_part));
END_PROGRAM
