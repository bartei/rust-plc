(*
 * ProcessControl — Multi-file OOP project
 *
 * Demonstrates a complete PLC project using classes, interfaces,
 * inheritance, pointers, and function blocks across multiple files:
 *
 *   interfaces/controllable.st  — IControllable interface
 *   interfaces/resettable.st    — IResettable interface
 *   classes/sensor.st           — Sensor class (implements IResettable)
 *   classes/controller.st       — BaseController + TempController (inheritance)
 *   classes/alarm.st            — Alarm class (pointer out-param pattern)
 *   utils/math_utils.st         — Clamp, ScalePercent utility functions
 *   main.st                     — This file: orchestrates everything
 *
 * Run with:
 *   st-cli run playground/oop_project/ -n 100
 *)

VAR_GLOBAL
    // Sensor outputs
    g_raw_temp       : INT;
    g_filtered_temp  : INT;
    g_sensor_samples : INT;
    g_temp_min       : INT;
    g_temp_max       : INT;

    // Controller outputs
    g_ctrl_enabled   : INT;
    g_ctrl_output    : INT;
    g_setpoint       : INT;

    // Alarm outputs
    g_alarm_active   : INT;
    g_alarm_trips    : INT;

    // Process
    g_cycle          : INT;
    g_temp_pct       : INT;
END_VAR

PROGRAM Main
VAR
    // Instances of classes defined in other files
    tempSensor : Sensor;
    controller : TempController;
    highAlarm  : Alarm;

    // Local state
    cycle      : INT := 0;
    simTemp    : INT := 200;    // raw ADC units (0-1000)
    heater     : INT := 0;
    alarmFlag  : INT := 0;
    alarmTrips : INT := 0;
END_VAR
    cycle := cycle + 1;
    g_cycle := cycle;

    // ── Configure on first cycle ──
    IF cycle = 1 THEN
        controller.Configure(sp := 500, gain := 15, oMin := 0, oMax := 100);
        controller.Enable();
        highAlarm.Configure(limit := 800, hyst := 50);
    END_IF;

    // ── Simulate temperature sensor ──
    // Base trend: ramp up then stabilize
    IF cycle < 30 THEN
        simTemp := 200 + cycle * 20;
    ELSE
        simTemp := 700;
    END_IF;
    // Apply heater feedback
    simTemp := simTemp + heater;
    // Add disturbance
    IF (cycle MOD 5) < 3 THEN
        simTemp := simTemp + 30;
    ELSE
        simTemp := simTemp - 20;
    END_IF;

    // ── Read and filter sensor (class from classes/sensor.st) ──
    tempSensor.Update(rawValue := simTemp);
    g_raw_temp := tempSensor.GetRaw();
    g_filtered_temp := tempSensor.GetFiltered();
    g_sensor_samples := tempSensor.GetSampleCount();
    g_temp_min := tempSensor.GetMin();
    g_temp_max := tempSensor.GetMax();

    // ── Scale to percentage (function from utils/math_utils.st) ──
    g_temp_pct := ScalePercent(raw := g_filtered_temp, rawMax := 1000);

    // ── Run controller (class from classes/controller.st, extends BaseController) ──
    controller.Compute(pv := g_filtered_temp);
    g_ctrl_enabled := controller.IsEnabled();
    g_ctrl_output := controller.GetOutput();
    g_setpoint := controller.GetSetpoint();

    // Heater response (simplified: controller output reduces simulated temp)
    IF g_ctrl_output > 50 THEN
        heater := -50;
    ELSIF g_ctrl_output > 0 THEN
        heater := -20;
    ELSE
        heater := 0;
    END_IF;

    // ── Check alarm (class from classes/alarm.st, uses pointer out-params) ──
    highAlarm.Evaluate(value := g_filtered_temp);
    highAlarm.WriteStatus(pActive := REF(alarmFlag), pTrips := REF(alarmTrips));
    g_alarm_active := alarmFlag;
    g_alarm_trips := alarmTrips;

    // Reset sensor stats every 50 cycles
    IF (cycle MOD 50) = 0 THEN
        tempSensor.Reset();
    END_IF;
END_PROGRAM
