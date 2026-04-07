# Classes, Methods & Interfaces

IEC 61131-3 Third Edition introduces object-oriented extensions. A `CLASS` is a stateful Program Organisation Unit — like a `FUNCTION_BLOCK` — but with explicit methods, access control, inheritance, and interface implementation.

## Class Declaration

```st
CLASS ClassName
  VAR_INPUT
    setpoint : INT;            (* supplied by caller *)
  END_VAR
  VAR_OUTPUT
    output : INT;              (* readable via dot notation *)
  END_VAR
  VAR
    _internal : INT := 0;     (* private persistent state *)
  END_VAR

  METHOD MethodName : INT      (* methods define behavior *)
  VAR_INPUT
    param : INT;
  END_VAR
    MethodName := _internal + param;
  END_METHOD
END_CLASS
```

## Instantiation

Classes are instantiated by declaring variables of the class type. Each instance has completely independent state.

```st
PROGRAM Main
  VAR
    ctrl1 : MotorController;
    ctrl2 : MotorController;
  END_VAR

  ctrl1.Start();
  ctrl2.Stop();
  (* ctrl1 and ctrl2 have separate internal state *)
END_PROGRAM
```

## Methods

Methods define the behavior of a class. They can accept parameters, return values, and access the class's variables directly.

### Void methods (no return value)

```st
CLASS Counter
  VAR
    _count : INT := 0;
  END_VAR

  METHOD Increment
    _count := _count + 1;
  END_METHOD

  METHOD Reset
    _count := 0;
  END_METHOD
END_CLASS
```

### Methods with return values

Name the return type after a colon. Assign the return value using the method name:

```st
  METHOD GetCount : INT
    GetCount := _count;
  END_METHOD

  METHOD Add : INT
  VAR_INPUT
    a : INT;
    b : INT;
  END_VAR
    Add := a + b;
  END_METHOD
```

### Calling methods

Use dot notation with named or positional arguments:

```st
  counter.Increment();
  val := counter.GetCount();
  sum := counter.Add(a := 10, b := 20);
```

## Access Specifiers

Control member visibility with `PUBLIC`, `PRIVATE`, `PROTECTED`, or `INTERNAL`:

```st
CLASS Sensor
  VAR
    _raw : INT := 0;
  END_VAR

  PUBLIC METHOD GetValue : INT
    GetValue := _raw;
  END_METHOD

  PRIVATE METHOD Calibrate
    _raw := _raw + 1;
  END_METHOD

  PROTECTED METHOD InternalUpdate
    (* accessible within this class and subclasses *)
  END_METHOD
END_CLASS
```

If omitted, methods default to `PUBLIC`.

## Interfaces

An `INTERFACE` declares a contract — a set of method signatures that implementing classes must provide.

```st
INTERFACE IResettable
  METHOD Reset
  END_METHOD
END_INTERFACE

INTERFACE IControllable
  METHOD Enable
  END_METHOD
  METHOD Disable
  END_METHOD
END_INTERFACE
```

### Implementing interfaces

Use `IMPLEMENTS` to declare that a class fulfils an interface. The compiler verifies that all required methods are present:

```st
CLASS Motor IMPLEMENTS IControllable, IResettable
  VAR
    _running : BOOL := FALSE;
  END_VAR

  METHOD Enable
    _running := TRUE;
  END_METHOD

  METHOD Disable
    _running := FALSE;
  END_METHOD

  METHOD Reset
    _running := FALSE;
  END_METHOD
END_CLASS
```

### Interface inheritance

Interfaces can extend other interfaces:

```st
INTERFACE IFullCounter EXTENDS IResettable
  METHOD Increment
  END_METHOD
  METHOD GetCount : INT
  END_METHOD
END_INTERFACE
```

## Inheritance

Use `EXTENDS` to create a subclass that inherits all variables and methods from a base class:

```st
CLASS Base
  VAR
    _value : INT := 0;
  END_VAR

  METHOD GetValue : INT
    GetValue := _value;
  END_METHOD
END_CLASS

CLASS Derived EXTENDS Base
  VAR
    _extra : INT := 0;
  END_VAR

  METHOD GetSum : INT
    (* can access both inherited _value and own _extra *)
    GetSum := _value + _extra;
  END_METHOD
END_CLASS
```

### Calling inherited methods

A derived instance can call methods defined in any ancestor class:

```st
VAR d : Derived; END_VAR
  val := d.GetValue();    (* inherited from Base *)
  sum := d.GetSum();      (* defined in Derived *)
```

### Overriding methods

Use `OVERRIDE` to replace a parent method's behavior:

```st
CLASS Base
  METHOD Process : INT
    Process := 0;
  END_METHOD
END_CLASS

CLASS Derived EXTENDS Base
  OVERRIDE METHOD Process : INT
    Process := 42;    (* replaces Base.Process *)
  END_METHOD
END_CLASS
```

The compiler checks that an overridden method actually exists in the base class.

## Abstract Classes

An `ABSTRACT` class cannot be instantiated directly. It defines methods that subclasses **must** implement:

