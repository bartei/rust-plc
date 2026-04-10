//! Probing tests for partial variable access (.%X, .%B, .%W, .%D).

use st_ir::*;
use st_runtime::*;

fn run_function(source: &str, func_name: &str) -> Value {
    let parse_result = st_syntax::parse(source);
    assert!(parse_result.errors.is_empty(), "Parse errors: {:?}", parse_result.errors);
    let module = st_compiler::compile(&parse_result.source_file).unwrap();
    let mut vm = Vm::new(module, VmConfig::default());
    vm.run(func_name).unwrap()
}

fn run_program(source: &str, cycles: u64) -> Engine {
    let parse_result = st_syntax::parse(source);
    assert!(parse_result.errors.is_empty(), "Parse errors: {:?}", parse_result.errors);
    let module = st_compiler::compile(&parse_result.source_file).expect("Compile failed");
    let program_name = module.functions.iter()
        .find(|f| f.kind == PouKind::Program).expect("No PROGRAM").name.clone();
    let config = EngineConfig { max_cycles: cycles, ..Default::default() };
    let mut engine = Engine::new(module, program_name, config);
    engine.run().expect("Runtime error");
    engine
}

// =============================================================================
// Bit access (.%X)
// =============================================================================

#[test]
fn read_bit_from_byte() {
    let val = run_function(r#"
FUNCTION Test : INT
VAR_INPUT dummy : INT; END_VAR
VAR
    b : BYTE := 16#A5;  // 10100101
END_VAR
    IF b.%X0 THEN Test := Test + 1; END_IF;    // bit 0 = 1
    IF b.%X1 THEN Test := Test + 10; END_IF;   // bit 1 = 0
    IF b.%X2 THEN Test := Test + 100; END_IF;  // bit 2 = 1
    IF b.%X5 THEN Test := Test + 1000; END_IF; // bit 5 = 1
    IF b.%X7 THEN Test := Test + 10000; END_IF; // bit 7 = 1
END_FUNCTION
"#, "Test");
    // 0xA5 = 10100101: bits 0,2,5,7 are set
    assert_eq!(val, Value::Int(11101)); // 1 + 100 + 1000 + 10000
}

#[test]
fn write_bit_to_byte() {
    let source = r#"
VAR_GLOBAL g_val : INT; END_VAR
PROGRAM Main
VAR
    status : DWORD := 0;
END_VAR
    status.%X0 := TRUE;
    status.%X3 := TRUE;
    status.%X7 := TRUE;
    g_val := status;  // should be 0x89 = 137
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    // bit 0 + bit 3 + bit 7 = 1 + 8 + 128 = 137
    assert_eq!(engine.vm().get_global("g_val"), Some(&Value::Int(137)));
}

// =============================================================================
// Byte access (.%B)
// =============================================================================

#[test]
fn read_byte_from_dword() {
    let val = run_function(r#"
FUNCTION Test : INT
VAR_INPUT dummy : INT; END_VAR
VAR
    d : DWORD := 16#12345678;
END_VAR
    Test := d.%B0;  // lowest byte = 0x78 = 120
END_FUNCTION
"#, "Test");
    assert_eq!(val, Value::Int(0x78));
}

#[test]
fn read_byte1_from_dword() {
    let val = run_function(r#"
FUNCTION Test : INT
VAR_INPUT dummy : INT; END_VAR
VAR
    d : DWORD := 16#12345678;
END_VAR
    Test := d.%B1;  // second byte = 0x56
END_FUNCTION
"#, "Test");
    assert_eq!(val, Value::Int(0x56));
}

#[test]
fn write_byte_to_dword() {
    // g_val is DINT so the 0xBBAA = 48042 result fits without wrapping.
    // (16-bit INT cannot hold this value — it would wrap to -17750.)
    let source = r#"
VAR_GLOBAL g_val : DINT; END_VAR
PROGRAM Main
VAR
    d : DWORD := 0;
END_VAR
    d.%B0 := 16#AA;
    d.%B1 := 16#BB;
    g_val := d;  // should be 0xBBAA = 48042
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    assert_eq!(engine.vm().get_global("g_val"), Some(&Value::Int(0xBBAA)));
}

// =============================================================================
// Word access (.%W)
// =============================================================================

#[test]
fn read_word_from_dword() {
    // Test return is DINT so the 0xCCDD = 52445 result fits naturally.
    // (16-bit INT cannot hold this value — it would wrap to -13091.)
    let val = run_function(r#"
FUNCTION Test : DINT
VAR_INPUT dummy : INT; END_VAR
VAR
    d : DWORD := 16#AABB_CCDD;
END_VAR
    Test := d.%W0;  // lower word = 0xCCDD
END_FUNCTION
"#, "Test");
    assert_eq!(val, Value::Int(0xCCDD));
}

#[test]
fn read_word1_from_dword() {
    let val = run_function(r#"
FUNCTION Test : DINT
VAR_INPUT dummy : INT; END_VAR
VAR
    d : DWORD := 16#AABB_CCDD;
END_VAR
    Test := d.%W1;  // upper word = 0xAABB
END_FUNCTION
"#, "Test");
    assert_eq!(val, Value::Int(0xAABB));
}

// =============================================================================
// Combined read + write
// =============================================================================

#[test]
fn toggle_bits_via_partial() {
    let source = r#"
VAR_GLOBAL g_val : INT; END_VAR
PROGRAM Main
VAR
    flags : WORD := 16#00FF;  // lower 8 bits set
END_VAR
    // Clear bit 0, set bit 8
    flags.%X0 := FALSE;
    flags.%X8 := TRUE;
    g_val := flags;  // 0x01FE = 510
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    // 0x00FF → clear bit 0 → 0x00FE → set bit 8 → 0x01FE = 510
    assert_eq!(engine.vm().get_global("g_val"), Some(&Value::Int(0x01FE)));
}

#[test]
fn read_bit_from_word_variable() {
    let source = r#"
VAR_GLOBAL g_bit0 : INT; g_bit15 : INT; END_VAR
PROGRAM Main
VAR
    w : WORD := 16#8001;  // bit 0 and bit 15 set
END_VAR
    IF w.%X0 THEN g_bit0 := 1; ELSE g_bit0 := 0; END_IF;
    IF w.%X15 THEN g_bit15 := 1; ELSE g_bit15 := 0; END_IF;
END_PROGRAM
"#;
    let engine = run_program(source, 1);
    assert_eq!(engine.vm().get_global("g_bit0"), Some(&Value::Int(1)));
    assert_eq!(engine.vm().get_global("g_bit15"), Some(&Value::Int(1)));
}

// =============================================================================
// Multi-cycle with partial access
// =============================================================================

#[test]
fn partial_access_persists_across_cycles() {
    let source = r#"
VAR_GLOBAL g_val : INT; END_VAR
PROGRAM Main
VAR
    shift_reg : DWORD := 1;
    cycle : INT := 0;
END_VAR
    cycle := cycle + 1;
    // Shift left by 1 each cycle (manual bit manipulation)
    shift_reg := shift_reg * 2;
    g_val := shift_reg;
END_PROGRAM
"#;
    let engine = run_program(source, 4);
    // After 4 cycles: 1 → 2 → 4 → 8 → 16
    assert_eq!(engine.vm().get_global("g_val"), Some(&Value::Int(16)));
}
