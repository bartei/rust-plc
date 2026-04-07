(*
 * Class Instances — Lifecycle, Composition, and Interaction Patterns
 *
 * IEC 61131-3 classes are instantiated statically in VAR blocks (no NEW/heap).
 * This example demonstrates the full range of instance patterns:
 *
 * - Multiple independent instances of the same class
 * - Explicit initialization (constructor pattern)
 * - Init-once, run-many lifecycle
 * - Class instances inside function blocks (composition)
 * - Two classes interacting (producer-consumer)
 * - State machine lifecycle (idle → running → done → reset)
 * - Builder/configurator pattern
 * - Observer pattern: broadcast to multiple targets via pointers
 * - Inherited instances with independent state per level
 * - Reset and reuse across cycles
 *
 * Run with: st-cli run playground/14_class_instances.st -n 50
 *)

// ═══════════════════════════════════════════════════════════════════════
// Reusable class: Accumulator with reset
// ═══════════════════════════════════════════════════════════════════════

CLASS Accumulator
VAR
    _total : INT := 0;
    _count : INT := 0;
END_VAR

    PUBLIC METHOD Add
    VAR_INPUT v : INT; END_VAR
        _total := _total + v;
        _count := _count + 1;
    END_METHOD

    PUBLIC METHOD GetTotal : INT
        GetTotal := _total;
    END_METHOD

    PUBLIC METHOD GetCount : INT
        GetCount := _count;
    END_METHOD

    PUBLIC METHOD GetAverage : INT
        IF _count > 0 THEN
            GetAverage := _total / _count;
        ELSE
            GetAverage := 0;
        END_IF;
    END_METHOD

    PUBLIC METHOD Reset
        _total := 0;
        _count := 0;
    END_METHOD
END_CLASS

// ═══════════════════════════════════════════════════════════════════════
// State machine class — full lifecycle
// ═══════════════════════════════════════════════════════════════════════

CLASS BatchSequencer
VAR
    _state   : INT := 0;     // 0=idle, 1=filling, 2=heating, 3=mixing, 4=done
    _timer   : INT := 0;
    _target  : INT := 0;
    _product : INT := 0;     // accumulated product
END_VAR

    PUBLIC METHOD Start
    VAR_INPUT batchSize : INT; END_VAR
        IF _state = 0 THEN
            _state := 1;
            _timer := 0;
            _target := batchSize;
            _product := 0;
        END_IF;
    END_METHOD

    PUBLIC METHOD Update
        IF _state = 1 THEN
            // Filling
            _product := _product + 10;
            _timer := _timer + 1;
            IF _product >= _target THEN
                _state := 2;
                _timer := 0;
            END_IF;
        ELSIF _state = 2 THEN
            // Heating
            _timer := _timer + 1;
            IF _timer >= 5 THEN
                _state := 3;
                _timer := 0;
            END_IF;
        ELSIF _state = 3 THEN
            // Mixing
            _timer := _timer + 1;
            IF _timer >= 3 THEN
                _state := 4;
            END_IF;
        END_IF;
    END_METHOD

    PUBLIC METHOD GetState : INT
        GetState := _state;
    END_METHOD

    PUBLIC METHOD GetProduct : INT
        GetProduct := _product;
    END_METHOD

    PUBLIC METHOD IsDone : INT
        IF _state = 4 THEN IsDone := 1; ELSE IsDone := 0; END_IF;
    END_METHOD

    PUBLIC METHOD Reset
        _state := 0;
        _timer := 0;
        _product := 0;
    END_METHOD
END_CLASS

// ═══════════════════════════════════════════════════════════════════════
// Producer-Consumer pattern
// ═══════════════════════════════════════════════════════════════════════

CLASS Producer
VAR
    _sequence : INT := 0;
END_VAR
    PUBLIC METHOD Produce : INT
        _sequence := _sequence + 7;
        Produce := _sequence;
    END_METHOD
    PUBLIC METHOD GetSequence : INT
        GetSequence := _sequence;
    END_METHOD
END_CLASS

CLASS Consumer
VAR
    _consumed : INT := 0;
    _items    : INT := 0;
END_VAR
    PUBLIC METHOD Consume
    VAR_INPUT amount : INT; END_VAR
        _consumed := _consumed + amount;
        _items := _items + 1;
    END_METHOD
    PUBLIC METHOD GetConsumed : INT
        GetConsumed := _consumed;
    END_METHOD
    PUBLIC METHOD GetItems : INT
        GetItems := _items;
    END_METHOD
END_CLASS

// ═══════════════════════════════════════════════════════════════════════
// Observer pattern — broadcast values via pointers
// ═══════════════════════════════════════════════════════════════════════

CLASS Broadcaster
VAR
    _value : INT := 0;
END_VAR
    PUBLIC METHOD SetValue
    VAR_INPUT v : INT; END_VAR
        _value := v;
    END_METHOD
    PUBLIC METHOD Notify
    VAR_INPUT
        p1 : REF_TO INT;
        p2 : REF_TO INT;
        p3 : REF_TO INT;
    END_VAR
        IF p1 <> NULL THEN p1^ := _value; END_IF;
        IF p2 <> NULL THEN p2^ := _value; END_IF;
        IF p3 <> NULL THEN p3^ := _value; END_IF;
    END_METHOD
END_CLASS

// ═══════════════════════════════════════════════════════════════════════
// FB wrapper with class inside (composition)
// ═══════════════════════════════════════════════════════════════════════

