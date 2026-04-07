(*
 * Structs, Function Blocks, and Pointers — Working Together
 *
 * Demonstrates combining FB field access, class methods, and pointers
 * in a realistic industrial scenario: a batch process controller.
 *
 * Patterns demonstrated:
 * - FB field direct assignment (fb.field := value) and read (fb.field)
 * - Multiple FB field writes before call
 * - FB with pointer output binding
 * - Class methods accepting pointer parameters
 * - Cross-function pointer passing to modify caller's variables
 * - FB and Class instances working together
 * - Multi-cycle state persistence with field read-back
 * - Pointer-based out-parameter pattern
 *
 * Run with: st-cli run playground/13_structs_and_pointers.st -n 50
 *)

// ═══════════════════════════════════════════════════════════════════════
// Utility function: clamp a value via pointer (modifies in-place)
// ═══════════════════════════════════════════════════════════════════════

FUNCTION ClampValue : INT
VAR_INPUT
    target : REF_TO INT;
    lo : INT;
    hi : INT;
END_VAR
    IF target <> NULL THEN
        IF target^ < lo THEN
            target^ := lo;
        ELSIF target^ > hi THEN
            target^ := hi;
        END_IF;
    END_IF;
    ClampValue := target^;
END_FUNCTION

// ═══════════════════════════════════════════════════════════════════════
// FB: Analog input with scaling and filtering
// ═══════════════════════════════════════════════════════════════════════

FUNCTION_BLOCK AnalogInput
VAR_INPUT
    rawValue   : INT;         // Raw ADC value (0-1000)
    scaleLow   : INT;         // Engineering unit low
    scaleHigh  : INT;         // Engineering unit high
END_VAR
VAR_OUTPUT
    scaled     : INT;         // Scaled output in engineering units
    filtered   : INT;         // Filtered output (EMA)
    valid      : BOOL;        // TRUE if in valid range
END_VAR
VAR
    _filterState : INT := 0;
    _initialized : BOOL := FALSE;
END_VAR
    // Scale raw value to engineering units
    scaled := scaleLow + (rawValue * (scaleHigh - scaleLow)) / 1000;

    // Exponential moving average filter (alpha = 30%)
    IF NOT _initialized THEN
        _filterState := scaled;
        _initialized := TRUE;
    ELSE
        _filterState := (30 * scaled + 70 * _filterState) / 100;
    END_IF;
    filtered := _filterState;

    // Validity check
    IF rawValue >= 0 THEN
        IF rawValue <= 1000 THEN
            valid := TRUE;
        ELSE
            valid := FALSE;
        END_IF;
    ELSE
        valid := FALSE;
    END_IF;
END_FUNCTION_BLOCK

// ═══════════════════════════════════════════════════════════════════════
// FB: On/Off controller with hysteresis
// ═══════════════════════════════════════════════════════════════════════

FUNCTION_BLOCK HysteresisController
VAR_INPUT
    processValue : INT;
    setpoint     : INT;
    hysteresis   : INT;
END_VAR
VAR_OUTPUT
    output       : BOOL;
END_VAR
    IF processValue < (setpoint - hysteresis) THEN
        output := TRUE;    // Turn on
    ELSIF processValue > (setpoint + hysteresis) THEN
        output := FALSE;   // Turn off
    END_IF;
    // Otherwise maintain current state (hysteresis band)
END_FUNCTION_BLOCK

// ═══════════════════════════════════════════════════════════════════════
// FB: Configurable calculator with field-based I/O
// ═══════════════════════════════════════════════════════════════════════

FUNCTION_BLOCK Calculator
VAR_INPUT
    a   : INT;
    b   : INT;
    op  : INT;     // 0=add, 1=sub, 2=mul, 3=div
END_VAR
VAR_OUTPUT
    result : INT;
    error  : BOOL;
