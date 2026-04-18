//! Tests for native (Rust-backed) function block dispatch.

use st_comm_api::native_fb::*;
use st_comm_api::FieldDataType;
use st_ir::Value;
use std::sync::Arc;

/// A mock native FB that increments a `count` field on each call.
struct CounterFb {
    layout: NativeFbLayout,
}

impl CounterFb {
    fn new() -> Self {
        Self {
            layout: NativeFbLayout {
                type_name: "Counter".to_string(),
                fields: vec![
                    NativeFbField {
                        name: "step".to_string(),
                        data_type: FieldDataType::Int,
                        var_kind: NativeFbVarKind::VarInput,
                    },
                    NativeFbField {
                        name: "count".to_string(),
                        data_type: FieldDataType::Int,
                        var_kind: NativeFbVarKind::Var,
                    },
                ],
            },
        }
    }
}

impl NativeFb for CounterFb {
    fn type_name(&self) -> &str {
        &self.layout.type_name
    }
    fn layout(&self) -> &NativeFbLayout {
        &self.layout
    }
    fn execute(&self, fields: &mut [Value]) {
        let step = fields[0].as_int();
        let count = fields[1].as_int();
        fields[1] = Value::Int(count + if step == 0 { 1 } else { step });
    }
}

/// Compile with stdlib + native FB registry, run N cycles, return VM.
fn run_counter_program(source: &str, cycles: usize) -> st_engine::vm::Vm {
    let mut reg = st_comm_api::NativeFbRegistry::new();
    reg.register(Box::new(CounterFb::new()));

    let stdlib = st_syntax::multi_file::builtin_stdlib();
    let mut all: Vec<&str> = stdlib;
    all.push(source);
    let parse_result = st_syntax::multi_file::parse_multi(&all);
    assert!(
        parse_result.errors.is_empty(),
        "Parse errors: {:?}",
        parse_result.errors
    );

    let module = st_compiler::compile_with_native_fbs(&parse_result.source_file, Some(&reg))
        .expect("Compilation failed");

    let arc_reg = Arc::new(reg);
    let mut vm = st_engine::vm::Vm::new_with_native_fbs(
        module,
        st_engine::vm::VmConfig::default(),
        Some(arc_reg),
    );
    let _ = vm.run_global_init();
    for _ in 0..cycles {
        vm.scan_cycle("Main").expect("Scan cycle failed");
    }
    vm
}

#[test]
fn native_fb_execute_called() {
    let vm = run_counter_program(
        r#"
VAR_GLOBAL
    result : INT := 0;
END_VAR

PROGRAM Main
VAR
    c : Counter;
END_VAR
    c();
    result := c.count;
END_PROGRAM
"#,
        1,
    );
    assert_eq!(vm.get_global("result"), Some(&Value::Int(1)));
}

#[test]
fn native_fb_state_persists_across_cycles() {
    let vm = run_counter_program(
        r#"
VAR_GLOBAL
    result : INT := 0;
END_VAR

PROGRAM Main
VAR
    c : Counter;
END_VAR
    c();
    result := c.count;
END_PROGRAM
"#,
        5,
    );
    assert_eq!(vm.get_global("result"), Some(&Value::Int(5)));
}

#[test]
fn native_fb_input_params() {
    let vm = run_counter_program(
        r#"
VAR_GLOBAL
    result : INT := 0;
END_VAR

PROGRAM Main
VAR
    c : Counter;
END_VAR
    c(step := 10);
    result := c.count;
END_PROGRAM
"#,
        3,
    );
    // step=10, so after 3 cycles: 10+10+10 = 30
    assert_eq!(vm.get_global("result"), Some(&Value::Int(30)));
}

#[test]
fn native_fb_field_write() {
    let vm = run_counter_program(
        r#"
VAR_GLOBAL
    result : INT := 0;
END_VAR

PROGRAM Main
VAR
    c : Counter;
END_VAR
    c.count := 100;
    c();
    result := c.count;
END_PROGRAM
"#,
        1,
    );
    // Set count to 100, then execute (step=0 → +1), so result = 101
    assert_eq!(vm.get_global("result"), Some(&Value::Int(101)));
}

#[test]
fn native_fb_multiple_instances() {
    let vm = run_counter_program(
        r#"
VAR_GLOBAL
    ra : INT := 0;
    rb : INT := 0;
END_VAR

PROGRAM Main
VAR
    a : Counter;
    b : Counter;
END_VAR
    a(step := 1);
    b(step := 5);
    ra := a.count;
    rb := b.count;
END_PROGRAM
"#,
        3,
    );
    // a: 1+1+1=3, b: 5+5+5=15
    assert_eq!(vm.get_global("ra"), Some(&Value::Int(3)));
    assert_eq!(vm.get_global("rb"), Some(&Value::Int(15)));
}
