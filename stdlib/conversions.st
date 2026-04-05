(*
 * IEC 61131-3 Standard Library: Type Conversions
 *
 * Most conversion functions are compiled as VM intrinsic instructions.
 * The compiler recognizes *_TO_* patterns and emits ToInt, ToReal, or
 * ToBool instructions directly.
 *
 * Available conversions:
 *
 * To REAL/LREAL:
 *   INT_TO_REAL, SINT_TO_REAL, DINT_TO_REAL, LINT_TO_REAL
 *   UINT_TO_REAL, USINT_TO_REAL, UDINT_TO_REAL, ULINT_TO_REAL
 *   BOOL_TO_REAL
 *   INT_TO_LREAL, SINT_TO_LREAL, DINT_TO_LREAL, LINT_TO_LREAL
 *   REAL_TO_LREAL
 *
 * To INT/DINT/LINT/SINT:
 *   REAL_TO_INT, LREAL_TO_INT, REAL_TO_DINT, LREAL_TO_DINT
 *   REAL_TO_LINT, LREAL_TO_LINT, REAL_TO_SINT, LREAL_TO_SINT
 *   BOOL_TO_INT, BOOL_TO_DINT, BOOL_TO_LINT
 *   UINT_TO_INT, UDINT_TO_DINT, ULINT_TO_LINT
 *   INT_TO_DINT, INT_TO_LINT, DINT_TO_LINT
 *   SINT_TO_INT, SINT_TO_DINT, SINT_TO_LINT
 *
 * To BOOL:
 *   INT_TO_BOOL, REAL_TO_BOOL, DINT_TO_BOOL, LINT_TO_BOOL
 *
 * Example usage:
 *   my_real := INT_TO_REAL(IN1 := my_int);
 *   my_int  := REAL_TO_INT(IN1 := my_real);  (* truncates *)
 *   my_bool := INT_TO_BOOL(IN1 := my_int);   (* 0=FALSE, else TRUE *)
 *)
