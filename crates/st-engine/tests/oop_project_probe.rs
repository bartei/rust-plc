//! Probe the exact oop_project scenario: cross-file class methods storing to globals.

use st_ir::*;
use st_engine::*;

fn run_multi(sources: &[&str], cycles: u64) -> Engine {
    let parse_result = st_syntax::multi_file::parse_multi(sources);
    assert!(parse_result.errors.is_empty(), "Parse errors: {:?}", parse_result.errors);
    let module = st_compiler::compile(&parse_result.source_file).expect("Compile failed");
    let program_name = module.functions.iter()
        .find(|f| f.kind == PouKind::Program).expect("No PROGRAM").name.clone();
    let config = EngineConfig { max_cycles: cycles, ..Default::default() };
    let mut engine = Engine::new(module, program_name, config);
    engine.run().expect("Runtime error");
    engine
}

#[test]
fn sensor_get_raw_returns_value() {
    // Minimal reproduction: Sensor class + main program
    let sensor = r#"
CLASS Sensor
VAR
    _raw : INT := 0;
END_VAR
PUBLIC METHOD Update
VAR_INPUT rawValue : INT; END_VAR
    _raw := rawValue;
END_METHOD
PUBLIC METHOD GetRaw : INT
    GetRaw := _raw;
END_METHOD
END_CLASS
"#;
    let main = r#"
VAR_GLOBAL g_raw : INT; g_direct : INT; END_VAR
PROGRAM Main
VAR
    tempSensor : Sensor;
    simTemp : INT := 500;
END_VAR
    tempSensor.Update(rawValue := simTemp);
    g_raw := tempSensor.GetRaw();
    g_direct := simTemp;
END_PROGRAM
"#;
    let e = run_multi(&[sensor, main], 1);
    let g_raw = e.vm().get_global("g_raw");
    let g_direct = e.vm().get_global("g_direct");
    eprintln!("g_raw = {g_raw:?}, g_direct = {g_direct:?}");
    assert_eq!(g_direct, Some(&Value::Int(500)), "simTemp should be 500");
    assert_eq!(g_raw, Some(&Value::Int(500)), "GetRaw should return 500 after Update(500)");
}

#[test]
fn method_result_stored_in_global() {
    // Even simpler: class method returns constant
    let cls = r#"
CLASS Getter
METHOD GetVal : INT
    GetVal := 42;
END_METHOD
END_CLASS
"#;
    let main = r#"
VAR_GLOBAL g_val : INT; END_VAR
PROGRAM Main
VAR g : Getter; END_VAR
    g_val := g.GetVal();
END_PROGRAM
"#;
    let e = run_multi(&[cls, main], 1);
    assert_eq!(e.vm().get_global("g_val"), Some(&Value::Int(42)));
}

