(*
 * Advanced Class Patterns — Industrial OOP in Structured Text
 *
 * Demonstrates real-world class patterns for industrial automation:
 * - State machine as a class with encapsulated transitions
 * - Sensor abstraction with filtering (moving average)
 * - Alarm manager with configurable thresholds
 * - Multi-level inheritance (3 levels deep)
 * - Multiple class instances working together in a control loop
 * - Method chains: one method calls another on the same instance
 * - State persistence across scan cycles
 *
 * Run with: st-cli run playground/11_class_patterns.st -n 100
 *)

// ═══════════════════════════════════════════════════════════════════════
// Interface: common alarm contract
// ═══════════════════════════════════════════════════════════════════════

INTERFACE IAlarmSource
    METHOD IsActive : INT
    END_METHOD
    METHOD Acknowledge
    END_METHOD
END_INTERFACE

// ═══════════════════════════════════════════════════════════════════════
// Sensor filter — exponential moving average
// ═══════════════════════════════════════════════════════════════════════

CLASS SensorFilter
VAR
    _filtered : INT := 0;
    _alpha    : INT := 30;   // Filter weight (0-100), higher = more responsive
    _initialized : BOOL := FALSE;
END_VAR

    PUBLIC METHOD Configure
    VAR_INPUT
        alpha : INT;
    END_VAR
        IF alpha > 0 THEN
            IF alpha > 100 THEN
                _alpha := 100;
            ELSE
                _alpha := alpha;
            END_IF;
        END_IF;
    END_METHOD

    PUBLIC METHOD Update : INT
    VAR_INPUT
        rawValue : INT;
    END_VAR
        IF NOT _initialized THEN
            _filtered := rawValue;
            _initialized := TRUE;
        ELSE
            // EMA: filtered = alpha * raw + (100-alpha) * prev, scaled by 100
            _filtered := (_alpha * rawValue + (100 - _alpha) * _filtered) / 100;
        END_IF;
        Update := _filtered;
    END_METHOD

    PUBLIC METHOD GetFiltered : INT
        GetFiltered := _filtered;
    END_METHOD

    PUBLIC METHOD Reset
        _filtered := 0;
        _initialized := FALSE;
    END_METHOD
END_CLASS

// ═══════════════════════════════════════════════════════════════════════
// State machine — valve controller
// ═══════════════════════════════════════════════════════════════════════

CLASS ValveController
VAR
    _state     : INT := 0;     // 0=CLOSED, 1=OPENING, 2=OPEN, 3=CLOSING
    _position  : INT := 0;     // 0-100% position
    _target    : INT := 0;     // target position
    _speed     : INT := 5;     // movement speed per cycle
    _cycleCount : INT := 0;
END_VAR

    PUBLIC METHOD SetTarget
    VAR_INPUT
        target : INT;
    END_VAR
        IF target < 0 THEN
            _target := 0;
        ELSIF target > 100 THEN
            _target := 100;
        ELSE
            _target := target;
        END_IF;

        IF _target > _position THEN
            _state := 1;   // OPENING
        ELSIF _target < _position THEN
            _state := 3;   // CLOSING
        END_IF;
    END_METHOD

    PUBLIC METHOD Execute
        _cycleCount := _cycleCount + 1;

        CASE _state OF
            0:  // CLOSED — do nothing
                ;
            1:  // OPENING
                _position := _position + _speed;
                IF _position >= _target THEN
                    _position := _target;
                    IF _position >= 100 THEN
                        _state := 2;  // OPEN
                    ELSE
                        _state := 0;  // reached target, stop
                    END_IF;
                END_IF;
            2:  // OPEN — do nothing
                ;
            3:  // CLOSING
                _position := _position - _speed;
                IF _position <= _target THEN
                    _position := _target;
                    IF _position <= 0 THEN
                        _state := 0;  // CLOSED
                    ELSE
                        _state := 0;  // reached target, stop
                    END_IF;
                END_IF;
        END_CASE;
    END_METHOD

    PUBLIC METHOD GetPosition : INT
        GetPosition := _position;
    END_METHOD

    PUBLIC METHOD GetState : INT
        GetState := _state;
    END_METHOD

    PUBLIC METHOD IsFullyOpen : INT
        IF _position >= 100 THEN
            IsFullyOpen := 1;
        ELSE
            IsFullyOpen := 0;
        END_IF;
    END_METHOD

    PUBLIC METHOD IsFullyClosed : INT
        IF _position <= 0 THEN
            IsFullyClosed := 1;
        ELSE
            IsFullyClosed := 0;
        END_IF;
    END_METHOD
