(*
 * Error showcase — intentionally broken code to demonstrate diagnostics.
 * Open this file to see the LSP reporting errors and warnings in real time.
 *
 * Try fixing the errors one by one and watch diagnostics update!
 *)

PROGRAM ErrorShowcase
VAR
    x : INT := 0;
    b : BOOL := TRUE;
    unused_var : REAL := 3.14;  // ⚠ unused variable warning
END_VAR
    // ❌ Error: undeclared variable
    x := undeclared_variable;

    // ❌ Error: type mismatch — assigning BOOL to INT
    x := b;

    // ❌ Error: condition must be BOOL, not INT
    IF x THEN
        x := 0;
    END_IF;

    // ❌ Error: EXIT outside of a loop
    EXIT;

    // ❌ Error: calling something that isn't a function
    x := x(a := 1);
END_PROGRAM
