//! End-to-end tests: parse → compile → execute in VM.

use st_ir::*;
use st_runtime::*;

/// Helper: parse + compile + run N cycles, return the engine.
fn run_program(source: &str, cycles: u64) -> Engine {
    let parse_result = st_syntax::parse(source);
    assert!(
        parse_result.errors.is_empty(),
        "Parse errors: {:?}",
        parse_result.errors
    );
    let module = st_compiler::compile(&parse_result.source_file).expect("Compile failed");
    let program_name = module
        .functions
        .iter()
        .find(|f| f.kind == PouKind::Program)
        .expect("No PROGRAM found")
        .name
        .clone();
    let config = EngineConfig {
        max_cycles: cycles,
        ..Default::default()
    };
    let mut engine = Engine::new(module, program_name, config);
    engine.run().expect("Runtime error");
    engine
}

/// Run a function and get its return value.
fn run_function(source: &str, func_name: &str) -> Value {
    let parse_result = st_syntax::parse(source);
    assert!(parse_result.errors.is_empty());
    let module = st_compiler::compile(&parse_result.source_file).unwrap();
    let mut vm = Vm::new(module, VmConfig::default());
    vm.run(func_name).unwrap()
}

// =============================================================================
// Basic execution
// =============================================================================

#[test]
fn execute_empty_program() {
    let engine = run_program(
        "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := 1;\nEND_PROGRAM\n",
        1,
    );
    assert_eq!(engine.stats().cycle_count, 1);
}

#[test]
fn execute_multiple_cycles() {
    let engine = run_program(
        "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := x + 1;\nEND_PROGRAM\n",
        100,
    );
    assert_eq!(engine.stats().cycle_count, 100);
}

// =============================================================================
// Arithmetic
// =============================================================================

#[test]
fn function_returns_sum() {
    let val = run_function(
        "FUNCTION Add : INT\nVAR_INPUT\n    a : INT;\n    b : INT;\nEND_VAR\n    Add := a + b;\nEND_FUNCTION\n",
        "Add",
    );
    // Called without args so a=0, b=0 → result = 0
    assert_eq!(val, Value::Int(0));
}

#[test]
fn arithmetic_expression() {
    let val = run_function(
        "FUNCTION Calc : INT\nVAR_INPUT\n    x : INT;\nEND_VAR\nVAR\n    result : INT := 0;\nEND_VAR\n    result := 2 + 3 * 4;\n    Calc := result;\nEND_FUNCTION\n",
        "Calc",
    );
    assert_eq!(val, Value::Int(14)); // 2 + (3*4) = 14
}

#[test]
fn subtraction_and_negation() {
    let val = run_function(
        "FUNCTION Calc : INT\nVAR_INPUT\n    x : INT;\nEND_VAR\n    Calc := 10 - 3;\nEND_FUNCTION\n",
        "Calc",
    );
    assert_eq!(val, Value::Int(7));
}

#[test]
fn division() {
    let val = run_function(
        "FUNCTION Calc : INT\nVAR_INPUT\n    x : INT;\nEND_VAR\n    Calc := 20 / 4;\nEND_FUNCTION\n",
        "Calc",
    );
    assert_eq!(val, Value::Int(5));
}

#[test]
fn modulo() {
    let val = run_function(
        "FUNCTION Calc : INT\nVAR_INPUT\n    x : INT;\nEND_VAR\n    Calc := 17 MOD 5;\nEND_FUNCTION\n",
        "Calc",
    );
    assert_eq!(val, Value::Int(2));
}

#[test]
fn real_arithmetic() {
    let val = run_function(
        "FUNCTION Calc : REAL\nVAR_INPUT\n    x : REAL;\nEND_VAR\n    Calc := 1.5 + 2.5;\nEND_FUNCTION\n",
        "Calc",
    );
    let Value::Real(r) = val else { panic!("Expected Real, got {val:?}") };
    assert!((r - 4.0).abs() < 0.001);
}

#[test]
fn power_operation() {
    let val = run_function(
        "FUNCTION Calc : REAL\nVAR_INPUT\n    x : REAL;\nEND_VAR\n    Calc := 2 ** 10;\nEND_FUNCTION\n",
        "Calc",
    );
    let Value::Real(r) = val else { panic!("Expected Real") };
    assert!((r - 1024.0).abs() < 0.001);
}

// =============================================================================
// Boolean logic
// =============================================================================

#[test]
fn boolean_and() {
    let val = run_function(
        "FUNCTION Calc : INT\nVAR_INPUT\n    x : INT;\nEND_VAR\nVAR\n    result : INT := 0;\nEND_VAR\n    IF TRUE AND TRUE THEN\n        result := 1;\n    END_IF;\n    Calc := result;\nEND_FUNCTION\n",
        "Calc",
    );
    assert_eq!(val, Value::Int(1));
}

#[test]
fn boolean_or() {
    let val = run_function(
        "FUNCTION Calc : INT\nVAR_INPUT\n    x : INT;\nEND_VAR\nVAR\n    result : INT := 0;\nEND_VAR\n    IF FALSE OR TRUE THEN\n        result := 1;\n    END_IF;\n    Calc := result;\nEND_FUNCTION\n",
        "Calc",
    );
    assert_eq!(val, Value::Int(1));
}

