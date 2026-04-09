# Project Configuration

A `plc-project.yaml` file at the root of a project directory tells `st-cli`,
the LSP, the DAP debug server, and the runtime how to discover sources, what
to run, and how the scan cycle should behave. The file is detected by walking
up from any source file, so all the toolchain pieces see the same project.

## Minimal example

```yaml
name: MyProject
version: "1.0.0"
entryPoint: Main
```

## Full schema

The full set of top-level keys is documented in
`schemas/plc-project.schema.json` and is auto-completed in VS Code via the
`yaml-language-server` schema directive at the top of the file:

```yaml
# yaml-language-server: $schema=../../schemas/plc-project.schema.json
```

| Key           | Type    | Description                                                            |
|---------------|---------|------------------------------------------------------------------------|
| `name`        | string  | Project name. Used in CLI output. Required.                            |
| `version`     | string  | Semantic version string.                                               |
| `entryPoint`  | string  | Name of the `PROGRAM` to run. Defaults to the first one found.         |
| `target`      | string  | Build target. Currently `host` only.                                   |
| `sources`     | array   | Explicit list of source files / globs. Otherwise auto-discovered.      |
| `libraries`   | array   | Extra library directories.                                             |
| `exclude`     | array   | Patterns to exclude from auto-discovery.                               |
| `engine`      | object  | Scan cycle engine settings — see below.                                |
| `links`       | array   | Communication links (TCP, serial, simulated).                          |
| `devices`     | array   | Communication devices on those links.                                  |

## Scan cycle: `engine.cycle_time`

The `engine.cycle_time` setting controls the **scan cycle period** — the time
between the start of one scan cycle and the start of the next. This is the
single most important runtime setting on a real PLC, and rust-plc honors it
the same way:

```yaml
engine:
  cycle_time: 10ms
```

When set, the engine measures how long each cycle takes and sleeps the
difference so the *total* period (execution + sleep) matches the target. If
a single cycle exceeds the target, the next cycle starts immediately — no
catch-up sleep accumulation.

When **omitted**, the engine runs as fast as the CPU allows. This is fine for
unit tests, throughput benchmarks, or `st-cli run -n 10000`-style scripted
runs, but not for code that controls real hardware or talks to simulated
devices on a UI loop.

### Accepted formats

| Value      | Meaning                                                |
|------------|--------------------------------------------------------|
| `10ms`     | 10 milliseconds                                        |
| `500us`    | 500 microseconds                                       |
| `500µs`    | Same as `500us` — Unicode µ accepted                   |
| `1s`       | 1 second                                               |
| `250ns`    | 250 nanoseconds                                        |
| `5`        | Bare number → milliseconds (so `5` ≡ `5ms`)            |

### Where it applies

- **`st-cli run`** — `Engine::run` reads `engine.cycle_time` from the project
  YAML and `std::thread::sleep`s after each cycle.
- **`st-cli debug` / VS Code DAP sessions** — the DAP run loop honors the
  same setting. The sleep is broken into 10ms chunks that poll the request
  channel between chunks, so `Pause` and `Disconnect` from the IDE remain
  responsive even at long cycle times.

### Example: simulated PLC at 10ms

The bundled `playground/sim_project` demonstrates this exact pattern with
two simulated devices on web UIs:

```yaml
# playground/sim_project/plc-project.yaml
name: SimulatedIO
version: "1.0.0"
entryPoint: Main

engine:
  cycle_time: 10ms

links:
  - name: sim_link
    type: simulated

devices:
  - name: io_rack
    link: sim_link
    protocol: simulated
    mode: cyclic
    device_profile: sim_8di_4ai_4do_2ao
  - name: pump_vfd
    link: sim_link
    protocol: simulated
    mode: cyclic
    device_profile: sim_vfd
```

Run it with:

