  ---                                                    
Manual Verification Checklist

1. IDE Code Completion

Open playground/native_fb_demo/main.st in your JetBrains IDE and verify:

- [X] io. triggers dot-completion showing: refresh_rate, connected, error_code, io_cycles, last_response_ms, DI_0..DI_3, AI_0, DO_0, DO_1, AO_0
- [X] Hover over io shows FUNCTION_BLOCK DemoIo(refresh_rate) => (connected, error_code, ...)
- [X] Hover over io.DI_0 shows type BOOL
- [X] Typing io.NONEXISTENT produces a red squiggly / "no member" diagnostic
- [X] No false errors on the existing program — the file should show zero problems

2. CLI Run

cargo run -p st-cli -- run playground/native_fb_demo -n 10

- [X] Compiles without errors
- [X] Reports "Loaded 3 native FB type(s) from profiles" (or similar)
- [X] Runs 10 cycles and reports execution stats
- [X] Web UI starts on port 8090+ (open http://localhost:8090 in browser)

3. CLI Check

cargo run -p st-cli -- check playground/native_fb_demo

- [X] Reports "OK" with no errors

4. Debugger (DAP)

Open playground/native_fb_demo/ in VS Code with the ST extension, set a breakpoint on io.DO_0 := io.DI_0;, and launch the debugger:

- [X] Program stops at the breakpoint
- [X] Variables panel shows io as an expandable FB instance 
- [X] Expanding io shows all fields: refresh_rate, connected, DI_0, DO_0, etc. 
- [X] Field values update when stepping (e.g., cycle increments) 
- [X] Can write to io.DI_0 via the debug console or watch panel

5. Existing Projects Still Work

Verify the old comm path hasn't regressed:

cargo run -p st-cli -- run playground/sim_project -n 5

- [X] Compiles and runs (uses legacy flat-global path)
- [X] Web UIs start on ports 8080/8081

cargo run -p st-cli -- check playground/multi_file_project

- [X] Reports "OK"

6. Bundle and Deploy

cargo run -p st-cli -- bundle playground/native_fb_demo

- [X] Creates .st-bundle file without errors
- [x] Inspect it: cargo run -p st-cli -- bundle inspect NativeFbDemo.st-bundle
- [X] Shows "NativeFbDemo", version, file list including profiles/demo_io.yaml

  Bundle: NativeFbDemo.st-bundle
  Name:     NativeFbDemo
  Version:  1.0.0
  Mode:     development
  Compiled: 2026-04-19T01:39:57.958716695+00:00
  Compiler: 0.1.1
  Entry:    Main
  Checksum: c41e148df053db10 (valid)
  Size:     6435 bytes

Files:
29.5 KB  debug.map
301 B  manifest.yaml
81 B  plc-project.yaml
982 B  profiles/demo_io.yaml
81.5 KB  program.stc
667 B  source/main.st
7. Custom Device Profile

Create a new profile in playground/native_fb_demo/profiles/my_sensor.yaml:

name: MySensor                                                                                                                                                                                                                                
protocol: simulated                                    
fields:
- name: TEMP
type: REAL
direction: input                                                                                                                                                                                                                          
register: { address: 0, kind: virtual, unit: "°C" }
- name: ALARM                                                                                                                                                                                                                               
type: BOOL                                         
direction: output                                                                                                                                                                                                                         
register: { address: 1, kind: virtual }

Then in main.st, add sensor : MySensor; and sensor(); sensor.ALARM := sensor.TEMP > 80.0;:

- [X] IDE immediately recognizes MySensor as a type (completions work)
- [X] st-cli check passes
- [X] st-cli run executes without errors

8. Negative Tests

In playground/native_fb_demo/main.st, try:

- [X] io.NONEXISTENT := TRUE; — should produce a "no member" error in IDE and st-cli check
- [X] io.DI_0 := "hello"; — should produce a type mismatch error
- Remove the profiles/ directory and run st-cli check — should still compile (no profiles = no native FB types, but DemoIo becomes undeclared)      