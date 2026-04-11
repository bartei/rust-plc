//! Verification tests for playground/13_structs_and_pointers.st patterns.

use st_ir::*;
use st_engine::*;

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
// AnalogInput: field-based I/O with scaling
// =============================================================================

#[test]
fn analog_input_scaling() {
    let source = r#"
FUNCTION_BLOCK AnalogInput
VAR_INPUT
    rawValue : INT;
    scaleLow : INT;
    scaleHigh : INT;
END_VAR
VAR_OUTPUT
    scaled : INT;
END_VAR
    scaled := scaleLow + (rawValue * (scaleHigh - scaleLow)) / 1000;
END_FUNCTION_BLOCK

VAR_GLOBAL g_s1 : INT; g_s2 : INT; g_s3 : INT; END_VAR
PROGRAM Main
VAR ai : AnalogInput; END_VAR
    // 0 raw → 0 scaled
    ai.rawValue := 0; ai.scaleLow := 0; ai.scaleHigh := 100; ai();
    g_s1 := ai.scaled;

    // 500 raw → 50 scaled (midpoint)
    ai.rawValue := 500; ai();
    g_s2 := ai.scaled;

    // 1000 raw → 100 scaled (full scale)
    ai.rawValue := 1000; ai();
    g_s3 := ai.scaled;
END_PROGRAM
"#;
    let e = run_program(source, 1);
    assert_eq!(e.vm().get_global("g_s1"), Some(&Value::Int(0)));
    assert_eq!(e.vm().get_global("g_s2"), Some(&Value::Int(50)));
    assert_eq!(e.vm().get_global("g_s3"), Some(&Value::Int(100)));
}

// =============================================================================
// Calculator: multiple operations via field assignment
// =============================================================================

#[test]
fn calculator_field_io() {
    let source = r#"
FUNCTION_BLOCK Calculator
VAR_INPUT a : INT; b : INT; op : INT; END_VAR
VAR_OUTPUT result : INT; error : BOOL; END_VAR
    error := FALSE;
    IF op = 0 THEN result := a + b;
    ELSIF op = 1 THEN result := a - b;
    ELSIF op = 2 THEN result := a * b;
    ELSIF op = 3 THEN
        IF b <> 0 THEN result := a / b;
        ELSE result := 0; error := TRUE; END_IF;
    ELSE result := 0; error := TRUE; END_IF;
END_FUNCTION_BLOCK

VAR_GLOBAL g_add : INT; g_sub : INT; g_mul : INT; g_div : INT; END_VAR
PROGRAM Main
VAR c : Calculator; END_VAR
    c.a := 100; c.b := 7;

    c.op := 0; c(); g_add := c.result;
    c.op := 1; c(); g_sub := c.result;
    c.op := 2; c(); g_mul := c.result;
    c.op := 3; c(); g_div := c.result;
END_PROGRAM
"#;
    let e = run_program(source, 1);
    assert_eq!(e.vm().get_global("g_add"), Some(&Value::Int(107)));
    assert_eq!(e.vm().get_global("g_sub"), Some(&Value::Int(93)));
    assert_eq!(e.vm().get_global("g_mul"), Some(&Value::Int(700)));
    assert_eq!(e.vm().get_global("g_div"), Some(&Value::Int(14)));  // integer division
}

// =============================================================================
// Alarm class with pointer-based out-param
// =============================================================================