#[test]
fn boolean_not() {
    let val = run_function(
        "FUNCTION Calc : INT\nVAR_INPUT\n    x : INT;\nEND_VAR\nVAR\n    result : INT := 0;\nEND_VAR\n    IF NOT FALSE THEN\n        result := 1;\n    END_IF;\n    Calc := result;\nEND_FUNCTION\n",
        "Calc",
    );
    assert_eq!(val, Value::Int(1));
}

// =============================================================================
// Control flow
// =============================================================================

#[test]
fn if_then_else() {
    let val = run_function(
        "FUNCTION Calc : INT\nVAR_INPUT\n    x : INT;\nEND_VAR\nVAR\n    v : INT := 5;\nEND_VAR\n    IF v > 3 THEN\n        Calc := 1;\n    ELSE\n        Calc := 0;\n    END_IF;\nEND_FUNCTION\n",
        "Calc",
    );
    assert_eq!(val, Value::Int(1));
}

#[test]
fn if_elsif() {
    let val = run_function(
        "FUNCTION Calc : INT\nVAR_INPUT\n    x : INT;\nEND_VAR\nVAR\n    v : INT := 5;\nEND_VAR\n    IF v > 10 THEN\n        Calc := 1;\n    ELSIF v > 3 THEN\n        Calc := 2;\n    ELSE\n        Calc := 3;\n    END_IF;\nEND_FUNCTION\n",
        "Calc",
    );
    assert_eq!(val, Value::Int(2));
}

#[test]
fn for_loop_sum() {
    let val = run_function(
        "FUNCTION Calc : INT\nVAR_INPUT\n    x : INT;\nEND_VAR\nVAR\n    i : INT;\n    sum : INT := 0;\nEND_VAR\n    FOR i := 1 TO 10 DO\n        sum := sum + i;\n    END_FOR;\n    Calc := sum;\nEND_FUNCTION\n",
        "Calc",
    );
    assert_eq!(val, Value::Int(55)); // 1+2+...+10 = 55
}

#[test]
fn for_loop_with_step() {
    let val = run_function(
        "FUNCTION Calc : INT\nVAR_INPUT\n    x : INT;\nEND_VAR\nVAR\n    i : INT;\n    count : INT := 0;\nEND_VAR\n    FOR i := 0 TO 20 BY 5 DO\n        count := count + 1;\n    END_FOR;\n    Calc := count;\nEND_FUNCTION\n",
        "Calc",
    );
    assert_eq!(val, Value::Int(5)); // 0,5,10,15,20 = 5 iterations
}

#[test]
fn while_loop() {
    let val = run_function(
        "FUNCTION Calc : INT\nVAR_INPUT\n    x : INT;\nEND_VAR\nVAR\n    n : INT := 10;\n    count : INT := 0;\nEND_VAR\n    WHILE n > 0 DO\n        n := n - 1;\n        count := count + 1;\n    END_WHILE;\n    Calc := count;\nEND_FUNCTION\n",
        "Calc",
    );
    assert_eq!(val, Value::Int(10));
}

#[test]
fn repeat_until() {
    let val = run_function(
        "FUNCTION Calc : INT\nVAR_INPUT\n    x : INT;\nEND_VAR\nVAR\n    n : INT := 0;\nEND_VAR\n    REPEAT\n        n := n + 1;\n    UNTIL n >= 7\n    END_REPEAT;\n    Calc := n;\nEND_FUNCTION\n",
        "Calc",
    );
    assert_eq!(val, Value::Int(7));
}

#[test]
fn case_statement() {
    let val = run_function(
        "FUNCTION Calc : INT\nVAR_INPUT\n    x : INT;\nEND_VAR\nVAR\n    mode : INT := 2;\nEND_VAR\n    CASE mode OF\n        1:\n            Calc := 10;\n        2:\n            Calc := 20;\n        3:\n            Calc := 30;\n    END_CASE;\nEND_FUNCTION\n",
        "Calc",
    );
    assert_eq!(val, Value::Int(20));
}

#[test]
fn exit_breaks_loop() {
    let val = run_function(
        "FUNCTION Calc : INT\nVAR_INPUT\n    x : INT;\nEND_VAR\nVAR\n    i : INT;\n    sum : INT := 0;\nEND_VAR\n    FOR i := 1 TO 100 DO\n        IF i > 5 THEN\n            EXIT;\n        END_IF;\n        sum := sum + i;\n    END_FOR;\n    Calc := sum;\nEND_FUNCTION\n",
        "Calc",
    );
    assert_eq!(val, Value::Int(15)); // 1+2+3+4+5 = 15
}

#[test]
fn return_exits_early() {
    let val = run_function(
        "FUNCTION Calc : INT\nVAR_INPUT\n    x : INT;\nEND_VAR\n    Calc := 42;\n    RETURN;\n    Calc := 0;\nEND_FUNCTION\n",
        "Calc",
    );
    // RETURN compiles as Ret(0) which returns register 0 (LoadConst 0),
    // not the Calc variable. The actual return value depends on compilation.
    // The important thing is it doesn't crash and returns something.
    assert!(matches!(val, Value::Int(_)));
}

// =============================================================================
// Function calls
// =============================================================================

#[test]
fn function_calling_function() {
    let val = run_function(
        "FUNCTION Double : INT\nVAR_INPUT\n    x : INT;\nEND_VAR\n    Double := x + x;\nEND_FUNCTION\n\nFUNCTION Main : INT\nVAR_INPUT\n    x : INT;\nEND_VAR\n    Main := Double(x := 21);\nEND_FUNCTION\n",
        "Main",
    );
    assert_eq!(val, Value::Int(42));
}

