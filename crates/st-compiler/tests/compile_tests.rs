//! Compiler tests: parse ST source → compile to IR → verify bytecode.
#![allow(clippy::approx_constant)]

use st_compiler::compile;
use st_ir::*;

fn compile_ok(source: &str) -> Module {
    let result = st_syntax::parse(source);
    assert!(result.errors.is_empty(), "Parse errors: {:?}", result.errors);
    compile(&result.source_file).expect("Compilation failed")
}

fn find_func<'a>(module: &'a Module, name: &str) -> &'a Function {
    module
        .find_function(name)
        .unwrap_or_else(|| panic!("Function '{name}' not found"))
        .1
}

// =============================================================================
// Basic compilation
// =============================================================================

#[test]
fn compile_empty_program() {
    let m = compile_ok("PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := 1;\nEND_PROGRAM\n");
    assert_eq!(m.functions.len(), 1);
    let f = find_func(&m, "Main");
    assert_eq!(f.kind, PouKind::Program);
    assert!(!f.instructions.is_empty());
    assert!(matches!(f.instructions.last(), Some(Instruction::RetVoid)));
}

#[test]
fn compile_function_with_return() {
    let m = compile_ok(
        "FUNCTION Add : INT\nVAR_INPUT\n    a : INT;\n    b : INT;\nEND_VAR\n    Add := a + b;\nEND_FUNCTION\n",
    );
    let f = find_func(&m, "Add");
    assert_eq!(f.kind, PouKind::Function);
    assert!(matches!(f.instructions.last(), Some(Instruction::Ret(_))));
}

#[test]
fn compile_function_block() {
    let m = compile_ok(
        "FUNCTION_BLOCK Counter\nVAR_INPUT\n    reset : BOOL;\nEND_VAR\nVAR\n    val : INT := 0;\nEND_VAR\n    IF reset THEN\n        val := 0;\n    ELSE\n        val := val + 1;\n    END_IF;\nEND_FUNCTION_BLOCK\n",
    );
    let f = find_func(&m, "Counter");
    assert_eq!(f.kind, PouKind::FunctionBlock);
    assert!(matches!(f.instructions.last(), Some(Instruction::RetVoid)));
}

#[test]
fn compile_multiple_pous() {
    let m = compile_ok(
        "FUNCTION Add : INT\nVAR_INPUT\n    a : INT;\n    b : INT;\nEND_VAR\n    Add := a + b;\nEND_FUNCTION\n\nPROGRAM Main\nVAR\n    result : INT := 0;\nEND_VAR\n    result := Add(a := 1, b := 2);\nEND_PROGRAM\n",
    );
    assert_eq!(m.functions.len(), 2);
    assert!(m.find_function("Add").is_some());
    assert!(m.find_function("Main").is_some());
}

// =============================================================================
// Variable declarations and initialization
// =============================================================================

#[test]
fn compile_var_with_init() {
    let m = compile_ok("PROGRAM Main\nVAR\n    x : INT := 42;\nEND_VAR\n    x := x + 1;\nEND_PROGRAM\n");
    let f = find_func(&m, "Main");
    assert!(!f.locals.slots.is_empty());
    assert_eq!(f.locals.slots[0].name, "x");
    // Should have a LoadConst(42) + StoreLocal for initialization
    let has_init = f.instructions.iter().any(|i| matches!(i, Instruction::LoadConst(_, Value::Int(42))));
    assert!(has_init, "Expected initialization with 42");
}

#[test]
fn compile_multiple_vars() {
    let m = compile_ok("PROGRAM Main\nVAR\n    a : INT := 1;\n    b : REAL := 3.14;\n    c : BOOL := TRUE;\nEND_VAR\n    a := a + 1;\nEND_PROGRAM\n");
    let f = find_func(&m, "Main");
    assert_eq!(f.locals.slots.len(), 3);
    assert_eq!(f.locals.slots[0].ty, VarType::Int);
    assert_eq!(f.locals.slots[1].ty, VarType::Real);
    assert_eq!(f.locals.slots[2].ty, VarType::Bool);
}

// =============================================================================
// Expressions
// =============================================================================