```bash
$ cd playground/sim_project
$ st-cli run -n 50
[COMM] Generated I/O map: ./_io_map.st (2 device(s))
Project 'SimulatedIO': 2 source file(s)
[ENGINE] cycle_time: 10ms
[COMM] Registered 2 simulated device(s)
[SIM-WEB] Device 'io_rack' web UI at http://localhost:8080
[SIM-WEB] Device 'pump_vfd' web UI at http://localhost:8081
Executed 50 cycle(s) in 508.216525ms wall (3.311467ms cpu, avg 66.229µs/cycle exec, 39 instructions)
```

50 cycles × 10ms = ~500ms of wall time, regardless of how fast the CPU could
have run them otherwise. The CLI reports both numbers when `cycle_time` is
set: **wall** is the total time including the inter-cycle sleep, **cpu** is
the actual VM execution time per cycle. The ratio tells you how much
headroom you have before the cycle budget is exhausted — in this run,
3.3ms / 500ms ≈ 0.7%, so the CPU is idle 99.3% of the time.

Open the device web UIs at <http://localhost:8080> and <http://localhost:8081>
and toggle inputs while the program runs — the toggles propagate through the
ST program at exactly the rate you configured.

### Jitter: measuring cycle timing accuracy

When `cycle_time` is set, the engine tracks **jitter** — the deviation of
each actual cycle period from the configured target. This is critical for
time-sensitive control loops (PID, temperature, position) where the integral
or derivative terms depend on a consistent sample interval.

**Definitions:**

| Metric | Meaning |
|--------|---------|
| **Period** | Wall-clock interval between the *start* of one cycle and the *start* of the next (execution + sleep). This is what control algorithms see. |
| **Cycle time** | Pure VM execution time per cycle (what the engine measured before `cycle_time` was introduced). |
| **Jitter** | `max(|period - target|)` — the worst absolute deviation of any observed period from the configured `cycle_time`. |

The engine reports **period**, not just cycle time, because they differ by
the inter-cycle sleep. A cycle that executes in 200µs and targets 10ms has a
period of ~10ms (200µs execution + 9.8ms sleep). The jitter comes from
variation in the sleep's accuracy (OS scheduler granularity, other processes,
GC pauses, etc.).

**Where jitter is surfaced:**

| Surface | How to access |
|---------|---------------|
| Debug Console REPL | Type `scanCycleInfo` — shows `jitter: Nµs`, `period: Nµs (min/max)` |
| VS Code status bar | Hover the `$(pulse) PLC ...` widget — tooltip shows jitter, period, and target |
| `plc/cycleStats` telemetry | Fields: `jitter_max_us`, `last_period_us`, `min_period_us`, `max_period_us`, `target_us` (schema v2) |
| CLI output | `st-cli run` reports wall time vs cpu time — the ratio shows headroom |
| Future: `/api/diagnostics` | Phase 13a.1 will expose jitter on the HTTP JSON endpoint for FUXA/Node-RED |

**Interpreting jitter for control loops:**

- **< 100µs** — excellent. Suitable for servo drives, high-speed position control.
- **100µs – 1ms** — good. Fine for most PID loops (temperature, pressure, flow).
- **1ms – 5ms** — acceptable for slow processes with large time constants.
- **> 5ms** — investigate. Common causes: other processes competing for CPU,
  OS power management throttling the core, or the cycle execution itself
  exceeds the target budget (check `min_us` / `max_us` in the stats).

**Note:** Jitter measurement is only meaningful when `cycle_time` is set. In
free-run mode (no target), the engine runs as fast as possible and periods
vary with instruction count; "jitter" in that context is just normal
execution-time variation, not a quality indicator.

### Indefinite debug sessions

When debugging from VS Code (`F5`), there is **no upper bound** on how long
a session can stay connected. The `Continue` command runs the program forever
until the user pauses, sets a breakpoint, or disconnects — exactly like a
real PLC engineer expects. Cycle counters and statistics are stored in `u64`
fields so they remain precise for any practical session length.

A 10-million-cycle safety net protects against runaway loops in tests and CI;
at a 10ms cycle time that's ~28 hours of continuous execution before the cap
is reached, well past any interactive use.