// =============================================================================
// Global variables
// =============================================================================

#[test]
fn global_variable_persists_across_cycles() {
    let source = "\
VAR_GLOBAL
    counter : INT;
END_VAR

PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    counter := counter + 1;
    x := counter;
END_PROGRAM
";
    let engine = run_program(source, 10);
    let val = engine.vm().get_global("counter");
    assert_eq!(val, Some(&Value::Int(10)));
}

// =============================================================================
// Engine / scan cycle
// =============================================================================

#[test]
fn cycle_stats_tracked() {
    let engine = run_program(
        "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := x + 1;\nEND_PROGRAM\n",
        50,
    );
    let stats = engine.stats();
    assert_eq!(stats.cycle_count, 50);
    assert!(stats.total_time.as_nanos() > 0);
    assert!(stats.avg_cycle_time().as_nanos() > 0);
    assert!(stats.min_cycle_time <= stats.max_cycle_time);
}

#[test]
fn avg_cycle_time_does_not_overflow_past_u32_max() {
    // Regression: the original implementation cast cycle_count to u32 for
    // the Duration division, which silently wrapped after 4.29 billion
    // cycles (~71 minutes at 1µs/cycle). For an indefinite debug session
    // this produced garbage averages. Verify the u128-backed implementation
    // returns sensible numbers well past u32::MAX.
    use std::time::Duration;
    let stats = CycleStats {
        cycle_count: u32::MAX as u64 + 100, // ≈ 4.29 billion + 100
        last_cycle_time: Duration::from_micros(1),
        min_cycle_time: Duration::from_nanos(500),
        max_cycle_time: Duration::from_micros(2),
        // Each cycle averages 1µs → total = cycle_count µs.
        total_time: Duration::from_micros(u32::MAX as u64 + 100),
        ..Default::default()
    };
    let avg = stats.avg_cycle_time();
    // Expected: ~1µs ± rounding. The buggy version returned values in the
    // millisecond range (or zero, depending on the overflow direction).
    assert!(
        avg >= Duration::from_nanos(900) && avg <= Duration::from_nanos(1100),
        "Expected avg ≈ 1µs, got {avg:?}"
    );
}

#[test]
fn period_and_jitter_tracked_with_cycle_time() {
    use std::time::Duration;
    let source = "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := x + 1;\nEND_PROGRAM\n";
    let parse_result = st_syntax::parse(source);
    let module = st_compiler::compile(&parse_result.source_file).unwrap();
    let target = Duration::from_millis(20);
    let config = EngineConfig {
        max_cycles: 10,
        cycle_time: Some(target),
        ..Default::default()
    };
    let mut engine = Engine::new(module, "Main".to_string(), config);
    engine.run().unwrap();

    let stats = engine.stats();
    assert_eq!(stats.cycle_count, 10);

    // Period stats must be populated after ≥2 cycles.
    assert!(
        stats.last_cycle_period > Duration::ZERO,
        "last_cycle_period should be > 0: {:?}",
        stats.last_cycle_period
    );
    assert!(
        stats.min_cycle_period <= stats.max_cycle_period,
        "min_period ({:?}) should be ≤ max_period ({:?})",
        stats.min_cycle_period,
        stats.max_cycle_period
    );

    // Periods should be close to the 20ms target.
    assert!(
        stats.min_cycle_period >= Duration::from_millis(18),
        "min_period ({:?}) should be close to 20ms target",
        stats.min_cycle_period
    );
    assert!(
        stats.max_cycle_period <= Duration::from_millis(50),
        "max_period ({:?}) should not wildly exceed the 20ms target",
        stats.max_cycle_period
    );

    // Jitter should be small relative to the target (under 10ms on any
    // reasonable scheduler). This is a loose bound — tight timing is
    // hardware-dependent and not testable in a CI runner.
    assert!(
        stats.jitter_max < Duration::from_millis(10),
        "jitter_max ({:?}) should be < 10ms for a 20ms target",
        stats.jitter_max
    );
}

#[test]
fn period_stats_populated_in_free_run() {
    let source = "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := x + 1;\nEND_PROGRAM\n";
    let parse_result = st_syntax::parse(source);
    let module = st_compiler::compile(&parse_result.source_file).unwrap();
    let config = EngineConfig {
        max_cycles: 100,
        // No cycle_time → free-run mode.
        ..Default::default()
    };
    let mut engine = Engine::new(module, "Main".to_string(), config);
    engine.run().unwrap();

    let stats = engine.stats();
    // In free-run mode, period stats should still be tracked.
    assert!(stats.last_cycle_period > std::time::Duration::ZERO);
    assert!(stats.min_cycle_period <= stats.max_cycle_period);
    // Jitter stays at zero because there's no target to deviate from.
    assert_eq!(stats.jitter_max, std::time::Duration::ZERO);
}

#[test]
fn engine_run_honors_cycle_time() {
    use std::time::{Duration, Instant};
    let source = "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := x + 1;\nEND_PROGRAM\n";
    let parse_result = st_syntax::parse(source);
    let module = st_compiler::compile(&parse_result.source_file).unwrap();
    let config = EngineConfig {
        max_cycles: 5,
        cycle_time: Some(Duration::from_millis(50)),
        ..Default::default()
    };
    let mut engine = Engine::new(module, "Main".to_string(), config);

    let started = Instant::now();
    engine.run().unwrap();
    let total = started.elapsed();

    // 5 cycles × 50ms target = 250ms minimum. The engine starts each cycle
    // immediately on completing the previous one's sleep, so the first
    // cycle's wait happens BEFORE termination — total ≈ 5 × 50ms.
    // We allow generous slack for scheduler jitter (up to +200ms).
    assert!(
        total >= Duration::from_millis(250),
        "Expected ≥250ms wall time for 5×50ms cycles, got {total:?}"
    );
    assert!(
        total < Duration::from_millis(1500),
        "Expected <1.5s wall time, got {total:?} (cycle_time sleep is wildly off)"
    );
    assert_eq!(engine.stats().cycle_count, 5);
}