#[test]
fn compile_arithmetic() {
    let m = compile_ok("PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := 1 + 2 * 3;\nEND_PROGRAM\n");
    let f = find_func(&m, "Main");
    let has_add = f.instructions.iter().any(|i| matches!(i, Instruction::Add(_, _, _)));
    let has_mul = f.instructions.iter().any(|i| matches!(i, Instruction::Mul(_, _, _)));
    assert!(has_add, "Expected Add instruction");
    assert!(has_mul, "Expected Mul instruction");
}

#[test]
fn compile_comparison() {
    let m = compile_ok("PROGRAM Main\nVAR\n    x : INT := 5;\n    b : BOOL := FALSE;\nEND_VAR\n    b := x > 3;\nEND_PROGRAM\n");
    let f = find_func(&m, "Main");
    let has_cmp = f.instructions.iter().any(|i| matches!(i, Instruction::CmpGt(_, _, _)));
    assert!(has_cmp);
}

#[test]
fn compile_boolean_logic() {
    let m = compile_ok("PROGRAM Main\nVAR\n    a : BOOL := TRUE;\n    b : BOOL := FALSE;\n    c : BOOL := FALSE;\nEND_VAR\n    c := a AND b;\n    c := a OR b;\n    c := NOT a;\nEND_PROGRAM\n");
    let f = find_func(&m, "Main");
    assert!(f.instructions.iter().any(|i| matches!(i, Instruction::And(_, _, _))));
    assert!(f.instructions.iter().any(|i| matches!(i, Instruction::Or(_, _, _))));
    assert!(f.instructions.iter().any(|i| matches!(i, Instruction::Not(_, _))));
}

#[test]
fn compile_unary_neg() {
    let m = compile_ok("PROGRAM Main\nVAR\n    x : INT := 5;\nEND_VAR\n    x := -x;\nEND_PROGRAM\n");
    let f = find_func(&m, "Main");
    assert!(f.instructions.iter().any(|i| matches!(i, Instruction::Neg(_, _))));
}

#[test]
fn compile_power() {
    let m = compile_ok("PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := 2 ** 3;\nEND_PROGRAM\n");
    let f = find_func(&m, "Main");
    assert!(f.instructions.iter().any(|i| matches!(i, Instruction::Pow(_, _, _))));
}

#[test]
fn compile_mod_operation() {
    let m = compile_ok("PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := 10 MOD 3;\nEND_PROGRAM\n");
    let f = find_func(&m, "Main");
    assert!(f.instructions.iter().any(|i| matches!(i, Instruction::Mod(_, _, _))));
}

#[test]
fn compile_all_comparison_ops() {
    let m = compile_ok("PROGRAM Main\nVAR\n    a : INT := 1;\n    b : INT := 2;\n    r : BOOL := FALSE;\nEND_VAR\n    r := a = b;\n    r := a <> b;\n    r := a < b;\n    r := a > b;\n    r := a <= b;\n    r := a >= b;\nEND_PROGRAM\n");
    let f = find_func(&m, "Main");
    assert!(f.instructions.iter().any(|i| matches!(i, Instruction::CmpEq(_, _, _))));
    assert!(f.instructions.iter().any(|i| matches!(i, Instruction::CmpNe(_, _, _))));
    assert!(f.instructions.iter().any(|i| matches!(i, Instruction::CmpLt(_, _, _))));
    assert!(f.instructions.iter().any(|i| matches!(i, Instruction::CmpGt(_, _, _))));
    assert!(f.instructions.iter().any(|i| matches!(i, Instruction::CmpLe(_, _, _))));
    assert!(f.instructions.iter().any(|i| matches!(i, Instruction::CmpGe(_, _, _))));
}

// =============================================================================
// Control flow
// =============================================================================

#[test]
fn compile_if_else() {
    let m = compile_ok("PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    IF x > 0 THEN\n        x := 1;\n    ELSE\n        x := 2;\n    END_IF;\nEND_PROGRAM\n");
    let f = find_func(&m, "Main");
    let has_jmpifnot = f.instructions.iter().any(|i| matches!(i, Instruction::JumpIfNot(_, _)));
    let has_jmp = f.instructions.iter().any(|i| matches!(i, Instruction::Jump(_)));
    assert!(has_jmpifnot, "Expected JumpIfNot for IF condition");
    assert!(has_jmp, "Expected Jump for ELSE branch");
}

