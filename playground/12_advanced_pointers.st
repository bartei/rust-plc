(*
 * Advanced Pointers — REF_TO patterns for industrial automation
 *
 * Demonstrates pointer features beyond the basics:
 * - Cross-function pointer passing (pointers survive call boundaries)
 * - Write-through-pointer modifying the original variable
 * - Pointer reassignment to different targets
 * - Swap pattern using pointer parameters
 * - Pointer-based indirect increment
 * - Conditional pointer target selection
 * - NULL safety: dereference returns 0, write is no-op
 * - Two pointers aliasing the same variable
 * - Pointers to globals from within functions
 * - Multi-cycle pointer state persistence
 * - Pointers combined with classes
 *
 * Run with: st-cli run playground/12_advanced_pointers.st -n 20
 *)

// ═══════════════════════════════════════════════════════════════════════
// Utility functions that operate through pointers
// ═══════════════════════════════════════════════════════════════════════

// Swap two integers through pointers — the classic pattern
FUNCTION Swap : INT
VAR_INPUT
    p1 : REF_TO INT;
    p2 : REF_TO INT;
END_VAR
VAR
    temp : INT;
END_VAR
    temp := p1^;
    p1^ := p2^;
    p2^ := temp;
    Swap := 0;
END_FUNCTION

// Clamp a value in-place through a pointer
FUNCTION ClampInPlace : INT
VAR_INPUT
    target : REF_TO INT;
    lo : INT;
    hi : INT;
END_VAR
    IF target^ < lo THEN
        target^ := lo;
    ELSIF target^ > hi THEN
        target^ := hi;
    END_IF;
    ClampInPlace := target^;
END_FUNCTION

// Increment through a pointer, returning the new value
FUNCTION IncrementBy : INT
VAR_INPUT
    target : REF_TO INT;
    amount : INT;
END_VAR
    IF target <> NULL THEN
        target^ := target^ + amount;
    END_IF;
    IncrementBy := target^;
END_FUNCTION

// Select the larger of two values and double it in-place
FUNCTION DoubleLarger : INT
VAR_INPUT
    pa : REF_TO INT;
    pb : REF_TO INT;
END_VAR
    IF pa^ >= pb^ THEN
        pa^ := pa^ * 2;
        DoubleLarger := pa^;
    ELSE
        pb^ := pb^ * 2;
        DoubleLarger := pb^;
    END_IF;
END_FUNCTION

// ═══════════════════════════════════════════════════════════════════════
// FB that uses pointers for output binding
// ═══════════════════════════════════════════════════════════════════════

FUNCTION_BLOCK RampGenerator
VAR_INPUT
    target   : REF_TO INT;    // where to write the output
    stepSize : INT;
    maxValue : INT;
END_VAR
VAR
    direction : INT := 1;     // 1 = up, -1 = down
END_VAR
    IF target <> NULL THEN
        target^ := target^ + stepSize * direction;
        IF target^ >= maxValue THEN
            direction := -1;
        ELSIF target^ <= 0 THEN
            direction := 1;
        END_IF;
    END_IF;
END_FUNCTION_BLOCK

// ═══════════════════════════════════════════════════════════════════════
// Class that uses pointers internally
// ═══════════════════════════════════════════════════════════════════════

CLASS PointerAccumulator
VAR
    _total : INT := 0;
END_VAR

    // Add a value referenced by a pointer
    PUBLIC METHOD AddFrom
    VAR_INPUT
        source : REF_TO INT;
    END_VAR
        IF source <> NULL THEN
            _total := _total + source^;
        END_IF;
    END_METHOD

    // Write total into a target via pointer
    PUBLIC METHOD WriteTo
    VAR_INPUT
        dest : REF_TO INT;
    END_VAR
        IF dest <> NULL THEN
            dest^ := _total;
        END_IF;
    END_METHOD

    PUBLIC METHOD GetTotal : INT
        GetTotal := _total;
    END_METHOD

    PUBLIC METHOD Reset
        _total := 0;
    END_METHOD
END_CLASS