FUNCTION_BLOCK SensorChannel
VAR_INPUT
    rawValue : INT;
END_VAR
VAR_OUTPUT
    filtered : INT;
    sampleCount : INT;
END_VAR
VAR
    stats : Accumulator;
END_VAR
    stats.Add(v := rawValue);
    filtered := stats.GetAverage();
    sampleCount := stats.GetCount();
END_FUNCTION_BLOCK

// ═══════════════════════════════════════════════════════════════════════
// Inheritance: base controller → specialized controller
// ═══════════════════════════════════════════════════════════════════════

CLASS BaseController
VAR
    _output : INT := 0;
    _enabled : BOOL := FALSE;
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
        IF _enabled THEN IsEnabled := 1; ELSE IsEnabled := 0; END_IF;
    END_METHOD
END_CLASS

CLASS BangBangController EXTENDS BaseController
VAR
    _setpoint : INT := 50;
    _hysteresis : INT := 5;
END_VAR
    PUBLIC METHOD Configure
    VAR_INPUT sp : INT; hyst : INT; END_VAR
        _setpoint := sp;
        _hysteresis := hyst;
    END_METHOD
    PUBLIC METHOD Compute
    VAR_INPUT pv : INT; END_VAR
        IF _enabled THEN
            IF pv < (_setpoint - _hysteresis) THEN
                _output := 100;
            ELSIF pv > (_setpoint + _hysteresis) THEN
                _output := 0;
            END_IF;
        ELSE
            _output := 0;
        END_IF;
    END_METHOD
END_CLASS

// ═══════════════════════════════════════════════════════════════════════
// Main program
// ═══════════════════════════════════════════════════════════════════════

VAR_GLOBAL
    // Multiple accumulators
    g_fast_total   : INT;
    g_slow_total   : INT;

    // Batch sequencer
    g_batch_state  : INT;
    g_batch_done   : INT;
    g_batch_product : INT;

    // Producer-Consumer
    g_produced     : INT;
    g_consumed     : INT;

    // Observer
    g_obs_a        : INT;
    g_obs_b        : INT;
    g_obs_c        : INT;

    // Sensor channel (FB with class)
    g_ch1_avg      : INT;
    g_ch2_avg      : INT;
    g_ch1_samples  : INT;
    g_ch2_samples  : INT;

    // Controller
    g_ctrl_out     : INT;

    g_cycle        : INT;
END_VAR

PROGRAM Main
VAR
    // Multiple independent instances
    fastAcc   : Accumulator;
    slowAcc   : Accumulator;

    // State machine
    batch     : BatchSequencer;

    // Producer-Consumer
    producer  : Producer;
    consumer  : Consumer;
    item      : INT;

    // Observer
    broadcast : Broadcaster;
    obs_a : INT := 0;
    obs_b : INT := 0;
    obs_c : INT := 0;

    // FB with class inside — two independent channels
    ch1 : SensorChannel;
    ch2 : SensorChannel;

    // Inherited controller
    ctrl : BangBangController;

    cycle : INT := 0;
    simTemp : INT := 30;
END_VAR
    cycle := cycle + 1;
    g_cycle := cycle;

    // ── Multiple independent accumulators ──
    fastAcc.Add(v := 10);
    IF (cycle MOD 3) = 0 THEN
        slowAcc.Add(v := 100);
    END_IF;
    g_fast_total := fastAcc.GetTotal();
    g_slow_total := slowAcc.GetTotal();
    // Reset fast accumulator every 10 cycles
    IF (cycle MOD 10) = 0 THEN
        fastAcc.Reset();
    END_IF;

    // ── Batch sequencer lifecycle ──
    IF cycle = 1 THEN
        batch.Start(batchSize := 50);
    END_IF;
    batch.Update();
    g_batch_state := batch.GetState();
    g_batch_done := batch.IsDone();
    g_batch_product := batch.GetProduct();
    // Restart if done
    IF batch.IsDone() = 1 THEN
        batch.Reset();
        batch.Start(batchSize := 50);
    END_IF;

    // ── Producer-Consumer ──
    item := producer.Produce();
    consumer.Consume(amount := item);
    g_produced := item;
    g_consumed := consumer.GetConsumed();

    // ── Observer broadcast ──
    broadcast.SetValue(v := cycle * 11);
    broadcast.Notify(p1 := REF(obs_a), p2 := REF(obs_b), p3 := REF(obs_c));
    g_obs_a := obs_a;
    g_obs_b := obs_b;
    g_obs_c := obs_c;

    // ── Two sensor channels with class inside FB ──
    ch1(rawValue := 100 + cycle);
    ch2(rawValue := 200 + cycle * 2);
    g_ch1_avg := ch1.filtered;
    g_ch2_avg := ch2.filtered;
    g_ch1_samples := ch1.sampleCount;
    g_ch2_samples := ch2.sampleCount;

    // ── Inherited controller ──
    IF cycle = 1 THEN
        ctrl.Configure(sp := 50, hyst := 5);
        ctrl.Enable();
    END_IF;
    // Simulate temperature responding to controller
    IF g_ctrl_out > 50 THEN
        simTemp := simTemp + 2;
    ELSIF simTemp > 30 THEN
        simTemp := simTemp - 1;
    END_IF;
    ctrl.Compute(pv := simTemp);
    g_ctrl_out := ctrl.GetOutput();
END_PROGRAM