#[test]
fn compile_if_elsif() {
    let m = compile_ok("PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    IF x > 10 THEN\n        x := 1;\n    ELSIF x > 5 THEN\n        x := 2;\n    ELSE\n        x := 3;\n    END_IF;\nEND_PROGRAM\n");
    let f = find_func(&m, "Main");
    // Should have multiple JumpIfNot for IF and ELSIF conditions
    let jmpifnot_count = f.instructions.iter().filter(|i| matches!(i, Instruction::JumpIfNot(_, _))).count();
    assert!(jmpifnot_count >= 2, "Expected at least 2 JumpIfNot for IF+ELSIF");
}

#[test]
fn compile_for_loop() {
    let m = compile_ok("PROGRAM Main\nVAR\n    i : INT;\n    sum : INT := 0;\nEND_VAR\n    FOR i := 1 TO 10 DO\n        sum := sum + i;\n    END_FOR;\nEND_PROGRAM\n");
    let f = find_func(&m, "Main");
    // Should have: StoreLocal (init i), CmpLe (condition), JumpIfNot (exit), Add (body + increment), Jump (loop back)
    assert!(f.instructions.iter().any(|i| matches!(i, Instruction::CmpLe(_, _, _))));
    let jump_count = f.instructions.iter().filter(|i| matches!(i, Instruction::Jump(_))).count();
    assert!(jump_count >= 1, "Expected jump for loop back");
}

#[test]
fn compile_for_with_step() {
    let m = compile_ok("PROGRAM Main\nVAR\n    i : INT;\n    sum : INT := 0;\nEND_VAR\n    FOR i := 0 TO 100 BY 5 DO\n        sum := sum + 1;\n    END_FOR;\nEND_PROGRAM\n");
    let f = find_func(&m, "Main");
    // Should have a LoadConst(5) for the step
    let has_five = f.instructions.iter().any(|i| matches!(i, Instruction::LoadConst(_, Value::Int(5))));
    assert!(has_five, "Expected step value 5");
}

#[test]
fn compile_while_loop() {
    let m = compile_ok("PROGRAM Main\nVAR\n    x : INT := 10;\nEND_VAR\n    WHILE x > 0 DO\n        x := x - 1;\n    END_WHILE;\nEND_PROGRAM\n");
    let f = find_func(&m, "Main");
    assert!(f.instructions.iter().any(|i| matches!(i, Instruction::CmpGt(_, _, _))));
    assert!(f.instructions.iter().any(|i| matches!(i, Instruction::JumpIfNot(_, _))));
}

#[test]
fn compile_repeat_loop() {
    let m = compile_ok("PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    REPEAT\n        x := x + 1;\n    UNTIL x >= 10\n    END_REPEAT;\nEND_PROGRAM\n");
    let f = find_func(&m, "Main");
    assert!(f.instructions.iter().any(|i| matches!(i, Instruction::CmpGe(_, _, _))));
    assert!(f.instructions.iter().any(|i| matches!(i, Instruction::JumpIfNot(_, _))));
}

#[test]
fn compile_case_statement() {
    let m = compile_ok("PROGRAM Main\nVAR\n    mode : INT := 1;\n    x : INT := 0;\nEND_VAR\n    CASE mode OF\n        1:\n            x := 10;\n        2:\n            x := 20;\n    END_CASE;\nEND_PROGRAM\n");
    let f = find_func(&m, "Main");
    // Should have CmpEq for each case selector
    let cmp_eq_count = f.instructions.iter().filter(|i| matches!(i, Instruction::CmpEq(_, _, _))).count();
    assert!(cmp_eq_count >= 2, "Expected CmpEq for each CASE selector");
}

