(*
 * Playground 18: String Manipulation & Formatting
 *
 * Demonstrates every IEC 61131-3 / CODESYS-compatible string function:
 *   - Core: LEN, LEFT, RIGHT, MID, CONCAT, INSERT, DELETE, REPLACE, FIND
 *   - Case: TO_UPPER, TO_LOWER (and aliases UPPER_CASE, LOWER_CASE)
 *   - Trim: TRIM, LTRIM, RTRIM
 *   - Numeric ↔ STRING: *_TO_STRING, STRING_TO_*, generic TO_STRING
 *
 * Each global below has a deterministic expected value; the matching
 * E2E test in stdlib_tests.rs (`playground_18_strings_e2e`) asserts
 * every one of them.
 *
 * Run with: st-cli run playground/18_strings.st
 *)

VAR_GLOBAL
    (* LEN *)
    g_len_hello   : INT;
    g_len_empty   : INT;

    (* LEFT / RIGHT — including out-of-range clamping *)
    g_left_3      : STRING;
    g_left_zero   : STRING;
    g_left_huge   : STRING;
    g_left_neg    : STRING;
    g_right_3     : STRING;
    g_right_zero  : STRING;
    g_right_huge  : STRING;

    (* MID — including edge cases *)
    g_mid_mid     : STRING;
    g_mid_start   : STRING;
    g_mid_end     : STRING;
    g_mid_overrun : STRING;
    g_mid_pos0    : STRING;
    g_mid_negpos  : STRING;
    g_mid_zerolen : STRING;

    (* CONCAT *)
    g_concat      : STRING;
    g_concat_empty: STRING;

    (* INSERT — pos=0, middle, beyond-end *)
    g_insert_mid  : STRING;
    g_insert_zero : STRING;
    g_insert_far  : STRING;

    (* DELETE — normal + no-op cases *)
    g_delete_mid  : STRING;
    g_delete_pos0 : STRING;
    g_delete_zero : STRING;
    g_delete_overrun : STRING;

    (* REPLACE — normal, replace at start, append *)
    g_replace_mid : STRING;
    g_replace_zero: STRING;
    g_replace_far : STRING;

    (* FIND *)
    g_find_yes    : INT;
    g_find_first  : INT;
    g_find_no     : INT;
    g_find_empty  : INT;

    (* Case conversion *)
    g_upper       : STRING;
    g_lower       : STRING;
    g_upper_alias : STRING;
    g_lower_alias : STRING;

    (* Trim *)
    g_trim        : STRING;
    g_ltrim       : STRING;
    g_rtrim       : STRING;

    (* Numeric → STRING *)
    g_int_to_s    : STRING;
    g_dint_to_s   : STRING;
    g_neg_to_s    : STRING;
    g_real_to_s   : STRING;
    g_real_int_s  : STRING;
    g_bool_t_to_s : STRING;
    g_bool_f_to_s : STRING;
    g_to_string_i : STRING;
    g_to_string_b : STRING;

    (* STRING → numeric *)
    g_s_to_int    : INT;
    g_s_to_int_neg: INT;
    g_s_to_real   : REAL;
    g_s_to_bool_t : INT;
    g_s_to_bool_1 : INT;
    g_s_to_bool_f : INT;
    g_s_to_int_bad: INT;

    (* Round-trip *)
    g_roundtrip_int : INT;
END_VAR

PROGRAM Main
VAR
    s         : STRING := 'hello';
    abc       : STRING := 'abcdef';
    spaces    : STRING := '  spaced  ';
    foo       : STRING := 'foo';
    bar       : STRING := 'bar';
    empty     : STRING := '';
    haystack  : STRING := 'rust-plc rocks';
    needle1   : STRING := 'plc';
    needle2   : STRING := 'X';
    n_int     : INT := 42;
    n_dint    : DINT := 1234567;
    n_neg     : INT := -7;
    n_real    : REAL := 3.5;
    n_real_i  : REAL := 1.0;
    s_num     : STRING := '123';
    s_neg_num : STRING := '-99';
    s_real    : STRING := '2.5';
    s_true    : STRING := 'TRUE';
    s_one     : STRING := '1';
    s_false   : STRING := 'false';
    s_bad     : STRING := 'banana';
    rt_text   : STRING;
