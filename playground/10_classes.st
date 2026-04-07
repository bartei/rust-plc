(*
 * Classes and OOP — IEC 61131-3 Object-Oriented Extensions
 *
 * Demonstrates the full OOP feature set:
 * - CLASS with VAR blocks, methods, and properties
 * - INTERFACE with method prototypes
 * - Inheritance via EXTENDS
 * - Interface implementation via IMPLEMENTS
 * - Access specifiers: PUBLIC, PRIVATE, PROTECTED
 * - ABSTRACT classes and methods
 * - FINAL classes and methods
 * - OVERRIDE for method overriding
 * - Properties with GET / SET accessors
 *
 * Run with: st-cli run playground/10_classes.st -n 10
 *)

// ═══════════════════════════════════════════════════════════════════════
// Interfaces — define contracts that classes must fulfill
// ═══════════════════════════════════════════════════════════════════════

INTERFACE IResettable
    METHOD Reset
    END_METHOD
END_INTERFACE

INTERFACE ICountable
    METHOD GetCount : INT
    END_METHOD
END_INTERFACE

// Interfaces can extend other interfaces
INTERFACE IFullCounter EXTENDS ICountable
    METHOD Increment
    END_METHOD
    METHOD Decrement
    END_METHOD
END_INTERFACE

// ═══════════════════════════════════════════════════════════════════════
// Basic class — encapsulates state + behavior
// ═══════════════════════════════════════════════════════════════════════

CLASS Counter IMPLEMENTS IFullCounter, IResettable
VAR
    _count : INT := 0;
    _maxCount : INT := 1000;
END_VAR

    // Public method — accessible from anywhere
    PUBLIC METHOD Increment
        IF _count < _maxCount THEN
            _count := _count + 1;
        END_IF;
    END_METHOD

    PUBLIC METHOD Decrement
        IF _count > 0 THEN
            _count := _count - 1;
        END_IF;
    END_METHOD

    PUBLIC METHOD GetCount : INT
        GetCount := _count;
    END_METHOD

    PUBLIC METHOD Reset
        _count := 0;
    END_METHOD

    // Private helper — only visible inside this class
    PRIVATE METHOD IsAtLimit : BOOL
    VAR
        result : BOOL;
    END_VAR
        result := (_count >= _maxCount);
        IsAtLimit := result;
    END_METHOD

    // Property — syntactic sugar for getter/setter
    PUBLIC PROPERTY Count : INT
        GET
            Count := _count;
        END_GET
        SET
            _count := Count;
        END_SET
    END_PROPERTY

    // Read-only property (getter only)
    PUBLIC PROPERTY MaxCount : INT
        GET
            MaxCount := _maxCount;
        END_GET
    END_PROPERTY
END_CLASS

// ═══════════════════════════════════════════════════════════════════════
// Inheritance — EXTENDS builds on existing classes
// ═══════════════════════════════════════════════════════════════════════

CLASS StepCounter EXTENDS Counter
VAR
    _stepSize : INT := 1;
    _accumulated : INT := 0;
END_VAR

    // Override changes the behavior of a parent method
    OVERRIDE METHOD Increment
    VAR
        i : INT;
    END_VAR
        _accumulated := _accumulated + _stepSize;
    END_METHOD

    PUBLIC METHOD SetStep
    VAR_INPUT
        step : INT;
    END_VAR
        IF step > 0 THEN
            _stepSize := step;
        END_IF;
    END_METHOD

    PUBLIC METHOD GetAccumulated : INT
        GetAccumulated := _accumulated;
    END_METHOD
END_CLASS

// ═══════════════════════════════════════════════════════════════════════
// Abstract classes — cannot be instantiated, define a template
// ═══════════════════════════════════════════════════════════════════════

ABSTRACT CLASS Shape
VAR
    _x : REAL := 0.0;
    _y : REAL := 0.0;
END_VAR

    // Abstract methods MUST be implemented by subclasses
    ABSTRACT METHOD Area : REAL
    END_METHOD

    ABSTRACT METHOD Perimeter : REAL
    END_METHOD

    // Concrete methods CAN be inherited as-is
    PUBLIC METHOD MoveTo
    VAR_INPUT
        newX : REAL;
        newY : REAL;
    END_VAR
        _x := newX;
        _y := newY;
    END_METHOD

    PUBLIC METHOD GetX : REAL
        GetX := _x;
    END_METHOD

    PUBLIC METHOD GetY : REAL
        GetY := _y;
    END_METHOD
END_CLASS

CLASS Rectangle EXTENDS Shape
VAR
    _width  : REAL := 0.0;
    _height : REAL := 0.0;
END_VAR

    PUBLIC METHOD SetSize
    VAR_INPUT
        w : REAL;
        h : REAL;
    END_VAR
        _width := w;
        _height := h;
    END_METHOD

    // Implementing the abstract methods
    OVERRIDE METHOD Area : REAL
        Area := _width * _height;
    END_METHOD

    OVERRIDE METHOD Perimeter : REAL
        Perimeter := 2.0 * (_width + _height);
    END_METHOD
END_CLASS

CLASS Circle EXTENDS Shape
VAR
    _radius : REAL := 0.0;
END_VAR

    PUBLIC METHOD SetRadius
    VAR_INPUT
        r : REAL;
    END_VAR
        _radius := r;
    END_METHOD

    OVERRIDE METHOD Area : REAL
        Area := 3.14159 * _radius * _radius;
    END_METHOD

    OVERRIDE METHOD Perimeter : REAL
        Perimeter := 2.0 * 3.14159 * _radius;
    END_METHOD