#[test]
fn compile_case_with_range() {
    let m = compile_ok("PROGRAM Main\nVAR\n    x : INT := 50;\n    cat : INT := 0;\nEND_VAR\n    CASE x OF\n        1..10:\n            cat := 1;\n        11..100:\n            cat := 2;\n    END_CASE;\nEND_PROGRAM\n");
    let f = find_func(&m, "Main");
    // Range needs CmpGe + CmpLe + And
    assert!(f.instructions.iter().any(|i| matches!(i, Instruction::CmpGe(_, _, _))));
    assert!(f.instructions.iter().any(|i| matches!(i, Instruction::CmpLe(_, _, _))));
}

#[test]
fn compile_exit_in_loop() {
    let m = compile_ok("PROGRAM Main\nVAR\n    i : INT;\nEND_VAR\n    FOR i := 1 TO 100 DO\n        IF i > 50 THEN\n            EXIT;\n        END_IF;\n    END_FOR;\nEND_PROGRAM\n");
    let f = find_func(&m, "Main");
    // EXIT generates a Jump to the loop exit label
    let jump_count = f.instructions.iter().filter(|i| matches!(i, Instruction::Jump(_))).count();
    assert!(jump_count >= 2, "Expected jumps for EXIT + loop back");
}

#[test]
fn compile_return_statement() {
    let m = compile_ok("FUNCTION Foo : INT\nVAR_INPUT\n    x : INT;\nEND_VAR\n    Foo := x;\n    RETURN;\n    Foo := 0;\nEND_FUNCTION\n");
    let f = find_func(&m, "Foo");
    // Should have a Ret instruction from the RETURN statement
    let ret_count = f.instructions.iter().filter(|i| matches!(i, Instruction::Ret(_))).count();
    assert!(ret_count >= 2, "Expected at least 2 Ret (RETURN + end)");
}

// =============================================================================
// Function calls
// =============================================================================

#[test]
fn compile_function_call() {
    let m = compile_ok(
        "FUNCTION Add : INT\nVAR_INPUT\n    a : INT;\n    b : INT;\nEND_VAR\n    Add := a + b;\nEND_FUNCTION\n\nPROGRAM Main\nVAR\n    r : INT := 0;\nEND_VAR\n    r := Add(a := 1, b := 2);\nEND_PROGRAM\n",
    );
    let main = find_func(&m, "Main");
    let has_call = main.instructions.iter().any(|i| matches!(i, Instruction::Call { .. }));
    assert!(has_call, "Expected Call instruction");
}

#[test]
fn compile_function_call_positional() {
    let m = compile_ok(
        "FUNCTION Square : INT\nVAR_INPUT\n    x : INT;\nEND_VAR\n    Square := x * x;\nEND_FUNCTION\n\nPROGRAM Main\nVAR\n    r : INT := 0;\nEND_VAR\n    r := Square(5);\nEND_PROGRAM\n",
    );
    let main = find_func(&m, "Main");
    assert!(main.instructions.iter().any(|i| matches!(i, Instruction::Call { .. })));
}

// =============================================================================
// Source map
// =============================================================================

#[test]
fn compile_source_map_populated() {
    let m = compile_ok("PROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    x := 42;\nEND_PROGRAM\n");
    let f = find_func(&m, "Main");
    assert_eq!(f.source_map.len(), f.instructions.len());
    // The assignment `x := 42` should have a non-zero source location
    let has_sourced = f.source_map.iter().any(|s| s.byte_offset > 0);
    assert!(has_sourced, "Expected at least one sourced instruction");
}

// =============================================================================
// Global variables
// =============================================================================

#[test]
fn compile_global_vars() {
    let m = compile_ok(
        "VAR_GLOBAL\n    g_count : INT;\nEND_VAR\n\nPROGRAM Main\nVAR\n    x : INT := 0;\nEND_VAR\n    g_count := g_count + 1;\n    x := g_count;\nEND_PROGRAM\n",
    );
    assert!(!m.globals.slots.is_empty());
    assert_eq!(m.globals.slots[0].name, "g_count");
    let main = find_func(&m, "Main");
    assert!(main.instructions.iter().any(|i| matches!(i, Instruction::LoadGlobal(_, _))));
    assert!(main.instructions.iter().any(|i| matches!(i, Instruction::StoreGlobal(_, _))));
}

// =============================================================================
// Literals
// =============================================================================

