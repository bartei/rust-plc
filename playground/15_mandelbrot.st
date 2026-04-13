(*
 * Mandelbrot set — ASCII art rendered one row per scan cycle.
 *
 * Computes the classic Mandelbrot fractal over the region
 * [-2.0, 0.8] x [-1.2, 1.2] on an 80x40 character grid.
 * Each scan cycle computes one full row (80 pixels).
 * After 40 cycles the entire image is done and it restarts.
 *
 * The escape-time algorithm iterates z = z^2 + c for each pixel,
 * counting how many iterations before |z| > 2.0 (max 80).
 * The iteration count maps to an ASCII density character.
 *
 * Run:
 *   st-cli run playground/15_mandelbrot.st -n 42
 *)

VAR_GLOBAL
    (* Current row being rendered (0..39) *)
    g_row        : INT := 0;
    (* Total rows completed *)
    g_rows_done  : INT := 0;
    (* Pixels computed this cycle *)
    g_pixels     : INT := 0;
    (* Total escape iterations across all pixels (measures "complexity") *)
    g_total_iters : INT := 0;
    (* Max iterations hit by any single pixel *)
    g_max_iters   : INT := 0;
    (* Count of pixels inside the set (didn't escape) *)
    g_inside_count : INT := 0;
END_VAR

(*
 * Mandelbrot escape-time computation for a single point.
 * Returns the number of iterations before |z| > 2.0,
 * or max_iter if the point is inside the set.
 *)
FUNCTION MandelbrotEscape : INT
VAR_INPUT
    cx       : REAL;    (* Real part of c *)
    cy       : REAL;    (* Imaginary part of c *)
    max_iter : INT;     (* Maximum iterations *)
END_VAR
VAR
    zr   : REAL := 0.0;   (* Real part of z *)
    zi   : REAL := 0.0;   (* Imaginary part of z *)
    zr2  : REAL;           (* zr * zr — cached to avoid recomputation *)
    zi2  : REAL;           (* zi * zi *)
    iter : INT  := 0;
END_VAR
    (* z = 0 + 0i, iterate z := z^2 + c *)
    WHILE iter < max_iter DO
        zr2 := zr * zr;
        zi2 := zi * zi;

        (* Escape condition: |z|^2 > 4.0 *)
        IF (zr2 + zi2) > 4.0 THEN
            MandelbrotEscape := iter;
            RETURN;
        END_IF;

        (* z := z^2 + c
           (a + bi)^2 = a^2 - b^2 + 2abi *)
        zi := 2.0 * zr * zi + cy;
        zr := zr2 - zi2 + cx;

        iter := iter + 1;
    END_WHILE;

    (* Point is inside the set *)
    MandelbrotEscape := max_iter;
END_FUNCTION

(*
 * Map an iteration count to an ASCII density character index.
 * 0 = space (inside set), higher = denser glyph.
 * We return an INT index into a conceptual palette:
 *   0: ' '  (inside the Mandelbrot set)
 *   1: '.'  2: ':'  3: '-'  4: '='  5: '+'
 *   6: '*'  7: '#'  8: '%'  9: '@'
 *)
FUNCTION IterToGlyph : INT
VAR_INPUT
    iters    : INT;
    max_iter : INT;
END_VAR
    IF iters >= max_iter THEN
        IterToGlyph := 0;    (* Inside set *)
    ELSIF iters < 2 THEN
        IterToGlyph := 9;    (* Very fast escape = dense *)
    ELSIF iters < 4 THEN
        IterToGlyph := 8;
    ELSIF iters < 8 THEN
        IterToGlyph := 7;
    ELSIF iters < 12 THEN
        IterToGlyph := 6;
    ELSIF iters < 20 THEN
        IterToGlyph := 5;
    ELSIF iters < 30 THEN
        IterToGlyph := 4;
    ELSIF iters < 45 THEN
        IterToGlyph := 3;
    ELSIF iters < 60 THEN
        IterToGlyph := 2;
    ELSE
        IterToGlyph := 1;
    END_IF;
END_FUNCTION

(*
 * Main program — computes one row of the Mandelbrot set per scan cycle.
 *
 * Grid: 80 columns x 40 rows
 * Region: x in [-2.0, 0.8], y in [-1.2, 1.2]
 *)
PROGRAM Main
VAR
    col       : INT;
    cx        : REAL;
    cy        : REAL;
    iters     : INT;
    glyph     : INT;
    row_max   : INT := 0;
    row_inside : INT := 0;
    row_iters  : INT := 0;
END_VAR
    (* Map current row to imaginary axis: y in [-1.2, 1.2] *)
    cy := 1.2 - (INT_TO_REAL(IN1 := g_row) * 2.4 / 40.0);

    row_max := 0;
    row_inside := 0;
    row_iters := 0;
    g_pixels := 0;

    FOR col := 0 TO 79 DO
        (* Map column to real axis: x in [-2.0, 0.8] *)
        cx := -2.0 + (INT_TO_REAL(IN1 := col) * 2.8 / 80.0);

        (* Compute escape time *)
        iters := MandelbrotEscape(cx := cx, cy := cy, max_iter := 80);

        (* Track stats *)
        row_iters := row_iters + iters;
        IF iters > row_max THEN
            row_max := iters;
        END_IF;
        IF iters >= 80 THEN
            row_inside := row_inside + 1;
        END_IF;

        (* Convert to ASCII glyph index *)
        glyph := IterToGlyph(iters := iters, max_iter := 80);

        g_pixels := g_pixels + 1;
    END_FOR;

    (* Update globals *)
    g_total_iters := g_total_iters + row_iters;
    g_inside_count := g_inside_count + row_inside;
    IF row_max > g_max_iters THEN
        g_max_iters := row_max;
    END_IF;

    g_rows_done := g_rows_done + 1;
    g_row := g_row + 1;

    (* Wrap around after 40 rows — restart the render *)
    IF g_row >= 40 THEN
        g_row := 0;
    END_IF;
END_PROGRAM
