(*
 * IControllable — common interface for all controllable devices.
 * Defined in its own file so any class in the project can implement it.
 *)

INTERFACE IControllable
    METHOD Enable
    END_METHOD
    METHOD Disable
    END_METHOD
    METHOD IsEnabled : INT
    END_METHOD
END_INTERFACE