END_VAR
    error := FALSE;
    IF op = 0 THEN
        result := a + b;
    ELSIF op = 1 THEN
        result := a - b;
    ELSIF op = 2 THEN
        result := a * b;
    ELSIF op = 3 THEN
        IF b <> 0 THEN
            result := a / b;
        ELSE
            result := 0;
            error := TRUE;
        END_IF;
    ELSE
        result := 0;
        error := TRUE;
    END_IF;
END_FUNCTION_BLOCK

// ═══════════════════════════════════════════════════════════════════════
// Class: Alarm with trip count and configurable limits
// ═══════════════════════════════════════════════════════════════════════

CLASS Alarm
VAR
    _active     : BOOL := FALSE;
    _acked      : BOOL := FALSE;
    _tripCount  : INT := 0;
    _highLimit  : INT := 100;
    _hysteresis : INT := 5;
END_VAR

    PUBLIC METHOD Configure
    VAR_INPUT
        limit : INT;
        hyst  : INT;
    END_VAR
        _highLimit := limit;
        _hysteresis := hyst;
    END_METHOD

    PUBLIC METHOD Evaluate
    VAR_INPUT
        value : INT;
    END_VAR
    VAR
        wasActive : BOOL;
    END_VAR
        wasActive := _active;
        IF value > _highLimit THEN
            _active := TRUE;
        ELSIF value < (_highLimit - _hysteresis) THEN
            _active := FALSE;
        END_IF;
        // Count transitions to active
        IF _active AND NOT wasActive THEN
            _tripCount := _tripCount + 1;
            _acked := FALSE;
        END_IF;
    END_METHOD

    PUBLIC METHOD IsActive : INT
        IF _active THEN IsActive := 1; ELSE IsActive := 0; END_IF;
    END_METHOD

    PUBLIC METHOD GetTripCount : INT
        GetTripCount := _tripCount;
    END_METHOD

    PUBLIC METHOD Acknowledge
        _acked := TRUE;
    END_METHOD

    // Write alarm status through a pointer (out-param pattern)
    PUBLIC METHOD WriteStatus
    VAR_INPUT
        pActive : REF_TO INT;
        pTrips  : REF_TO INT;
    END_VAR
        IF pActive <> NULL THEN
            IF _active THEN pActive^ := 1; ELSE pActive^ := 0; END_IF;
        END_IF;
        IF pTrips <> NULL THEN
            pTrips^ := _tripCount;
        END_IF;
    END_METHOD
END_CLASS

// ═══════════════════════════════════════════════════════════════════════
// Class: Data logger with pointer-based sampling
// ═══════════════════════════════════════════════════════════════════════

CLASS DataLogger
VAR
    _sum   : INT := 0;
    _min   : INT := 32767;
    _max   : INT := -32768;
    _count : INT := 0;
END_VAR

    PUBLIC METHOD RecordFrom
    VAR_INPUT
        source : REF_TO INT;
    END_VAR
    VAR
        val : INT;
    END_VAR
        IF source <> NULL THEN
            val := source^;
            _sum := _sum + val;
            _count := _count + 1;
            IF val < _min THEN _min := val; END_IF;
            IF val > _max THEN _max := val; END_IF;
        END_IF;
    END_METHOD

    PUBLIC METHOD GetAverage : INT
        IF _count > 0 THEN
            GetAverage := _sum / _count;
        ELSE
            GetAverage := 0;
        END_IF;
    END_METHOD

    PUBLIC METHOD GetMin : INT
        GetMin := _min;
    END_METHOD

    PUBLIC METHOD GetMax : INT
        GetMax := _max;
    END_METHOD

    PUBLIC METHOD GetCount : INT
        GetCount := _count;
    END_METHOD

    PUBLIC METHOD Reset
        _sum := 0;
        _min := 32767;
        _max := -32768;
        _count := 0;
    END_METHOD
END_CLASS

// ═══════════════════════════════════════════════════════════════════════
// Main program: batch process with temperature control
// ═══════════════════════════════════════════════════════════════════════