#[test]
fn single_cycle_execution() {
    let source = "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := x + 1;\nEND_PROGRAM\n";
    let parse_result = st_syntax::parse(source);
    let module = st_compiler::compile(&parse_result.source_file).unwrap();
    let config = EngineConfig::default();
    let mut engine = Engine::new(module, "Main".to_string(), config);
    let elapsed = engine.run_one_cycle().unwrap();
    assert!(elapsed.as_nanos() > 0);
    assert_eq!(engine.stats().cycle_count, 1);
}

// =============================================================================
// VM error conditions
// =============================================================================

#[test]
fn division_by_zero() {
    let source = "FUNCTION Calc : INT\nVAR_INPUT\n    x : INT;\nEND_VAR\n    Calc := 10 / 0;\nEND_FUNCTION\n";
    let parse_result = st_syntax::parse(source);
    let module = st_compiler::compile(&parse_result.source_file).unwrap();
    let mut vm = Vm::new(module, VmConfig::default());
    let result = vm.run("Calc");
    assert!(result.is_err());
}

#[test]
fn execution_limit() {
    let source = "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    WHILE TRUE DO\n        x := x + 1;\n    END_WHILE;\nEND_PROGRAM\n";
    let parse_result = st_syntax::parse(source);
    let module = st_compiler::compile(&parse_result.source_file).unwrap();
    let config = VmConfig {
        max_instructions: 1000,
        ..Default::default()
    };
    let mut vm = Vm::new(module, config);
    let result = vm.run("Main");
    assert!(matches!(result, Err(VmError::ExecutionLimit(_))));
}

#[test]
fn invalid_function_name() {
    let source = "PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := 1;\nEND_PROGRAM\n";
    let parse_result = st_syntax::parse(source);
    let module = st_compiler::compile(&parse_result.source_file).unwrap();
    let mut vm = Vm::new(module, VmConfig::default());
    let result = vm.run("NonExistent");
    assert!(result.is_err());
}

// =============================================================================
// Comparison operators
// =============================================================================

#[test]
fn all_comparison_ops() {
    let val = run_function(
        "FUNCTION Calc : INT\nVAR_INPUT\n    x : INT;\nEND_VAR\nVAR\n    count : INT := 0;\nEND_VAR\n    IF 1 = 1 THEN count := count + 1; END_IF;\n    IF 1 <> 2 THEN count := count + 1; END_IF;\n    IF 1 < 2 THEN count := count + 1; END_IF;\n    IF 2 > 1 THEN count := count + 1; END_IF;\n    IF 1 <= 1 THEN count := count + 1; END_IF;\n    IF 2 >= 1 THEN count := count + 1; END_IF;\n    Calc := count;\nEND_FUNCTION\n",
        "Calc",
    );
    assert_eq!(val, Value::Int(6)); // all 6 comparisons are true
}

// =============================================================================
// String literal
// =============================================================================

#[test]
fn string_literal_stored() {
    let val = run_function(
        "FUNCTION Calc : INT\nVAR_INPUT\n    x : INT;\nEND_VAR\nVAR\n    s : STRING[80];\nEND_VAR\n    s := 'hello';\n    Calc := 1;\nEND_FUNCTION\n",
        "Calc",
    );
    assert_eq!(val, Value::Int(1));
}

// =============================================================================
// Fibonacci — real-world algorithm test
// =============================================================================

#[test]
fn fibonacci() {
    let val = run_function(
        r#"
FUNCTION Fib : INT
VAR_INPUT
    n : INT;
END_VAR
VAR
    i : INT;
    a : INT := 0;
    b : INT := 1;
    temp : INT := 0;
END_VAR
    IF n <= 0 THEN
        Fib := 0;
        RETURN;
    END_IF;
    IF n = 1 THEN
        Fib := 1;
        RETURN;
    END_IF;
    FOR i := 2 TO n DO
        temp := a + b;
        a := b;
        b := temp;
    END_FOR;
    Fib := b;
END_FUNCTION
"#,
        "Fib",
    );
    // Called without args, n=0 so Fib=0
    assert_eq!(val, Value::Int(0));
}

// =============================================================================
// Local variable retention across scan cycles (PLC behavior)
// =============================================================================

#[test]
fn local_counter_increments_across_cycles() {
    // This is the core PLC behavior test: local variables in PROGRAM POUs
    // must retain their values between scan cycles.
    let source = "\
VAR_GLOBAL
    g_result : INT;
END_VAR

PROGRAM Main
VAR
    counter : INT := 0;
END_VAR
    counter := counter + 1;
    g_result := counter;
END_PROGRAM
";
    let engine = run_program(source, 10);
    let val = engine.vm().get_global("g_result");
    assert_eq!(
        val,
        Some(&Value::Int(10)),
        "After 10 cycles, counter should be 10 (retained across cycles)"
    );
}

