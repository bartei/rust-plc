(*
 * Alarm class — monitors a value and triggers on threshold.
 * Uses pointer-based output pattern to write status to caller.
 *)

CLASS Alarm IMPLEMENTS IResettable
VAR
    _active    : BOOL := FALSE;
    _limit     : INT := 100;
    _hyst      : INT := 5;
    _tripCount : INT := 0;
END_VAR

    PUBLIC METHOD Configure
    VAR_INPUT limit : INT; hyst : INT; END_VAR
        _limit := limit;
        _hyst := hyst;
    END_METHOD

    PUBLIC METHOD Evaluate
    VAR_INPUT value : INT; END_VAR
    VAR wasActive : BOOL; END_VAR
        wasActive := _active;
        IF value > _limit THEN
            _active := TRUE;
        ELSIF value < (_limit - _hyst) THEN
            _active := FALSE;
        END_IF;
        IF _active AND NOT wasActive THEN
            _tripCount := _tripCount + 1;
        END_IF;
    END_METHOD

    PUBLIC METHOD IsActive : INT
        IF _active THEN IsActive := 1; ELSE IsActive := 0; END_IF;
    END_METHOD

    PUBLIC METHOD GetTripCount : INT
        GetTripCount := _tripCount;
    END_METHOD

    PUBLIC METHOD WriteStatus
    VAR_INPUT pActive : REF_TO INT; pTrips : REF_TO INT; END_VAR
        IF pActive <> NULL THEN
            IF _active THEN pActive^ := 1; ELSE pActive^ := 0; END_IF;
        END_IF;
        IF pTrips <> NULL THEN pTrips^ := _tripCount; END_IF;
    END_METHOD

    PUBLIC METHOD Reset
        _active := FALSE;
        _tripCount := 0;
    END_METHOD
END_CLASS