VAR_GLOBAL
    // Analog input results
    g_raw_temp      : INT;
    g_scaled_temp   : INT;
    g_filtered_temp : INT;
    g_valid         : INT;

    // Controller
    g_heating       : INT;

    // Calculator
    g_calc_result   : INT;

    // Alarm
    g_alarm_active  : INT;
    g_alarm_trips   : INT;

    // Logger stats
    g_log_avg       : INT;
    g_log_min       : INT;
    g_log_max       : INT;
    g_log_count     : INT;

    // Cycle
    g_cycle         : INT;
END_VAR

PROGRAM Main
VAR
    // FB instances
    tempInput   : AnalogInput;
    controller  : HysteresisController;
    calc        : Calculator;

    // Class instances
    highTempAlarm : Alarm;
    tempLogger    : DataLogger;

    // Working variables
    cycle        : INT := 0;
    simRawAdc    : INT := 0;
    heaterOutput : INT := 0;
    alarmStatus  : INT := 0;
    alarmTrips   : INT := 0;
    dummy        : INT;
END_VAR
    cycle := cycle + 1;
    g_cycle := cycle;

    // ── Configure on first cycle ──
    IF cycle = 1 THEN
        highTempAlarm.Configure(limit := 80, hyst := 5);
    END_IF;

    // ── Simulate raw ADC reading ──
    // Ramp up, then stabilize, with some disturbance
    IF cycle < 20 THEN
        simRawAdc := cycle * 40;              // 0→800 over 20 cycles
    ELSE
        simRawAdc := 600;                     // Stabilize around 600
    END_IF;
    // Add cyclic disturbance
    IF (cycle MOD 4) < 2 THEN
        simRawAdc := simRawAdc + 50;
    ELSE
        simRawAdc := simRawAdc - 30;
    END_IF;
    // Apply heater effect (feedback)
    simRawAdc := simRawAdc + heaterOutput;
    dummy := ClampValue(target := REF(simRawAdc), lo := 0, hi := 1000);
    g_raw_temp := simRawAdc;

    // ── Process analog input (field-based I/O) ──
    tempInput.rawValue := simRawAdc;
    tempInput.scaleLow := 0;
    tempInput.scaleHigh := 100;
    tempInput();
    g_scaled_temp := tempInput.scaled;
    g_filtered_temp := tempInput.filtered;
    IF tempInput.valid THEN
        g_valid := 1;
    ELSE
        g_valid := 0;
    END_IF;

    // ── Hysteresis controller ──
    controller(processValue := tempInput.filtered,
               setpoint := 50,
               hysteresis := 5);
    IF controller.output THEN
        heaterOutput := 50;
        g_heating := 1;
    ELSE
        heaterOutput := 0;
        g_heating := 0;
    END_IF;

    // ── Calculator: compute temperature deviation ──
    calc.a := tempInput.filtered;
    calc.b := 50;
    calc.op := 1;     // subtract: deviation = filtered - setpoint
    calc();
    g_calc_result := calc.result;

    // ── Alarm evaluation ──
    highTempAlarm.Evaluate(value := tempInput.filtered);
    // Use pointer-based out-param pattern
    highTempAlarm.WriteStatus(pActive := REF(alarmStatus),
                              pTrips := REF(alarmTrips));
    g_alarm_active := alarmStatus;
    g_alarm_trips := alarmTrips;

    // ── Data logger: record filtered temperature ──
    // NOTE: REF(fb.field) is not yet supported — use intermediate variable
    alarmStatus := tempInput.filtered;
    tempLogger.RecordFrom(source := REF(alarmStatus));
    g_log_avg := tempLogger.GetAverage();
    g_log_min := tempLogger.GetMin();
    g_log_max := tempLogger.GetMax();
    g_log_count := tempLogger.GetCount();

    // Reset logger every 25 cycles
    IF (cycle MOD 25) = 0 THEN
        tempLogger.Reset();
    END_IF;
END_PROGRAM