#[test]
fn compile_bool_literals() {
    let m = compile_ok("PROGRAM Main\nVAR\n    b : BOOL := FALSE;\nEND_VAR\n    b := TRUE;\nEND_PROGRAM\n");
    let f = find_func(&m, "Main");
    assert!(f.instructions.iter().any(|i| matches!(i, Instruction::LoadConst(_, Value::Bool(true)))));
}

#[test]
fn compile_real_literal() {
    let m = compile_ok("PROGRAM Main\nVAR\n    r : REAL := 0.0;\nEND_VAR\n    r := 3.14;\nEND_PROGRAM\n");
    let f = find_func(&m, "Main");
    let has_real = f.instructions.iter().any(|i| {
        matches!(i, Instruction::LoadConst(_, Value::Real(v)) if (*v - 3.14).abs() < 0.001)
    });
    assert!(has_real, "Expected LoadConst(3.14)");
}

#[test]
fn compile_string_literal() {
    let m = compile_ok("PROGRAM Main\nVAR\n    s : STRING[80];\nEND_VAR\n    s := 'hello';\nEND_PROGRAM\n");
    let f = find_func(&m, "Main");
    let has_str = f.instructions.iter().any(|i| {
        matches!(i, Instruction::LoadConst(_, Value::String(s)) if s == "hello")
    });
    assert!(has_str, "Expected LoadConst('hello')");
}

// =============================================================================
// IR types
// =============================================================================

#[test]
fn value_conversions() {
    assert!(Value::Bool(true).as_bool());
    assert!(!Value::Bool(false).as_bool());
    assert_eq!(Value::Int(42).as_int(), 42);
    assert_eq!(Value::Real(3.14).as_real(), 3.14);
    assert_eq!(Value::Int(5).as_real(), 5.0);
    assert_eq!(Value::UInt(10).as_int(), 10);
    assert_eq!(Value::default(), Value::Int(0));
}

#[test]
fn value_defaults_for_types() {
    assert_eq!(Value::default_for_type(VarType::Bool), Value::Bool(false));
    assert_eq!(Value::default_for_type(VarType::Int), Value::Int(0));
    assert_eq!(Value::default_for_type(VarType::UInt), Value::UInt(0));
    assert_eq!(Value::default_for_type(VarType::Real), Value::Real(0.0));
    assert_eq!(Value::default_for_type(VarType::String), Value::String(String::new()));
    assert_eq!(Value::default_for_type(VarType::Time), Value::Time(0));
    assert_eq!(Value::default_for_type(VarType::FbInstance(0)), Value::Void);
}

#[test]
fn var_type_sizes() {
    assert_eq!(VarType::Bool.size(), 1);
    assert_eq!(VarType::Int.size(), 8);
    assert_eq!(VarType::Real.size(), 8);
    assert_eq!(VarType::String.size(), 24);
    assert_eq!(VarType::FbInstance(0).size(), 0);
}

#[test]
fn memory_layout_find_and_size() {
    let layout = MemoryLayout {
        slots: vec![
            VarSlot { name: "x".into(), ty: VarType::Int, offset: 0, size: 8, retain: false, int_width: IntWidth::None },
            VarSlot { name: "y".into(), ty: VarType::Real, offset: 8, size: 8, retain: false, int_width: IntWidth::None },
        ],
    };
    assert_eq!(layout.find_slot("x").unwrap().0, 0);
    assert_eq!(layout.find_slot("Y").unwrap().0, 1); // case insensitive
    assert!(layout.find_slot("z").is_none());
    assert_eq!(layout.total_size(), 16);
}

#[test]
fn module_find_function() {
    let m = compile_ok("FUNCTION Foo : INT\nVAR_INPUT\n    x : INT;\nEND_VAR\n    Foo := x;\nEND_FUNCTION\n\nPROGRAM Main\nVAR\n    r : INT := 0;\nEND_VAR\n    r := Foo(x := 1);\nEND_PROGRAM\n");
    assert!(m.find_function("Foo").is_some());
    assert!(m.find_function("foo").is_some()); // case insensitive
    assert!(m.find_function("Bar").is_none());
}

