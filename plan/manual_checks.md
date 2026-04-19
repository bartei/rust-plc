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
- [ ] Can write to io.DI_0 via the debug console or watch panel -> found two issues, when there are no variables in the watch list the scan cycle data is not updating. i tried forcing io.DI_0 but it's not accepting the input value "TRUE" or 1

5. Existing Projects Still Work

Verify the old comm path hasn't regressed:

cargo run -p st-cli -- run playground/sim_project -n 5

- Compiles and runs (uses legacy flat-global path)
- Web UIs start on ports 8080/8081

cargo run -p st-cli -- check playground/multi_file_project

- Reports "OK"

6. Bundle and Deploy

cargo run -p st-cli -- bundle playground/native_fb_demo

- Creates .st-bundle file without errors
- Inspect it: cargo run -p st-cli -- bundle inspect playground/native_fb_demo/NativeFbE2E.st-bundle
- Shows "NativeFbDemo", version, file list including profiles/demo_io.yaml

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

- IDE immediately recognizes MySensor as a type (completions work)
- st-cli check passes
- st-cli run executes without errors

8. Negative Tests

In playground/native_fb_demo/main.st, try:

- io.NONEXISTENT := TRUE; — should produce a "no member" error in IDE and st-cli check
- io.DI_0 := "hello"; — should produce a type mismatch error
- Remove the profiles/ directory and run st-cli check — should still compile (no profiles = no native FB types, but DemoIo becomes undeclared)      