#[test]
fn alarm_with_pointer_out_param() {
    let source = r#"
CLASS Alarm
VAR
    _active : BOOL := FALSE;
    _tripCount : INT := 0;
    _highLimit : INT := 80;
    _hysteresis : INT := 5;
END_VAR
PUBLIC METHOD Configure
VAR_INPUT limit : INT; hyst : INT; END_VAR
    _highLimit := limit; _hysteresis := hyst;
END_METHOD
PUBLIC METHOD Evaluate
VAR_INPUT value : INT; END_VAR
VAR wasActive : BOOL; END_VAR
    wasActive := _active;
    IF value > _highLimit THEN _active := TRUE;
    ELSIF value < (_highLimit - _hysteresis) THEN _active := FALSE; END_IF;
    IF _active AND NOT wasActive THEN _tripCount := _tripCount + 1; END_IF;
END_METHOD
PUBLIC METHOD WriteStatus
VAR_INPUT pActive : REF_TO INT; pTrips : REF_TO INT; END_VAR
    IF pActive <> NULL THEN
        IF _active THEN pActive^ := 1; ELSE pActive^ := 0; END_IF;
    END_IF;
    IF pTrips <> NULL THEN pTrips^ := _tripCount; END_IF;
END_METHOD
END_CLASS

VAR_GLOBAL g_active : INT; g_trips : INT; END_VAR
PROGRAM Main
VAR
    alarm : Alarm;
    status : INT := 0;
    trips : INT := 0;
END_VAR
    alarm.Configure(limit := 50, hyst := 5);

    // Below limit
    alarm.Evaluate(value := 30);
    alarm.WriteStatus(pActive := REF(status), pTrips := REF(trips));
    g_active := status;
    g_trips := trips;
END_PROGRAM
"#;
    let e = run_program(source, 1);
    assert_eq!(e.vm().get_global("g_active"), Some(&Value::Int(0)), "30 < 50: not active");
    assert_eq!(e.vm().get_global("g_trips"), Some(&Value::Int(0)), "no trips yet");
}

#[test]
fn alarm_trips_on_high_value() {
    let source = r#"
CLASS Alarm
VAR
    _active : BOOL := FALSE;
    _tripCount : INT := 0;
    _highLimit : INT := 50;
END_VAR
PUBLIC METHOD Evaluate
VAR_INPUT value : INT; END_VAR
VAR wasActive : BOOL; END_VAR
    wasActive := _active;
    IF value > _highLimit THEN _active := TRUE;
    ELSIF value < (_highLimit - 5) THEN _active := FALSE; END_IF;
    IF _active AND NOT wasActive THEN _tripCount := _tripCount + 1; END_IF;
END_METHOD
PUBLIC METHOD WriteStatus
VAR_INPUT pActive : REF_TO INT; pTrips : REF_TO INT; END_VAR
    IF pActive <> NULL THEN
        IF _active THEN pActive^ := 1; ELSE pActive^ := 0; END_IF;
    END_IF;
    IF pTrips <> NULL THEN pTrips^ := _tripCount; END_IF;
END_METHOD
END_CLASS

VAR_GLOBAL g_active : INT; g_trips : INT; END_VAR
PROGRAM Main
VAR
    alarm : Alarm;
    status : INT := 0;
    trips : INT := 0;
END_VAR
    alarm.Evaluate(value := 60);  // above 50 → trip
    alarm.WriteStatus(pActive := REF(status), pTrips := REF(trips));
    g_active := status;
    g_trips := trips;
END_PROGRAM
"#;
    let e = run_program(source, 1);
    assert_eq!(e.vm().get_global("g_active"), Some(&Value::Int(1)), "60 > 50: active");
    assert_eq!(e.vm().get_global("g_trips"), Some(&Value::Int(1)), "one trip");
}

// =============================================================================
// DataLogger with pointer-based sampling
// =============================================================================

#[test]
fn data_logger_records_stats() {
    let source = r#"
CLASS DataLogger
VAR
    _sum : INT := 0;
    _min : INT := 32767;
    _max : INT := -32768;
    _count : INT := 0;
END_VAR
PUBLIC METHOD RecordFrom
VAR_INPUT source : REF_TO INT; END_VAR
VAR val : INT; END_VAR
    IF source <> NULL THEN
        val := source^;
        _sum := _sum + val;
        _count := _count + 1;
        IF val < _min THEN _min := val; END_IF;
        IF val > _max THEN _max := val; END_IF;
    END_IF;
END_METHOD
PUBLIC METHOD GetAverage : INT
    IF _count > 0 THEN GetAverage := _sum / _count;
    ELSE GetAverage := 0; END_IF;
END_METHOD
PUBLIC METHOD GetMin : INT GetMin := _min; END_METHOD
PUBLIC METHOD GetMax : INT GetMax := _max; END_METHOD
PUBLIC METHOD GetCount : INT GetCount := _count; END_METHOD
END_CLASS

VAR_GLOBAL g_avg : INT; g_min : INT; g_max : INT; g_count : INT; END_VAR
PROGRAM Main
VAR
    logger : DataLogger;
    v1 : INT := 10;
    v2 : INT := 50;
    v3 : INT := 30;
    v4 : INT := 80;
    v5 : INT := 20;
END_VAR
    logger.RecordFrom(source := REF(v1));
    logger.RecordFrom(source := REF(v2));
    logger.RecordFrom(source := REF(v3));
    logger.RecordFrom(source := REF(v4));
    logger.RecordFrom(source := REF(v5));
    g_avg := logger.GetAverage();
    g_min := logger.GetMin();
    g_max := logger.GetMax();
    g_count := logger.GetCount();
END_PROGRAM
"#;
    let e = run_program(source, 1);
    // sum = 10+50+30+80+20 = 190, avg = 190/5 = 38
    assert_eq!(e.vm().get_global("g_avg"), Some(&Value::Int(38)));
    assert_eq!(e.vm().get_global("g_min"), Some(&Value::Int(10)));
    assert_eq!(e.vm().get_global("g_max"), Some(&Value::Int(80)));
    assert_eq!(e.vm().get_global("g_count"), Some(&Value::Int(5)));
}