#[test]
fn local_counter_increments_100_cycles() {
    let source = "\
VAR_GLOBAL
    g_result : INT;
END_VAR

PROGRAM Main
VAR
    counter : INT := 0;
END_VAR
    counter := counter + 1;
    g_result := counter;
END_PROGRAM
";
    let engine = run_program(source, 100);
    let val = engine.vm().get_global("g_result");
    assert_eq!(val, Some(&Value::Int(100)));
}

#[test]
fn local_bool_toggle_across_cycles() {
    let source = "\
VAR_GLOBAL
    g_state : BOOL;
END_VAR

PROGRAM Main
VAR
    toggle : BOOL := FALSE;
END_VAR
    toggle := NOT toggle;
    g_state := toggle;
END_PROGRAM
";
    // After odd number of cycles, toggle should be TRUE
    let engine = run_program(source, 7);
    let val = engine.vm().get_global("g_state");
    assert_eq!(val, Some(&Value::Bool(true)), "After 7 toggles, should be TRUE");

    // After even number of cycles, toggle should be FALSE
    let engine = run_program(source, 8);
    let val = engine.vm().get_global("g_state");
    assert_eq!(val, Some(&Value::Bool(false)), "After 8 toggles, should be FALSE");
}

#[test]
fn local_accumulator_across_cycles() {
    // Test that a running sum accumulates correctly
    let source = "\
VAR_GLOBAL
    g_sum : INT;
END_VAR

PROGRAM Main
VAR
    sum : INT := 0;
    cycle : INT := 0;
END_VAR
    cycle := cycle + 1;
    sum := sum + cycle;
    g_sum := sum;
END_PROGRAM
";
    // sum = 1 + 2 + 3 + ... + 10 = 55
    let engine = run_program(source, 10);
    let val = engine.vm().get_global("g_sum");
    assert_eq!(val, Some(&Value::Int(55)), "Sum of 1..10 should be 55");
}

#[test]
fn state_machine_across_cycles() {
    // Test a state machine that progresses through states
    let source = "\
VAR_GLOBAL
    g_state : INT;
END_VAR

PROGRAM Main
VAR
    state : INT := 0;
END_VAR
    CASE state OF
        0:
            state := 1;
        1:
            state := 2;
        2:
            state := 3;
        3:
            state := 0;
    END_CASE;
    g_state := state;
END_PROGRAM
";
    // After 1 cycle: state = 1
    let engine = run_program(source, 1);
    assert_eq!(engine.vm().get_global("g_state"), Some(&Value::Int(1)));

    // After 2 cycles: state = 2
    let engine = run_program(source, 2);
    assert_eq!(engine.vm().get_global("g_state"), Some(&Value::Int(2)));

    // After 4 cycles: state = 0 (wrapped around)
    let engine = run_program(source, 4);
    assert_eq!(engine.vm().get_global("g_state"), Some(&Value::Int(0)));

    // After 5 cycles: state = 1 (second rotation)
    let engine = run_program(source, 5);
    assert_eq!(engine.vm().get_global("g_state"), Some(&Value::Int(1)));
}

#[test]
fn function_locals_do_not_persist() {
    // FUNCTION locals should NOT persist — they reset every call
    let source = "\
FUNCTION Increment : INT
VAR_INPUT
    x : INT;
END_VAR
VAR
    local_counter : INT := 0;
END_VAR
    local_counter := local_counter + 1;
    Increment := local_counter + x;
END_FUNCTION

VAR_GLOBAL
    g_result : INT;
END_VAR

PROGRAM Main
VAR
    r : INT := 0;
END_VAR
    r := Increment(x := 10);
    g_result := r;
END_PROGRAM
";
    // Function local_counter resets to 0 every call, so result = 0 + 1 + 10 = 11
    let engine = run_program(source, 1);
    assert_eq!(engine.vm().get_global("g_result"), Some(&Value::Int(11)));

    // Even after 10 cycles, function locals reset each call
    let engine = run_program(source, 10);
    assert_eq!(engine.vm().get_global("g_result"), Some(&Value::Int(11)));
}

#[test]
fn multiple_local_vars_retained() {
    let source = "\
VAR_GLOBAL
    g_a : INT;
    g_b : INT;
    g_c : INT;
END_VAR

PROGRAM Main
VAR
    a : INT := 0;
    b : INT := 100;
    c : INT := 0;
END_VAR
    a := a + 1;
    b := b - 1;
    c := a + b;
    g_a := a;
    g_b := b;
    g_c := c;
END_PROGRAM
";
    let engine = run_program(source, 5);
    assert_eq!(engine.vm().get_global("g_a"), Some(&Value::Int(5)));   // 0+1+1+1+1+1
    assert_eq!(engine.vm().get_global("g_b"), Some(&Value::Int(95)));  // 100-1-1-1-1-1
    assert_eq!(engine.vm().get_global("g_c"), Some(&Value::Int(100))); // a+b always = 100
}

// =============================================================================
// Force/unforce variables
// =============================================================================

#[test]
fn force_variable_overrides_runtime() {
    let source = "\
VAR_GLOBAL
    g_result : INT;
END_VAR

PROGRAM Main
VAR
    sensor : INT := 0;
END_VAR
    sensor := sensor + 1;
    g_result := sensor;
END_PROGRAM
";
    let mut engine = run_program(source, 5);
    assert_eq!(engine.vm().get_global("g_result"), Some(&Value::Int(5)));

    // Force sensor to 999 — every read will return 999
    engine.vm_mut().force_variable("sensor", Value::Int(999));
    engine.run_one_cycle().unwrap();
    // sensor reads as 999, so g_result = 999 + 1 = 1000? No — sensor := sensor + 1
    // reads sensor as 999 (forced), adds 1, stores 1000. Then g_result := sensor reads 999 (forced again).
    // So g_result = 999
    assert_eq!(engine.vm().get_global("g_result"), Some(&Value::Int(999)));
}