END_CLASS

// ═══════════════════════════════════════════════════════════════════════
// Alarm base class + level alarm
// ═══════════════════════════════════════════════════════════════════════

CLASS AlarmBase IMPLEMENTS IAlarmSource
VAR
    _active    : BOOL := FALSE;
    _acked     : BOOL := FALSE;
    _tripCount : INT := 0;
END_VAR

    PUBLIC METHOD IsActive : INT
        IF _active THEN
            IsActive := 1;
        ELSE
            IsActive := 0;
        END_IF;
    END_METHOD

    PUBLIC METHOD Acknowledge
        _acked := TRUE;
    END_METHOD

    PUBLIC METHOD GetTripCount : INT
        GetTripCount := _tripCount;
    END_METHOD

    // Protected helper for subclasses
    PROTECTED METHOD SetActive
    VAR_INPUT
        active : BOOL;
    END_VAR
        IF active AND NOT _active THEN
            _tripCount := _tripCount + 1;
            _acked := FALSE;
        END_IF;
        _active := active;
    END_METHOD
END_CLASS

CLASS LevelAlarm EXTENDS AlarmBase
VAR
    _highLimit  : INT := 100;
    _lowLimit   : INT := 0;
    _hysteresis : INT := 5;
    _mode       : INT := 0;    // 0=high, 1=low, 2=band
END_VAR

    PUBLIC METHOD ConfigureHigh
    VAR_INPUT
        limit : INT;
        hyst  : INT;
    END_VAR
        _highLimit := limit;
        _hysteresis := hyst;
        _mode := 0;
    END_METHOD

    PUBLIC METHOD ConfigureLow
    VAR_INPUT
        limit : INT;
        hyst  : INT;
    END_VAR
        _lowLimit := limit;
        _hysteresis := hyst;
        _mode := 1;
    END_METHOD

    PUBLIC METHOD Evaluate
    VAR_INPUT
        value : INT;
    END_VAR
    VAR
        newActive : BOOL;
    END_VAR
        newActive := _active;

        IF _mode = 0 THEN
            // High alarm
            IF value > _highLimit THEN
                newActive := TRUE;
            ELSIF value < (_highLimit - _hysteresis) THEN
                newActive := FALSE;
            END_IF;
        ELSIF _mode = 1 THEN
            // Low alarm
            IF value < _lowLimit THEN
                newActive := TRUE;
            ELSIF value > (_lowLimit + _hysteresis) THEN
                newActive := FALSE;
            END_IF;
        END_IF;

        _active := newActive;
        IF newActive AND NOT _active THEN
            _tripCount := _tripCount + 1;
            _acked := FALSE;
        END_IF;
    END_METHOD
END_CLASS

// ═══════════════════════════════════════════════════════════════════════
// Multi-level inheritance: Base → Controller → TemperatureController
// ═══════════════════════════════════════════════════════════════════════

CLASS BaseController
VAR
    _enabled : BOOL := FALSE;
    _output  : INT := 0;
END_VAR

    PUBLIC METHOD Enable
        _enabled := TRUE;
    END_METHOD

    PUBLIC METHOD Disable
        _enabled := FALSE;
        _output := 0;
    END_METHOD

    PUBLIC METHOD GetOutput : INT
        GetOutput := _output;
    END_METHOD

    PUBLIC METHOD IsEnabled : INT
        IF _enabled THEN
            IsEnabled := 1;
        ELSE
            IsEnabled := 0;
        END_IF;
    END_METHOD
END_CLASS

CLASS ProportionalController EXTENDS BaseController
VAR
    _setpoint : INT := 0;
    _gain     : INT := 10;     // gain * 10 (scaled integer math)
END_VAR

    PUBLIC METHOD SetSetpoint
    VAR_INPUT
        sp : INT;
    END_VAR
        _setpoint := sp;
    END_METHOD

    PUBLIC METHOD SetGain
    VAR_INPUT
        gain : INT;
    END_VAR
        _gain := gain;
    END_METHOD

    PUBLIC METHOD Calculate
    VAR_INPUT
        feedback : INT;
    END_VAR
    VAR
        error : INT;
    END_VAR
        IF _enabled THEN
            error := _setpoint - feedback;
            _output := (error * _gain) / 10;
            // Clamp output 0-100
            IF _output < 0 THEN
                _output := 0;
            ELSIF _output > 100 THEN
                _output := 100;
            END_IF;
        ELSE
            _output := 0;
        END_IF;
    END_METHOD