END_CLASS

// ═══════════════════════════════════════════════════════════════════════
// Final class — cannot be subclassed (sealed)
// ═══════════════════════════════════════════════════════════════════════

FINAL CLASS Logger
VAR
    _lastCode : INT := 0;
    _entryCount : INT := 0;
END_VAR

    PUBLIC METHOD Log
    VAR_INPUT
        code : INT;
    END_VAR
        _lastCode := code;
        _entryCount := _entryCount + 1;
    END_METHOD

    // Final method — cannot be overridden even if class is subclassed
    FINAL METHOD GetLastCode : INT
        GetLastCode := _lastCode;
    END_METHOD

    PUBLIC METHOD GetEntryCount : INT
        GetEntryCount := _entryCount;
    END_METHOD
END_CLASS

// ═══════════════════════════════════════════════════════════════════════
// Access specifiers — control visibility of members
// ═══════════════════════════════════════════════════════════════════════

CLASS AccessDemo
VAR
    _secret  : INT := 42;    // Only accessible within this class
    _shared  : INT := 0;     // Accessible within hierarchy
    _open    : INT := 0;     // Accessible from anywhere
END_VAR

    PRIVATE METHOD GetSecret : INT
        GetSecret := _secret;
    END_METHOD

    PROTECTED METHOD GetShared : INT
        GetShared := _shared;
    END_METHOD

    PUBLIC METHOD GetOpen : INT
        GetOpen := _open;
    END_METHOD

    PUBLIC METHOD SetAll
    VAR_INPUT
        val : INT;
    END_VAR
        _secret := val;
        _shared := val;
        _open := val;
    END_METHOD
END_CLASS

// ═══════════════════════════════════════════════════════════════════════
// Industrial example — PID controller as a class
// ═══════════════════════════════════════════════════════════════════════

CLASS PidController IMPLEMENTS IResettable
VAR_INPUT
    setpoint : REAL;
    feedback : REAL;
END_VAR
VAR_OUTPUT
    output : REAL;
END_VAR
VAR
    _kp : REAL := 1.0;
    _ki : REAL := 0.1;
    _kd : REAL := 0.01;
    _integral   : REAL := 0.0;
    _prevError  : REAL := 0.0;
    _outMin     : REAL := 0.0;
    _outMax     : REAL := 100.0;
END_VAR

    PUBLIC METHOD Configure
    VAR_INPUT
        kp : REAL;
        ki : REAL;
        kd : REAL;
        minOut : REAL;
        maxOut : REAL;
    END_VAR
        _kp := kp;
        _ki := ki;
        _kd := kd;
        _outMin := minOut;
        _outMax := maxOut;
    END_METHOD

    PUBLIC METHOD Execute
    VAR
        error : REAL;
        derivative : REAL;
        raw : REAL;
    END_VAR
        error := setpoint - feedback;
        _integral := _integral + error;
        derivative := error - _prevError;
        _prevError := error;

        raw := _kp * error + _ki * _integral + _kd * derivative;

        // Clamp output
        IF raw < _outMin THEN
            output := _outMin;
        ELSIF raw > _outMax THEN
            output := _outMax;
        ELSE
            output := raw;
        END_IF;
    END_METHOD

    PUBLIC METHOD Reset
        _integral := 0.0;
        _prevError := 0.0;
        output := 0.0;
    END_METHOD
END_CLASS

// ═══════════════════════════════════════════════════════════════════════
// Main program — ties it all together
// ═══════════════════════════════════════════════════════════════════════

VAR_GLOBAL
    g_counter_value : INT;
    g_step_counter_value : INT;
    g_rect_area : REAL;
    g_circle_area : REAL;
    g_pid_output : REAL;
    g_log_entries : INT;
END_VAR

PROGRAM Main
VAR
    // Class instances — just like FB instances
    cnt       : Counter;
    stepCnt   : StepCounter;
    rect      : Rectangle;
    circ      : Circle;
    pid       : PidController;
    logger    : Logger;
    access    : AccessDemo;

    cycle     : INT := 0;
    simTemp   : REAL := 20.0;
END_VAR
    cycle := cycle + 1;

    // ── Counter demo ──
    cnt.Increment();
    cnt.Increment();
    cnt.Increment();
    g_counter_value := cnt.GetCount();

    // ── Step counter (inherited + overridden) ──
    stepCnt.SetStep(step := 5);
    stepCnt.Increment();   // Accumulates 5 instead of 1
    g_step_counter_value := stepCnt.GetAccumulated();

    // ── Shapes (abstract base, concrete subclasses) ──
    rect.SetSize(w := 4.0, h := 3.0);
    rect.MoveTo(newX := 10.0, newY := 20.0);
    g_rect_area := rect.Area();          // 12.0

    circ.SetRadius(r := 5.0);
    g_circle_area := circ.Area();        // ~78.54

    // ── PID controller ──
    pid.Configure(kp := 2.0, ki := 0.5, kd := 0.1,
                  minOut := 0.0, maxOut := 100.0);
    pid.setpoint := 50.0;
    pid.feedback := simTemp;
    pid.Execute();
    g_pid_output := pid.output;

    // Simulate process response
    simTemp := simTemp + pid.output * 0.01;

    // ── Logger (final class) ──
    logger.Log(code := cycle);
    g_log_entries := logger.GetEntryCount();

    // ── Access demo ──
    access.SetAll(val := cycle);

    // Reset everything every 100 cycles
    IF (cycle MOD 100) = 0 THEN
        cnt.Reset();
        stepCnt.Reset();
        pid.Reset();
    END_IF;
END_PROGRAM
