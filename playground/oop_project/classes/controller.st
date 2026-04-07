(*
 * Controller classes — base and derived.
 * Demonstrates cross-file inheritance: BaseController → TempController.
 * Uses IControllable interface from interfaces/.
 * Uses Clamp from utils/.
 *)

CLASS BaseController IMPLEMENTS IControllable
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

    PUBLIC METHOD IsEnabled : INT
        IF _enabled THEN IsEnabled := 1; ELSE IsEnabled := 0; END_IF;
    END_METHOD

    PUBLIC METHOD GetOutput : INT
        GetOutput := _output;
    END_METHOD
END_CLASS

CLASS TempController EXTENDS BaseController IMPLEMENTS IResettable
VAR
    _setpoint   : INT := 50;
    _gain       : INT := 20;
    _outMin     : INT := 0;
    _outMax     : INT := 100;
END_VAR

    PUBLIC METHOD Configure
    VAR_INPUT
        sp   : INT;
        gain : INT;
        oMin : INT;
        oMax : INT;
    END_VAR
        _setpoint := sp;
        _gain := gain;
        _outMin := oMin;
        _outMax := oMax;
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

    PUBLIC METHOD GetSetpoint : INT
        GetSetpoint := _setpoint;
    END_METHOD

    PUBLIC METHOD Reset
        _output := 0;
        _enabled := FALSE;
    END_METHOD
END_CLASS
