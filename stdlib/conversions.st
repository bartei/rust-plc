(*
 * IEC 61131-3 Standard Library: Type Conversions
 *
 * Most conversion functions are compiled as VM intrinsic instructions.
 * The compiler recognizes *_TO_* patterns and emits ToInt, ToReal,
 * ToBool, or ToTime instructions directly.
 *
 * Available conversions:
 *
 * To REAL/LREAL:
 *   INT_TO_REAL, SINT_TO_REAL, DINT_TO_REAL, LINT_TO_REAL
 *   UINT_TO_REAL, USINT_TO_REAL, UDINT_TO_REAL, ULINT_TO_REAL
 *   BOOL_TO_REAL
 *   INT_TO_LREAL, SINT_TO_LREAL, DINT_TO_LREAL, LINT_TO_LREAL
 *   REAL_TO_LREAL
 *   TIME_TO_REAL, TIME_TO_LREAL
 *
 * To INT/DINT/LINT/SINT (and unsigned variants):
 *   REAL_TO_INT, LREAL_TO_INT, REAL_TO_DINT, LREAL_TO_DINT
 *   REAL_TO_LINT, LREAL_TO_LINT, REAL_TO_SINT, LREAL_TO_SINT
 *   BOOL_TO_INT, BOOL_TO_DINT, BOOL_TO_LINT
 *   UINT_TO_INT, UDINT_TO_DINT, ULINT_TO_LINT
 *   INT_TO_DINT, INT_TO_LINT, DINT_TO_LINT
 *   SINT_TO_INT, SINT_TO_DINT, SINT_TO_LINT
 *   TIME_TO_INT, TIME_TO_SINT, TIME_TO_DINT, TIME_TO_LINT
 *   TIME_TO_UINT, TIME_TO_USINT, TIME_TO_UDINT, TIME_TO_ULINT
 *
 * To BOOL:
 *   INT_TO_BOOL, REAL_TO_BOOL, DINT_TO_BOOL, LINT_TO_BOOL
 *   TIME_TO_BOOL
 *
 * To TIME (value interpreted as milliseconds):
 *   INT_TO_TIME, SINT_TO_TIME, DINT_TO_TIME, LINT_TO_TIME
 *   UINT_TO_TIME, USINT_TO_TIME, UDINT_TO_TIME, ULINT_TO_TIME
 *   REAL_TO_TIME, LREAL_TO_TIME
 *   BOOL_TO_TIME
 *
 * Overloaded generic conversions (IEC 61131-3 Ed.3 / CODESYS-compatible):
 *   TO_INT, TO_SINT, TO_DINT, TO_LINT
 *   TO_UINT, TO_USINT, TO_UDINT, TO_ULINT
 *   TO_REAL, TO_LREAL
 *   TO_BOOL
 *   TO_TIME
 *   ANY_TO_INT, ANY_TO_SINT, ANY_TO_DINT, ANY_TO_LINT
 *   ANY_TO_UINT, ANY_TO_USINT, ANY_TO_UDINT, ANY_TO_ULINT
 *   ANY_TO_REAL, ANY_TO_LREAL
 *   ANY_TO_BOOL
 *   ANY_TO_TIME
 *
 * TO_<target> and ANY_TO_<target> accept any input type.
 * The VM dispatches the conversion dynamically based on the source value.
 *
 * Example usage:
 *   my_real := INT_TO_REAL(IN1 := my_int);
 *   my_int  := REAL_TO_INT(IN1 := my_real);  (* truncates *)
 *   my_bool := INT_TO_BOOL(IN1 := my_int);   (* 0=FALSE, else TRUE *)
 *   my_ms   := TIME_TO_DINT(IN1 := my_time); (* TIME -> milliseconds *)
 *   my_time := DINT_TO_TIME(IN1 := 5000);    (* 5000ms = T#5s *)
 *   my_int  := TO_INT(IN1 := my_time);       (* generic: any -> INT *)
 *   my_time := ANY_TO_TIME(IN1 := 1000);     (* generic: any -> TIME *)
 *)