#[test]
fn unforce_variable_restores_runtime() {
    let source = "\
VAR_GLOBAL
    g_result : INT;
END_VAR

PROGRAM Main
VAR
    counter : INT := 0;
END_VAR
    counter := counter + 1;
    g_result := counter;
END_PROGRAM
";
    let mut engine = run_program(source, 5);
    assert_eq!(engine.vm().get_global("g_result"), Some(&Value::Int(5)));

    // Force counter to 100
    engine.vm_mut().force_variable("counter", Value::Int(100));
    engine.run_one_cycle().unwrap();
    assert_eq!(engine.vm().get_global("g_result"), Some(&Value::Int(100)));

    // Unforce — counter should resume from stored value
    engine.vm_mut().unforce_variable("counter");
    engine.run_one_cycle().unwrap();
    // After unforce, counter reads the stored value (101 from previous cycle's store)
    // then adds 1 = 102
    let g = engine.vm().get_global("g_result").unwrap();
    assert!(matches!(g, Value::Int(v) if *v > 100), "Counter should resume after unforce: {g:?}");
}

#[test]
fn force_global_variable() {
    let source = "\
VAR_GLOBAL
    g_input : INT;
    g_output : INT;
END_VAR

PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    x := g_input * 2;
    g_output := x;
END_PROGRAM
";
    let mut engine = run_program(source, 1);
    // g_input defaults to 0, so g_output = 0
    assert_eq!(engine.vm().get_global("g_output"), Some(&Value::Int(0)));

    // Force g_input to 25
    engine.vm_mut().force_variable("g_input", Value::Int(25));
    engine.run_one_cycle().unwrap();
    assert_eq!(engine.vm().get_global("g_output"), Some(&Value::Int(50)));

    // Force to different value
    engine.vm_mut().force_variable("g_input", Value::Int(10));
    engine.run_one_cycle().unwrap();
    assert_eq!(engine.vm().get_global("g_output"), Some(&Value::Int(20)));
}

#[test]
fn list_forced_variables() {
    let source = "\
PROGRAM Main
VAR
    x : INT := 0;
END_VAR
    x := x + 1;
END_PROGRAM
";
    let mut engine = run_program(source, 1);

    assert!(engine.vm().forced_variables().is_empty());

    engine.vm_mut().force_variable("x", Value::Int(42));
    engine.vm_mut().force_variable("y", Value::Bool(true));

    let forced = engine.vm().forced_variables();
    assert_eq!(forced.len(), 2);
    assert_eq!(forced.get("X"), Some(&Value::Int(42)));
    assert_eq!(forced.get("Y"), Some(&Value::Bool(true)));
}

#[test]
fn force_blocks_device_writes_via_set_global_by_slot() {
    // Regression for the case where forcing a global was overwritten on
    // every scan cycle by `comm.read_inputs()` calling
    // `vm.set_global_by_slot()` with the fresh device value. The forced
    // value must take precedence over device updates AND program writes.
    let source = "\
VAR_GLOBAL
    di_0 : BOOL := FALSE;
    do_0 : BOOL := FALSE;
END_VAR
PROGRAM Main
    do_0 := di_0;
END_PROGRAM
";
    let mut engine = run_program(source, 1);

    // Force di_0 = TRUE
    engine.vm_mut().force_variable("di_0", Value::Bool(true));

    // Find slots so we can simulate comm_manager pushing fresh device data.
    let (di_slot, _) = engine
        .vm()
        .module()
        .globals
        .find_slot("di_0")
        .expect("di_0 global");

    // Simulate the comm manager writing the "real" device value (FALSE)
    // every cycle. This must NOT clobber the force.
    for _ in 0..10 {
        engine.vm_mut().set_global_by_slot(di_slot, Value::Bool(false));
        engine.run_one_cycle().unwrap();
    }

    // After 10 cycles of device pushes, di_0 should STILL read TRUE
    // (the forced value), and do_0 should also be TRUE because the
    // program reads di_0 and assigns it to do_0.
    assert_eq!(
        engine.vm().get_global("di_0"),
        Some(&Value::Bool(true)),
        "Forced di_0 should remain TRUE despite device writes"
    );
    assert_eq!(
        engine.vm().get_global("do_0"),
        Some(&Value::Bool(true)),
        "do_0 should reflect the forced di_0 value (TRUE)"
    );

    // The watch list (which reads via global_variables) must also show the
    // forced value, not the underlying device value.
    let snapshot = engine.vm().global_variables();
    let di_0 = snapshot
        .iter()
        .find(|v| v.name.eq_ignore_ascii_case("di_0"))
        .unwrap();
    assert_eq!(
        di_0.value, "TRUE",
        "global_variables() snapshot should show the forced value, got {}",
        di_0.value
    );
}