// ═══════════════════════════════════════════════════════════════════════
// Main program — exercises all pointer patterns
// ═══════════════════════════════════════════════════════════════════════

VAR_GLOBAL
    // Swap results
    g_swap_a     : INT;
    g_swap_b     : INT;

    // Clamp results
    g_clamped    : INT;

    // Pointer reassignment
    g_selected   : INT;

    // Two pointers, one variable
    g_aliased    : INT;

    // Cross-function write
    g_incremented : INT;

    // Conditional target
    g_cond_a     : INT;
    g_cond_b     : INT;

    // NULL safety
    g_null_read  : INT;
    g_null_ok    : INT;

    // Double-larger
    g_dl_x       : INT;
    g_dl_y       : INT;

    // Ramp generator
    g_ramp       : INT;

    // Accumulator
    g_acc_total  : INT;
    g_acc_dest   : INT;

    // Cycle tracking
    g_cycle      : INT;
END_VAR

PROGRAM Main
VAR
    // Working variables
    a : INT := 100;
    b : INT := 200;
    x : INT := 500;
    y : INT := 300;
    val : INT := 0;
    rampVal : INT := 0;
    source1 : INT := 7;
    source2 : INT := 13;
    destVal : INT := 0;

    // Pointers
    p : REF_TO INT;
    p1 : REF_TO INT;
    p2 : REF_TO INT;
    pNull : REF_TO INT;

    // FBs and classes
    ramp : RampGenerator;
    acc  : PointerAccumulator;

    cycle : INT := 0;
    dummy : INT;
END_VAR
    cycle := cycle + 1;
    g_cycle := cycle;

    // ── 1. Swap via pointers ──
    IF cycle = 1 THEN
        a := 100;
        b := 200;
    END_IF;
    dummy := Swap(p1 := REF(a), p2 := REF(b));
    g_swap_a := a;
    g_swap_b := b;
    // After each cycle, a and b toggle: 200/100, 100/200, ...

    // ── 2. Clamp in-place ──
    val := cycle * 15;
    dummy := ClampInPlace(target := REF(val), lo := 0, hi := 100);
    g_clamped := val;

    // ── 3. Pointer reassignment ──
    IF (cycle MOD 2) = 1 THEN
        p := REF(a);
    ELSE
        p := REF(b);
    END_IF;
    g_selected := p^;

    // ── 4. Two pointers, same variable (aliasing) ──
    x := 0;
    p1 := REF(x);
    p2 := REF(x);
    p1^ := 42;
    g_aliased := p2^;     // reads 42 through alias

    // ── 5. Cross-function increment ──
    dummy := IncrementBy(target := REF(x), amount := 10);
    g_incremented := x;   // 42 + 10 = 52

    // ── 6. Conditional pointer target ──
    IF cycle <= 10 THEN
        p := REF(a);
    ELSE
        p := REF(b);
    END_IF;
    p^ := p^ + 1;
    g_cond_a := a;
    g_cond_b := b;

    // ── 7. NULL safety ──
    pNull := NULL;
    g_null_read := pNull^;    // safe: returns 0
    pNull^ := 999;            // safe: no-op
    g_null_ok := 1;           // we survived

    // ── 8. Double the larger ──
    IF cycle = 1 THEN
        x := 30;
        y := 50;
    END_IF;
    dummy := DoubleLarger(pa := REF(x), pb := REF(y));
    g_dl_x := x;
    g_dl_y := y;

    // ── 9. Ramp generator (FB with pointer output) ──
    ramp(target := REF(rampVal), stepSize := 5, maxValue := 50);
    g_ramp := rampVal;

    // ── 10. Class method with pointer arguments ──
    acc.AddFrom(source := REF(source1));  // +7
    acc.AddFrom(source := REF(source2));  // +13
    g_acc_total := acc.GetTotal();
    acc.WriteTo(dest := REF(destVal));
    g_acc_dest := destVal;

    // Reset accumulator every 10 cycles
    IF (cycle MOD 10) = 0 THEN
        acc.Reset();
    END_IF;
END_PROGRAM