END_VAR
    (* ── LEN ──────────────────────────────────────────────────────── *)
    g_len_hello := LEN(IN := s);                          (* 5 *)
    g_len_empty := LEN(IN := empty);                      (* 0 *)

    (* ── LEFT / RIGHT ─────────────────────────────────────────────── *)
    g_left_3     := LEFT(STR := abc, SIZE := 3);          (* 'abc' *)
    g_left_zero  := LEFT(STR := abc, SIZE := 0);          (* '' *)
    g_left_huge  := LEFT(STR := abc, SIZE := 100);        (* 'abcdef' *)
    g_left_neg   := LEFT(STR := abc, SIZE := -5);         (* '' *)
    g_right_3    := RIGHT(STR := abc, SIZE := 3);         (* 'def' *)
    g_right_zero := RIGHT(STR := abc, SIZE := 0);         (* '' *)
    g_right_huge := RIGHT(STR := abc, SIZE := 100);       (* 'abcdef' *)

    (* ── MID — 1-indexed, clamped ─────────────────────────────────── *)
    g_mid_mid     := MID(STR := abc, LEN := 3, POS := 2); (* 'bcd' *)
    g_mid_start   := MID(STR := abc, LEN := 2, POS := 1); (* 'ab' *)
    g_mid_end     := MID(STR := abc, LEN := 2, POS := 5); (* 'ef' *)
    g_mid_overrun := MID(STR := abc, LEN := 10, POS := 4);(* 'def' *)
    g_mid_pos0    := MID(STR := abc, LEN := 3, POS := 0); (* '' *)
    g_mid_negpos  := MID(STR := abc, LEN := 3, POS := -1);(* '' *)
    g_mid_zerolen := MID(STR := abc, LEN := 0, POS := 2); (* '' *)

    (* ── CONCAT ───────────────────────────────────────────────────── *)
    g_concat       := CONCAT(STR1 := foo, STR2 := bar);   (* 'foobar' *)
    g_concat_empty := CONCAT(STR1 := empty, STR2 := foo); (* 'foo' *)

    (* ── INSERT ───────────────────────────────────────────────────── *)
    g_insert_mid  := INSERT(STR1 := 'abef', STR2 := 'cd', POS := 2);  (* 'abcdef' *)
    g_insert_zero := INSERT(STR1 := abc, STR2 := 'XYZ', POS := 0);    (* 'XYZabcdef' *)
    g_insert_far  := INSERT(STR1 := abc, STR2 := 'XYZ', POS := 100);  (* 'abcdefXYZ' *)

    (* ── DELETE ───────────────────────────────────────────────────── *)
    g_delete_mid     := DELETE(STR := abc, LEN := 2, POS := 3);  (* 'abef' *)
    g_delete_pos0    := DELETE(STR := abc, LEN := 2, POS := 0);  (* 'abcdef' (no-op) *)
    g_delete_zero    := DELETE(STR := abc, LEN := 0, POS := 2);  (* 'abcdef' (no-op) *)
    g_delete_overrun := DELETE(STR := abc, LEN := 100, POS := 3);(* 'ab' *)

    (* ── REPLACE ──────────────────────────────────────────────────── *)
    g_replace_mid  := REPLACE(STR1 := abc, STR2 := 'XY', LEN := 3, POS := 2); (* 'aXYef' *)
    g_replace_zero := REPLACE(STR1 := abc, STR2 := 'XY', LEN := 2, POS := 0); (* 'XYcdef' (insert at start, drop 2) *)
    g_replace_far  := REPLACE(STR1 := abc, STR2 := 'XY', LEN := 1, POS := 100); (* 'abcdefXY' *)

    (* ── FIND (1-based, 0 = not found) ────────────────────────────── *)
    g_find_yes   := FIND(STR1 := haystack, STR2 := needle1);  (* 6 *)
    g_find_first := FIND(STR1 := 'aaa', STR2 := 'a');         (* 1 *)
    g_find_no    := FIND(STR1 := haystack, STR2 := needle2);  (* 0 *)
    g_find_empty := FIND(STR1 := haystack, STR2 := empty);    (* 0 *)

    (* ── Case ─────────────────────────────────────────────────────── *)
    g_upper       := TO_UPPER(IN := s);                    (* 'HELLO' *)
    g_lower       := TO_LOWER(IN := 'WORLD');              (* 'world' *)
    g_upper_alias := UPPER_CASE(IN := 'mixed');            (* 'MIXED' *)
    g_lower_alias := LOWER_CASE(IN := 'MIXED');            (* 'mixed' *)

    (* ── Trim ─────────────────────────────────────────────────────── *)
    g_trim  := TRIM(IN := spaces);                         (* 'spaced' *)
    g_ltrim := LTRIM(IN := spaces);                        (* 'spaced  ' *)
    g_rtrim := RTRIM(IN := spaces);                        (* '  spaced' *)

    (* ── Numeric → STRING ─────────────────────────────────────────── *)
    g_int_to_s    := INT_TO_STRING(IN := n_int);           (* '42' *)
    g_dint_to_s   := DINT_TO_STRING(IN := n_dint);         (* '1234567' *)
    g_neg_to_s    := INT_TO_STRING(IN := n_neg);           (* '-7' *)
    g_real_to_s   := REAL_TO_STRING(IN := n_real);         (* '3.5' *)
    g_real_int_s  := REAL_TO_STRING(IN := n_real_i);       (* '1.0' *)
    g_bool_t_to_s := BOOL_TO_STRING(IN := TRUE);           (* 'TRUE' *)
    g_bool_f_to_s := BOOL_TO_STRING(IN := FALSE);          (* 'FALSE' *)
    g_to_string_i := TO_STRING(IN := 99);                  (* '99' *)
    g_to_string_b := TO_STRING(IN := TRUE);                (* 'TRUE' *)

    (* ── STRING → numeric ─────────────────────────────────────────── *)
    g_s_to_int     := STRING_TO_INT(IN := s_num);          (* 123 *)
    g_s_to_int_neg := STRING_TO_INT(IN := s_neg_num);      (* -99 *)
    g_s_to_real    := STRING_TO_REAL(IN := s_real);        (* 2.5 *)
    g_s_to_bool_t  := BOOL_TO_INT(IN := STRING_TO_BOOL(IN := s_true));  (* 1 *)
    g_s_to_bool_1  := BOOL_TO_INT(IN := STRING_TO_BOOL(IN := s_one));   (* 1 *)
    g_s_to_bool_f  := BOOL_TO_INT(IN := STRING_TO_BOOL(IN := s_false)); (* 0 *)
    g_s_to_int_bad := STRING_TO_INT(IN := s_bad);          (* 0 (parse fail) *)

    (* ── Round-trip: INT → STRING → INT ───────────────────────────── *)
    rt_text         := INT_TO_STRING(IN := 17);
    g_roundtrip_int := STRING_TO_INT(IN := rt_text);       (* 17 *)
END_PROGRAM