#[test]
fn full_oop_project_simulation() {
    // Replicate the actual oop_project structure
    let iface_controllable = r#"
INTERFACE IControllable
    METHOD Enable END_METHOD
    METHOD Disable END_METHOD
    METHOD IsEnabled : INT END_METHOD
END_INTERFACE
"#;
    let iface_resettable = r#"
INTERFACE IResettable
    METHOD Reset END_METHOD
END_INTERFACE
"#;
    let utils = r#"
FUNCTION Clamp : INT
VAR_INPUT val : INT; lo : INT; hi : INT; END_VAR
    IF val < lo THEN Clamp := lo;
    ELSIF val > hi THEN Clamp := hi;
    ELSE Clamp := val; END_IF;
END_FUNCTION
"#;
    let sensor = r#"
CLASS Sensor IMPLEMENTS IResettable
VAR
    _raw : INT := 0;
    _filtered : INT := 0;
    _count : INT := 0;
END_VAR
PUBLIC METHOD Update
VAR_INPUT rawValue : INT; END_VAR
VAR clamped : INT; END_VAR
    clamped := Clamp(val := rawValue, lo := 0, hi := 1000);
    _raw := clamped;
    IF _count = 0 THEN
        _filtered := clamped;
    ELSE
        _filtered := (25 * clamped + 75 * _filtered) / 100;
    END_IF;
    _count := _count + 1;
END_METHOD
PUBLIC METHOD GetFiltered : INT GetFiltered := _filtered; END_METHOD
PUBLIC METHOD GetRaw : INT GetRaw := _raw; END_METHOD
PUBLIC METHOD GetSampleCount : INT GetSampleCount := _count; END_METHOD
PUBLIC METHOD Reset
    _raw := 0; _filtered := 0; _count := 0;
END_METHOD
END_CLASS
"#;
    let controller = r#"
CLASS BaseController IMPLEMENTS IControllable
VAR
    _enabled : BOOL := FALSE;
    _output : INT := 0;
END_VAR
PUBLIC METHOD Enable _enabled := TRUE; END_METHOD
PUBLIC METHOD Disable _enabled := FALSE; _output := 0; END_METHOD
PUBLIC METHOD IsEnabled : INT
    IF _enabled THEN IsEnabled := 1; ELSE IsEnabled := 0; END_IF;
END_METHOD
PUBLIC METHOD GetOutput : INT GetOutput := _output; END_METHOD
END_CLASS

CLASS TempController EXTENDS BaseController IMPLEMENTS IResettable
VAR
    _setpoint : INT := 50;
    _gain : INT := 20;
    _outMin : INT := 0;
    _outMax : INT := 100;
END_VAR
PUBLIC METHOD Configure
VAR_INPUT sp : INT; gain : INT; oMin : INT; oMax : INT; END_VAR
    _setpoint := sp; _gain := gain; _outMin := oMin; _outMax := oMax;
END_METHOD
PUBLIC METHOD Compute
VAR_INPUT pv : INT; END_VAR
VAR error : INT; raw : INT; END_VAR
    IF _enabled THEN
        error := _setpoint - pv;
        raw := (error * _gain) / 10;
        _output := Clamp(val := raw, lo := _outMin, hi := _outMax);
    ELSE
        _output := 0;
    END_IF;
END_METHOD
PUBLIC METHOD GetSetpoint : INT GetSetpoint := _setpoint; END_METHOD
PUBLIC METHOD Reset _output := 0; _enabled := FALSE; END_METHOD
END_CLASS
"#;
    let main = r#"
VAR_GLOBAL
    g_raw_temp : INT;
    g_filtered_temp : INT;
    g_sensor_samples : INT;
    g_ctrl_output : INT;
    g_ctrl_enabled : INT;
    g_cycle : INT;
END_VAR

PROGRAM Main
VAR
    tempSensor : Sensor;
    controller : TempController;
    cycle : INT := 0;
    simTemp : INT := 200;
END_VAR
    cycle := cycle + 1;
    g_cycle := cycle;

    IF cycle = 1 THEN
        controller.Configure(sp := 500, gain := 15, oMin := 0, oMax := 100);
        controller.Enable();
    END_IF;

    simTemp := 200 + cycle * 20;

    tempSensor.Update(rawValue := simTemp);
    g_raw_temp := tempSensor.GetRaw();
    g_filtered_temp := tempSensor.GetFiltered();
    g_sensor_samples := tempSensor.GetSampleCount();

    controller.Compute(pv := g_filtered_temp);
    g_ctrl_output := controller.GetOutput();
    g_ctrl_enabled := controller.IsEnabled();
END_PROGRAM
"#;
    let e = run_multi(
        &[iface_controllable, iface_resettable, utils, sensor, controller, main],
        5,
    );

    let g_cycle = e.vm().get_global("g_cycle");
    let g_raw = e.vm().get_global("g_raw_temp");
    let g_filtered = e.vm().get_global("g_filtered_temp");
    let g_samples = e.vm().get_global("g_sensor_samples");
    let g_ctrl = e.vm().get_global("g_ctrl_output");
    let g_enabled = e.vm().get_global("g_ctrl_enabled");

    eprintln!("cycle={g_cycle:?} raw={g_raw:?} filtered={g_filtered:?} samples={g_samples:?} ctrl={g_ctrl:?} enabled={g_enabled:?}");

    assert_eq!(g_cycle, Some(&Value::Int(5)));
    // cycle 5: simTemp = 200 + 5*20 = 300
    assert_eq!(g_raw, Some(&Value::Int(300)), "raw should be 300 at cycle 5");
    assert!(g_samples == Some(&Value::Int(5)), "should have 5 samples");
    assert_eq!(g_enabled, Some(&Value::Int(1)), "controller should be enabled");
    // Controller: setpoint=500, pv=~300, error=200, raw=200*15/10=300, clamped to 100
    assert_eq!(g_ctrl, Some(&Value::Int(100)), "output should be clamped to 100");
}
