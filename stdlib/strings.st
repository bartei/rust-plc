(*
 * IEC 61131-3 Standard Library: String Manipulation & Formatting
 *
 * All string functions are compiled as VM intrinsic instructions. STRING
 * is byte-oriented per IEC 61131-3 (no multi-byte / Unicode handling).
 * All position arguments are 1-indexed; out-of-range arguments are
 * clamped to produce empty / unchanged results rather than raising errors.
 *
 * ── Core IEC 61131-3 manipulation ─────────────────────────────────────
 *
 *   LEN(IN: STRING) : INT
 *     Length in bytes.
 *
 *   LEFT(STR: STRING, SIZE: INT) : STRING
 *     Leftmost SIZE bytes of STR. SIZE clamped to [0, LEN(STR)].
 *
 *   RIGHT(STR: STRING, SIZE: INT) : STRING
 *     Rightmost SIZE bytes of STR. SIZE clamped to [0, LEN(STR)].
 *
 *   MID(STR: STRING, LEN: INT, POS: INT) : STRING
 *     LEN bytes of STR starting at 1-based POS. Returns '' if POS<1,
 *     LEN<=0, or POS > LEN(STR); LEN clamped at end-of-string.
 *
 *   CONCAT(STR1: STRING, STR2: STRING) : STRING
 *     STR1 + STR2 (binary; variadic CONCAT not yet supported).
 *
 *   INSERT(STR1: STRING, STR2: STRING, POS: INT) : STRING
 *     Insert STR2 into STR1 after the POS-th byte. POS=0 returns
 *     STR2+STR1; POS>=LEN(STR1) returns STR1+STR2.
 *
 *   DELETE(STR: STRING, LEN: INT, POS: INT) : STRING
 *     Delete LEN bytes from STR starting at 1-based POS. Returns STR
 *     unchanged on out-of-range POS or LEN<=0.
 *
 *   REPLACE(STR1: STRING, STR2: STRING, LEN: INT, POS: INT) : STRING
 *     Replace LEN bytes at POS in STR1 with STR2. POS<1 inserts at
 *     start; POS>LEN(STR1) appends.
 *
 *   FIND(STR1: STRING, STR2: STRING) : INT
 *     1-based position of the first occurrence of STR2 in STR1; 0 if
 *     not found or STR2 is empty.
 *
 * ── Case conversion (CODESYS extension) ───────────────────────────────
 *
 *   TO_UPPER(IN: STRING) : STRING        (alias UPPER_CASE)
 *   TO_LOWER(IN: STRING) : STRING        (alias LOWER_CASE)
 *     ASCII-only case conversion.
 *
 * ── Trimming (CODESYS extension) ──────────────────────────────────────
 *
 *   TRIM(IN: STRING)  : STRING    Strip leading + trailing whitespace.
 *   LTRIM(IN: STRING) : STRING    Strip leading whitespace only.
 *   RTRIM(IN: STRING) : STRING    Strip trailing whitespace only.
 *
 * ── Numeric / boolean → STRING ────────────────────────────────────────
 *
 *   INT_TO_STRING, SINT_TO_STRING, DINT_TO_STRING, LINT_TO_STRING
 *   UINT_TO_STRING, USINT_TO_STRING, UDINT_TO_STRING, ULINT_TO_STRING
 *   REAL_TO_STRING, LREAL_TO_STRING
 *   BOOL_TO_STRING                    (returns 'TRUE' or 'FALSE')
 *
 *   TO_STRING / ANY_TO_STRING         Overloaded — accepts any input.
 *
 * ── STRING → numeric / boolean ────────────────────────────────────────
 *
 *   STRING_TO_INT, STRING_TO_SINT, STRING_TO_DINT, STRING_TO_LINT
 *   STRING_TO_UINT, STRING_TO_USINT, STRING_TO_UDINT, STRING_TO_ULINT
 *   STRING_TO_REAL, STRING_TO_LREAL
 *   STRING_TO_BOOL                    (TRUE if input is 'TRUE' or '1')
 *
 *   Whitespace is trimmed before parsing; on parse failure the result
 *   is the type's zero value (0, 0.0, FALSE).
 *
 * ── Examples ──────────────────────────────────────────────────────────
 *
 *   n      := LEN(IN := 'hello');                     (* 5 *)
 *   sub    := MID(STR := 'abcdef', LEN := 3, POS := 2); (* 'bcd' *)
 *   joined := CONCAT(STR1 := 'foo', STR2 := 'bar');    (* 'foobar' *)
 *   pos    := FIND(STR1 := 'rusted', STR2 := 'st');    (* 3 *)
 *   upper  := TO_UPPER(IN := 'hello');                 (* 'HELLO' *)
 *   trimmed:= TRIM(IN := '  spaced  ');                (* 'spaced' *)
 *   txt    := INT_TO_STRING(IN := 42);                 (* '42' *)
 *   i      := STRING_TO_INT(IN := '123');              (* 123 *)
 *
 *)
