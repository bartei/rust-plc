(*
 * IEC 61131-3 Standard Library: Counters
 *
 * CTU  — Count Up
 * CTD  — Count Down
 * CTUD — Count Up/Down
 *)

FUNCTION_BLOCK CTU
VAR_INPUT
    CU    : BOOL;   (* Count up input — counts on rising edge *)
    RESET : BOOL;   (* Reset counter to 0 *)
    PV    : INT;    (* Preset value — Q goes TRUE when CV >= PV *)
END_VAR
VAR_OUTPUT
    Q     : BOOL;   (* Output — TRUE when CV >= PV *)
    CV    : INT;    (* Current counter value *)
END_VAR
VAR
    prev_cu : BOOL := FALSE;
END_VAR
    IF RESET THEN
        CV := 0;
    ELSIF CU AND NOT prev_cu THEN
        CV := CV + 1;
    END_IF;
    prev_cu := CU;
    Q := CV >= PV;
END_FUNCTION_BLOCK

FUNCTION_BLOCK CTD
VAR_INPUT
    CD   : BOOL;   (* Count down input — counts on rising edge *)
    LOAD : BOOL;   (* Load preset value *)
    PV   : INT;    (* Preset value *)
END_VAR
VAR_OUTPUT
    Q    : BOOL;   (* Output — TRUE when CV <= 0 *)
    CV   : INT;    (* Current counter value *)
END_VAR
VAR
    prev_cd : BOOL := FALSE;
END_VAR
    IF LOAD THEN
        CV := PV;
    ELSIF CD AND NOT prev_cd THEN
        CV := CV - 1;
    END_IF;
    prev_cd := CD;
    Q := CV <= 0;
END_FUNCTION_BLOCK

FUNCTION_BLOCK CTUD
VAR_INPUT
    CU    : BOOL;   (* Count up input *)
    CD    : BOOL;   (* Count down input *)
    RESET : BOOL;   (* Reset to 0 *)
    LOAD  : BOOL;   (* Load preset value *)
    PV    : INT;    (* Preset value *)
END_VAR
VAR_OUTPUT
    QU    : BOOL;   (* Up output — TRUE when CV >= PV *)
    QD    : BOOL;   (* Down output — TRUE when CV <= 0 *)
    CV    : INT;    (* Current counter value *)
END_VAR
VAR
    prev_cu : BOOL := FALSE;
    prev_cd : BOOL := FALSE;
END_VAR
    IF RESET THEN
        CV := 0;
    ELSIF LOAD THEN
        CV := PV;
    ELSE
        IF CU AND NOT prev_cu THEN
            CV := CV + 1;
        END_IF;
        IF CD AND NOT prev_cd THEN
            CV := CV - 1;
        END_IF;
    END_IF;
    prev_cu := CU;
    prev_cd := CD;
    QU := CV >= PV;
    QD := CV <= 0;
END_FUNCTION_BLOCK