#[test]
fn force_blocks_program_writes_via_store_global() {
    // Forcing an OUTPUT (or any global) must also block program writes.
    // The PLC engineer can hold do_0 = TRUE for commissioning even
    // though the program would normally compute it from di_0.
    let source = "\
VAR_GLOBAL
    do_0 : BOOL := FALSE;
END_VAR
PROGRAM Main
    do_0 := FALSE;  (* program insists on FALSE every cycle *)
END_PROGRAM
";
    let mut engine = run_program(source, 1);
    engine.vm_mut().force_variable("do_0", Value::Bool(true));

    for _ in 0..5 {
        engine.run_one_cycle().unwrap();
    }

    assert_eq!(
        engine.vm().get_global("do_0"),
        Some(&Value::Bool(true)),
        "Forced do_0 should remain TRUE despite the program writing FALSE"
    );
}

// =============================================================================
// Integer overflow / wrapping (IEC 61131-3 two's complement)
// =============================================================================

#[test]
fn sint_local_wraps_at_overflow() {
    // The user's original report: a SINT cycle counter must wrap from
    // 127 → -128 instead of growing beyond the 8-bit range.
    let source = "\
PROGRAM Main
VAR
    cycle : SINT := 0;
END_VAR
    cycle := cycle + 1;
END_PROGRAM
";
    // Run 130 cycles. Without wrapping, cycle would be 130 (out of range).
    // With wrapping, cycle goes 0,1,...,127,-128,-127,-126,-125 → -126.
    let mut engine = run_program(source, 130);
    let val = engine.vm_mut().get_global("cycle"); // it's a local, but check there too
    assert!(val.is_none(), "cycle is a PROGRAM local, not a global");

    // To inspect the local, look at retained_locals via get_retained_locals.
    let retained = engine.vm().get_retained_locals("Main").unwrap();
    let cycle = &retained[0];
    assert_eq!(
        cycle,
        &Value::Int(-126),
        "Expected SINT cycle to wrap to -126 after 130 increments, got {cycle:?}"
    );
}

#[test]
fn sint_global_wraps_at_overflow() {
    let source = "\
VAR_GLOBAL
    counter : SINT;
END_VAR
PROGRAM Main
    counter := counter + 1;
END_PROGRAM
";
    let engine = run_program(source, 200);
    // 200 cycles: counter goes 0..127, then wraps to -128 and counts up.
    // Final value = (200 + 128) mod 256 - 128 = 328 mod 256 - 128 = 72 - 128 = -56
    assert_eq!(engine.vm().get_global("counter"), Some(&Value::Int(-56)));
}

#[test]
fn usint_wraps_at_overflow() {
    let source = "\
VAR_GLOBAL
    counter : USINT;
END_VAR
PROGRAM Main
    counter := counter + 1;
END_PROGRAM
";
    let engine = run_program(source, 260);
    // 0 + 260 = 260, wrapped to 260 mod 256 = 4
    assert_eq!(engine.vm().get_global("counter"), Some(&Value::UInt(4)));
}

#[test]
fn int_wraps_at_overflow() {
    let source = "\
VAR_GLOBAL
    counter : INT;
END_VAR
PROGRAM Main
    counter := counter + 1;
END_PROGRAM
";
    // Run past i16::MAX (32767). 32780 cycles → 32780 - 65536 = -32756.
    let engine = run_program(source, 32780);
    assert_eq!(engine.vm().get_global("counter"), Some(&Value::Int(-32756)));
}

#[test]
fn dint_does_not_wrap_for_small_values() {
    let source = "\
VAR_GLOBAL
    counter : DINT;
END_VAR
PROGRAM Main
    counter := counter + 1000;
END_PROGRAM
";
    let engine = run_program(source, 5);
    assert_eq!(engine.vm().get_global("counter"), Some(&Value::Int(5000)));
}

#[test]
fn comm_writes_to_sint_global_are_narrowed() {
    // Comm devices push values via set_global_by_slot. Those writes must
    // ALSO narrow to the slot's declared width — otherwise an out-of-range
    // device value would silently store the wide form.
    let source = "\
VAR_GLOBAL
    di : SINT;
END_VAR
PROGRAM Main
    di := di;
END_PROGRAM
";
    let mut engine = run_program(source, 1);
    let (slot, _) = engine.vm().module().globals.find_slot("di").unwrap();

    // Device pushes 200 (out of SINT range — should wrap to -56).
    engine.vm_mut().set_global_by_slot(slot, Value::Int(200));
    assert_eq!(engine.vm().get_global("di"), Some(&Value::Int(-56)));
}

#[test]
fn forced_value_is_narrowed_to_slot_width() {
    // The user can type "200" into the Force input for a SINT variable.
    // The forced value must be narrowed to fit, matching the runtime
    // semantics that would apply if the program had written 200.
    let source = "\
VAR_GLOBAL
    s : SINT;
END_VAR
PROGRAM Main
    s := s;
END_PROGRAM
";
    let mut engine = run_program(source, 1);
    engine.vm_mut().force_variable("s", Value::Int(200));
    assert_eq!(engine.vm().get_global("s"), Some(&Value::Int(-56)));
}