END_CLASS

CLASS TemperatureController EXTENDS ProportionalController
VAR
    _minTemp : INT := 0;
    _maxTemp : INT := 200;
    _safeMode : BOOL := FALSE;
END_VAR

    PUBLIC METHOD SetLimits
    VAR_INPUT
        minT : INT;
        maxT : INT;
    END_VAR
        _minTemp := minT;
        _maxTemp := maxT;
    END_METHOD

    PUBLIC METHOD ProcessTemperature
    VAR_INPUT
        temperature : INT;
    END_VAR
        // Safety check
        IF temperature > _maxTemp THEN
            _safeMode := TRUE;
            _output := 0;
            _enabled := FALSE;
        ELSIF temperature < _minTemp THEN
            _safeMode := TRUE;
            _output := 100;
        ELSE
            _safeMode := FALSE;
        END_IF;

        // Normal control if not in safe mode
        IF NOT _safeMode AND _enabled THEN
            _output := (_setpoint - temperature) * _gain / 10;
            IF _output < 0 THEN
                _output := 0;
            ELSIF _output > 100 THEN
                _output := 100;
            END_IF;
        END_IF;
    END_METHOD

    PUBLIC METHOD IsSafe : INT
        IF _safeMode THEN
            IsSafe := 0;
        ELSE
            IsSafe := 1;
        END_IF;
    END_METHOD
END_CLASS

// ═══════════════════════════════════════════════════════════════════════
// Main program — integrated process control
// ═══════════════════════════════════════════════════════════════════════

VAR_GLOBAL
    // Sensor readings
    g_raw_temp       : INT;
    g_filtered_temp  : INT;

    // Controller state
    g_ctrl_output    : INT;
    g_ctrl_enabled   : INT;
    g_ctrl_safe      : INT;

    // Valve state
    g_valve_position : INT;
    g_valve_state    : INT;

    // Alarms
    g_high_alarm     : INT;
    g_alarm_trips    : INT;

    // Cycle tracking
    g_cycle          : INT;
END_VAR

PROGRAM Main
VAR
    // Process objects
    tempSensor  : SensorFilter;
    tempCtrl    : TemperatureController;
    valve       : ValveController;
    highAlarm   : LevelAlarm;

    // Simulation
    cycle       : INT := 0;
    simTemp     : INT := 20;
    heaterEffect : INT := 0;
END_VAR
    cycle := cycle + 1;
    g_cycle := cycle;

    // ── Configure on first cycle ──
    IF cycle = 1 THEN
        tempSensor.Configure(alpha := 40);
        tempCtrl.SetLimits(minT := -10, maxT := 120);
        tempCtrl.SetSetpoint(sp := 50);
        tempCtrl.SetGain(gain := 15);
        tempCtrl.Enable();
        highAlarm.ConfigureHigh(limit := 80, hyst := 5);
    END_IF;

    // ── Simulate temperature ──
    // Heater warms up, environment cools down
    heaterEffect := g_ctrl_output / 5;
    simTemp := simTemp + heaterEffect;
    IF simTemp > 20 THEN
        simTemp := simTemp - 1;      // cooling
    END_IF;
    // Add some cyclic disturbance
    IF (cycle MOD 20) < 10 THEN
        simTemp := simTemp + 2;
    ELSE
        simTemp := simTemp - 1;
    END_IF;
    g_raw_temp := simTemp;

    // ── Filter sensor reading ──
    g_filtered_temp := tempSensor.Update(rawValue := simTemp);

    // ── Run temperature controller ──
    tempCtrl.ProcessTemperature(temperature := g_filtered_temp);
    g_ctrl_output := tempCtrl.GetOutput();
    g_ctrl_enabled := tempCtrl.IsEnabled();
    g_ctrl_safe := tempCtrl.IsSafe();

    // ── Drive valve from controller output ──
    valve.SetTarget(target := g_ctrl_output);
    valve.Execute();
    g_valve_position := valve.GetPosition();
    g_valve_state := valve.GetState();

    // ── Check alarms ──
    highAlarm.Evaluate(value := g_filtered_temp);
    g_high_alarm := highAlarm.IsActive();
    g_alarm_trips := highAlarm.GetTripCount();

    // Auto-acknowledge alarms after 10 cycles
    IF g_high_alarm = 1 THEN
        IF (cycle MOD 10) = 0 THEN
            highAlarm.Acknowledge();
        END_IF;
    END_IF;
END_PROGRAM
