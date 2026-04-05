(*
 * IEC 61131-3 Standard Library: Trigonometry & Transcendental Functions
 *
 * These are thin wrappers around VM intrinsic instructions.
 * All angles are in radians.
 *
 * SQRT  — Square root
 * SIN   — Sine
 * COS   — Cosine
 * TAN   — Tangent
 * ASIN  — Arc sine (inverse sine)
 * ACOS  — Arc cosine (inverse cosine)
 * ATAN  — Arc tangent (inverse tangent)
 * LN    — Natural logarithm (base e)
 * LOG   — Common logarithm (base 10)
 * EXP   — Exponential (e^x)
 *)

(* Note: These functions are compiled as VM intrinsic instructions.
   The compiler recognizes SQRT, SIN, COS, TAN, ASIN, ACOS, ATAN,
   LN, LOG, and EXP by name and emits dedicated bytecode instructions
   instead of regular function calls. No ST implementation needed. *)