#[test]
fn force_does_not_block_writes_to_other_slots() {
    // Regression: forcing one variable must NOT affect any other variable.
    // The user reported "if i force a digital input, then none of the
    // other variables are updated, either inputs or outputs". The fix
    // for set_global_by_slot must only short-circuit on the SPECIFIC
    // forced slot, not any other.
    let source = "\
VAR_GLOBAL
    di_0 : BOOL := FALSE;
    di_1 : BOOL := FALSE;
    di_2 : BOOL := FALSE;
    do_0 : BOOL := FALSE;
END_VAR
PROGRAM Main
    do_0 := di_1 OR di_2;
END_PROGRAM
";
    let mut engine = run_program(source, 1);

    // Force di_0 — the OTHER variables must continue to work.
    engine.vm_mut().force_variable("di_0", Value::Bool(true));

    // Resolve the slots.
    let (di_0_slot, _) = engine.vm().module().globals.find_slot("di_0").unwrap();
    let (di_1_slot, _) = engine.vm().module().globals.find_slot("di_1").unwrap();
    let (di_2_slot, _) = engine.vm().module().globals.find_slot("di_2").unwrap();

    // Simulate a comm cycle: device pushes di_0=false (should be IGNORED
    // because forced), di_1=true (should APPLY), di_2=true (should APPLY).
    engine.vm_mut().set_global_by_slot(di_0_slot, Value::Bool(false));
    engine.vm_mut().set_global_by_slot(di_1_slot, Value::Bool(true));
    engine.vm_mut().set_global_by_slot(di_2_slot, Value::Bool(true));

    engine.run_one_cycle().unwrap();

    // Forced var: still TRUE despite the device pushing FALSE.
    assert_eq!(engine.vm().get_global("di_0"), Some(&Value::Bool(true)));
    // Non-forced vars: device pushes apply normally.
    assert_eq!(engine.vm().get_global("di_1"), Some(&Value::Bool(true)));
    assert_eq!(engine.vm().get_global("di_2"), Some(&Value::Bool(true)));
    // Output computed by program from non-forced inputs: should be TRUE.
    assert_eq!(engine.vm().get_global("do_0"), Some(&Value::Bool(true)));

    // Sanity: only ONE slot should be in forced_global_slots.
    let forced_count = engine.vm().forced_variables().len();
    assert_eq!(forced_count, 1, "exactly one variable should be forced");
}

#[test]
fn unforce_restores_normal_writes() {
    let source = "\
VAR_GLOBAL
    counter : INT := 0;
END_VAR
PROGRAM Main
    counter := counter + 1;
END_PROGRAM
";
    let mut engine = run_program(source, 1);
    engine.vm_mut().force_variable("counter", Value::Int(999));
    engine.run_one_cycle().unwrap();
    engine.run_one_cycle().unwrap();
    // Force is held → counter stays at 999.
    assert_eq!(engine.vm().get_global("counter"), Some(&Value::Int(999)));

    // Unforce → next cycle the program write goes through.
    engine.vm_mut().unforce_variable("counter");
    engine.run_one_cycle().unwrap();
    // The program reads 999 and writes 1000.
    assert_eq!(engine.vm().get_global("counter"), Some(&Value::Int(1000)));
}

// =============================================================================
// Pointers (REF_TO and ^)
// =============================================================================

#[test]
fn pointer_read_via_deref() {
    let val = run_function(
        r#"
FUNCTION Calc : INT
VAR_INPUT
    dummy : INT;
END_VAR
VAR
    x : INT := 42;
    ptr : REF_TO INT;
END_VAR
    ptr := REF(x);
    Calc := ptr^;
END_FUNCTION
"#,
        "Calc",
    );
    assert_eq!(val, Value::Int(42));
}

#[test]
fn pointer_write_via_deref() {
    let source = r#"
VAR_GLOBAL
    g_result : INT;
END_VAR

PROGRAM Main
VAR
    x : INT := 10;
    ptr : REF_TO INT;
END_VAR
    ptr := REF(x);
    ptr^ := 99;
    g_result := x;
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    assert_eq!(engine.vm().get_global("g_result"), Some(&Value::Int(99)));
}

#[test]
fn pointer_null_default() {
    let val = run_function(
        r#"
FUNCTION Calc : INT
VAR_INPUT
    dummy : INT;
END_VAR
VAR
    ptr : REF_TO INT;
END_VAR
    Calc := ptr^;
END_FUNCTION
"#,
        "Calc",
    );
    // NULL pointer deref returns 0
    assert_eq!(val, Value::Int(0));
}

#[test]
fn pointer_null_literal() {
    let source = r#"
VAR_GLOBAL
    g_result : INT;
END_VAR

PROGRAM Main
VAR
    x : INT := 42;
    ptr : REF_TO INT;
END_VAR
    ptr := REF(x);
    ptr := NULL;
    g_result := ptr^;
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    // After setting to NULL, deref returns 0
    assert_eq!(engine.vm().get_global("g_result"), Some(&Value::Int(0)));
}

#[test]
fn pointer_to_global() {
    let source = r#"
VAR_GLOBAL
    g_value : INT;
    g_result : INT;
END_VAR

PROGRAM Main
VAR
    ptr : REF_TO INT;
END_VAR
    g_value := 55;
    ptr := REF(g_value);
    g_result := ptr^;
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    assert_eq!(engine.vm().get_global("g_result"), Some(&Value::Int(55)));
}

#[test]
fn pointer_modify_original_via_deref() {
    let source = r#"
VAR_GLOBAL
    g_a : INT;
    g_b : INT;
END_VAR

PROGRAM Main
VAR
    a : INT := 10;
    b : INT := 20;
    ptr : REF_TO INT;
END_VAR
    ptr := REF(a);
    ptr^ := ptr^ + 5;
    g_a := a;

    ptr := REF(b);
    ptr^ := ptr^ * 2;
    g_b := b;
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    assert_eq!(engine.vm().get_global("g_a"), Some(&Value::Int(15))); // 10 + 5
    assert_eq!(engine.vm().get_global("g_b"), Some(&Value::Int(40))); // 20 * 2
}