```st
ABSTRACT CLASS Shape
  ABSTRACT METHOD Area : REAL
  END_METHOD

  METHOD Describe : INT
    Describe := 1;    (* concrete method — inherited as-is *)
  END_METHOD
END_CLASS

CLASS Rectangle EXTENDS Shape
  VAR
    _w : REAL := 0.0;
    _h : REAL := 0.0;
  END_VAR

  METHOD SetSize
  VAR_INPUT w : REAL; h : REAL; END_VAR
    _w := w;  _h := h;
  END_METHOD

  OVERRIDE METHOD Area : REAL
    Area := _w * _h;   (* must implement abstract method *)
  END_METHOD
END_CLASS
```

Abstract methods have no body — only the `ABSTRACT METHOD ... END_METHOD` signature.

## Final Classes and Methods

`FINAL` prevents further extension or overriding:

```st
FINAL CLASS Singleton
  (* this class cannot be extended *)
END_CLASS

CLASS Base
  FINAL METHOD Locked : INT
    Locked := 42;   (* this method cannot be overridden *)
  END_METHOD
END_CLASS
```

## Properties

Properties provide getter/setter syntax for encapsulated field access:

```st
CLASS Thermostat
  VAR
    _setpoint : INT := 20;
  END_VAR

  PROPERTY Setpoint : INT
    GET
      Setpoint := _setpoint;
    END_GET
    SET
      IF Setpoint >= 0 THEN
        _setpoint := Setpoint;
      END_IF;
    END_SET
  END_PROPERTY
END_CLASS
```

Read-only properties omit the `SET` block.

## Pointers and Classes

Class methods can accept and use pointers to modify external state:

```st
CLASS Logger
  VAR _count : INT := 0; END_VAR

  METHOD WriteCount
  VAR_INPUT target : REF_TO INT; END_VAR
    IF target <> NULL THEN
      target^ := _count;
    END_IF;
  END_METHOD

  METHOD Log
    _count := _count + 1;
  END_METHOD
END_CLASS
```

## Multi-File Projects

Classes, interfaces, and functions can be split across files. The two-pass compiler resolves forward references automatically — file order does not matter:

```
project/
  interfaces/resettable.st     (* INTERFACE IResettable *)
  classes/sensor.st            (* CLASS Sensor IMPLEMENTS IResettable *)
  classes/controller.st        (* CLASS TempController EXTENDS BaseController *)
  utils/math.st                (* FUNCTION Clamp *)
  main.st                      (* PROGRAM Main — uses all of the above *)
  plc-project.yaml
```

See `playground/oop_project/` for a complete multi-file example.

## Realistic Example

A complete PID-like controller using classes, inheritance, and interfaces:

```st
INTERFACE IResettable
  METHOD Reset
  END_METHOD
END_INTERFACE

CLASS BaseController IMPLEMENTS IResettable
  VAR
    _enabled : BOOL := FALSE;
    _output  : INT := 0;
  END_VAR

  METHOD Enable
    _enabled := TRUE;
  END_METHOD

  METHOD Disable
    _enabled := FALSE;
    _output := 0;
  END_METHOD

  METHOD GetOutput : INT
    GetOutput := _output;
  END_METHOD

  METHOD Reset
    _enabled := FALSE;
    _output := 0;
  END_METHOD
END_CLASS

CLASS TempController EXTENDS BaseController
  VAR
    _setpoint : INT := 50;
    _gain     : INT := 20;
  END_VAR

  METHOD Configure
  VAR_INPUT sp : INT; gain : INT; END_VAR
    _setpoint := sp;
    _gain := gain;
  END_METHOD

  METHOD Compute
  VAR_INPUT pv : INT; END_VAR
  VAR error : INT; END_VAR
    IF _enabled THEN
      error := _setpoint - pv;
      _output := (error * _gain) / 10;
      IF _output < 0 THEN _output := 0; END_IF;
      IF _output > 100 THEN _output := 100; END_IF;
    ELSE
      _output := 0;
    END_IF;
  END_METHOD
END_CLASS

PROGRAM Main
  VAR
    ctrl : TempController;
    simTemp : INT := 30;
  END_VAR

  ctrl.Configure(sp := 50, gain := 15);
  ctrl.Enable();
  ctrl.Compute(pv := simTemp);
  (* ctrl.GetOutput() returns the control action *)
END_PROGRAM
```

## Comparison: CLASS vs FUNCTION_BLOCK

| Aspect               | `FUNCTION_BLOCK`          | `CLASS`                         |
|----------------------|---------------------------|---------------------------------|
| State persistence    | Yes                       | Yes                             |
| Methods              | Single body only          | Named methods with access control |
| Inheritance          | No                        | `EXTENDS` single inheritance    |
| Interfaces           | No                        | `IMPLEMENTS` one or more        |
| Abstract/Final       | No                        | `ABSTRACT`, `FINAL` modifiers   |
| Properties           | No                        | `PROPERTY` with `GET`/`SET`     |
| Calling convention   | `fb(input := val);`       | `obj.Method(param := val);`     |
| Output access        | `fb.output`               | `obj.Method()` or `obj.field`   |

## Playground Examples

- `playground/10_classes.st` — basic OOP feature tour
- `playground/11_class_patterns.st` — industrial patterns (state machines, alarm managers, 3-level inheritance)
- `playground/14_class_instances.st` — lifecycle, composition, producer-consumer
- `playground/oop_project/` — multi-file project with classes across files

```bash
st-cli run playground/10_classes.st -n 10
st-cli run playground/oop_project/ -n 100
```
