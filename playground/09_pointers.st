(*
 * Pointers and References — REF_TO and ^ dereference operator
 *
 * Demonstrates IEC 61131-3 pointer support:
 * - REF_TO <type>  declares a typed pointer variable
 * - REF(variable)  takes the reference (address) of a variable
 * - ptr^           dereferences a pointer (read or write)
 * - NULL           null pointer literal
 *
 * Run with: st-cli run playground/09_pointers.st -n 10
 *)

VAR_GLOBAL
    g_direct_value : INT;
    g_via_pointer : INT;
    g_swapped_a : INT;
    g_swapped_b : INT;
END_VAR

// ── Swap two integers using pointers ─────────────────────────────────
// This demonstrates how pointers enable generic operations
// on variables without knowing their names at compile time.

FUNCTION SwapInt : INT
VAR_INPUT
    p1 : REF_TO INT;
    p2 : REF_TO INT;
END_VAR
VAR
    temp : INT := 0;
END_VAR
    temp := p1^;
    p1^ := p2^;
    p2^ := temp;
    SwapInt := 0;
END_FUNCTION

// ── Indirect write: modify a variable through a pointer ──────────────

FUNCTION IncrementVia : INT
VAR_INPUT
    target : REF_TO INT;
    amount : INT;
END_VAR
    IF target <> NULL THEN
        target^ := target^ + amount;
    END_IF;
    IncrementVia := target^;
END_FUNCTION

// ── Main Program ─────────────────────────────────────────────────────

PROGRAM Main
VAR
    counter : INT := 0;
    value_a : INT := 100;
    value_b : INT := 200;

    ptr : REF_TO INT;
    null_ptr : REF_TO INT;

    cycle : INT := 0;
    dummy : INT := 0;
END_VAR
    cycle := cycle + 1;

    // ── Basic pointer usage ──
    // Take a reference and read through it
    ptr := REF(counter);
    counter := counter + 1;
    g_direct_value := counter;
    g_via_pointer := ptr^;         // Same value, accessed via pointer

    // ── Indirect modification ──
    // Modify counter through the pointer
    dummy := IncrementVia(target := REF(counter), amount := 10);

    // ── Swap two values using pointers ──
    // Every 5 cycles, swap value_a and value_b
    IF (cycle MOD 5) = 0 THEN
        dummy := SwapInt(p1 := REF(value_a), p2 := REF(value_b));
    END_IF;

    g_swapped_a := value_a;
    g_swapped_b := value_b;

    // ── NULL safety ──
    // Dereferencing NULL returns 0 (safe, no crash)
    null_ptr := NULL;
    dummy := null_ptr^;            // Returns 0
END_PROGRAM