// =============================================================================
// Hysteresis controller: on/off with dead band
// =============================================================================

#[test]
fn hysteresis_controller_behavior() {
    let source = r#"
FUNCTION_BLOCK HysteresisController
VAR_INPUT processValue : INT; setpoint : INT; hysteresis : INT; END_VAR
VAR_OUTPUT output : BOOL; END_VAR
    IF processValue < (setpoint - hysteresis) THEN output := TRUE;
    ELSIF processValue > (setpoint + hysteresis) THEN output := FALSE; END_IF;
END_FUNCTION_BLOCK

VAR_GLOBAL g_on1 : INT; g_on2 : INT; g_on3 : INT; END_VAR
PROGRAM Main
VAR ctrl : HysteresisController; END_VAR
    // Below low threshold → ON
    ctrl(processValue := 40, setpoint := 50, hysteresis := 5);
    IF ctrl.output THEN g_on1 := 1; ELSE g_on1 := 0; END_IF;

    // In dead band → maintain (still ON from previous)
    ctrl(processValue := 48, setpoint := 50, hysteresis := 5);
    IF ctrl.output THEN g_on2 := 1; ELSE g_on2 := 0; END_IF;

    // Above high threshold → OFF
    ctrl(processValue := 60, setpoint := 50, hysteresis := 5);
    IF ctrl.output THEN g_on3 := 1; ELSE g_on3 := 0; END_IF;
END_PROGRAM
"#;
    let e = run_program(source, 1);
    assert_eq!(e.vm().get_global("g_on1"), Some(&Value::Int(1)), "40 < 45: ON");
    assert_eq!(e.vm().get_global("g_on2"), Some(&Value::Int(1)), "48 in dead band: stays ON");
    assert_eq!(e.vm().get_global("g_on3"), Some(&Value::Int(0)), "60 > 55: OFF");
}

// =============================================================================
// Full integration: FB field I/O → Class pointer method → Global
// =============================================================================

#[test]
fn integration_fb_class_pointer_chain() {
    let source = r#"
FUNCTION_BLOCK Sensor
VAR_INPUT raw : INT; END_VAR
VAR_OUTPUT value : INT; END_VAR
    value := raw / 10;
END_FUNCTION_BLOCK

CLASS Accumulator
VAR _total : INT := 0; END_VAR
PUBLIC METHOD AddFrom
VAR_INPUT source : REF_TO INT; END_VAR
    IF source <> NULL THEN _total := _total + source^; END_IF;
END_METHOD
PUBLIC METHOD GetTotal : INT
    GetTotal := _total;
END_METHOD
END_CLASS

VAR_GLOBAL g_total : INT; END_VAR
PROGRAM Main
VAR
    sensor : Sensor;
    acc : Accumulator;
    sensorVal : INT := 0;
    cycle : INT := 0;
END_VAR
    cycle := cycle + 1;

    // FB processes raw → scaled value
    sensor.raw := cycle * 100;
    sensor();

    // Class accumulates sensor output via pointer
    // NOTE: REF(fb.field) is not supported — use intermediate local
    sensorVal := sensor.value;
    acc.AddFrom(source := REF(sensorVal));

    g_total := acc.GetTotal();
END_PROGRAM
"#;
    // cycle 1: raw=100, value=10, total=10
    // cycle 2: raw=200, value=20, total=30
    // cycle 3: raw=300, value=30, total=60
    let e = run_program(source, 3);
    assert_eq!(e.vm().get_global("g_total"), Some(&Value::Int(60)));
}
