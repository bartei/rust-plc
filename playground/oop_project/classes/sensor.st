(*
 * Sensor class — reads and filters analog input.
 * Implements IResettable.
 * Uses Clamp from utils/math_utils.st.
 *)

CLASS Sensor IMPLEMENTS IResettable
VAR
    _raw      : INT := 0;
    _filtered : INT := 0;
    _count    : INT := 0;
    _min      : INT := 32767;
    _max      : INT := 0;
END_VAR

    PUBLIC METHOD Update
    VAR_INPUT rawValue : INT; END_VAR
    VAR clamped : INT; END_VAR
        clamped := Clamp(val := rawValue, lo := 0, hi := 1000);
        _raw := clamped;
        // Simple EMA filter: 25% new + 75% old
        IF _count = 0 THEN
            _filtered := clamped;
        ELSE
            _filtered := (25 * clamped + 75 * _filtered) / 100;
        END_IF;
        _count := _count + 1;
        IF clamped < _min THEN _min := clamped; END_IF;
        IF clamped > _max THEN _max := clamped; END_IF;
    END_METHOD

    PUBLIC METHOD GetFiltered : INT
        GetFiltered := _filtered;
    END_METHOD

    PUBLIC METHOD GetRaw : INT
        GetRaw := _raw;
    END_METHOD

    PUBLIC METHOD GetMin : INT
        GetMin := _min;
    END_METHOD

    PUBLIC METHOD GetMax : INT
        GetMax := _max;
    END_METHOD

    PUBLIC METHOD GetSampleCount : INT
        GetSampleCount := _count;
    END_METHOD

    PUBLIC METHOD Reset
        _raw := 0;
        _filtered := 0;
        _count := 0;
        _min := 32767;
        _max := 0;
    END_METHOD
END_CLASS
