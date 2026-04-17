# Native Compilation & Hardware Targets — Implementation Plan

> **Parent plan:** [implementation.md](implementation.md) — core platform, Phases 0-12.
> **See also:** [implementation_comm.md](implementation_comm.md) — communication layer (Phase 13).
> **See also:** [implementation_deploy.md](implementation_deploy.md) — remote deployment (Phase 15).

---

## Phase 14a: LLVM Native Compilation Backend

- [ ] Integrate `inkwell` (Rust LLVM bindings)
- [ ] IR → LLVM IR lowering for all 50+ bytecode instructions
- [ ] JIT compilation for development mode (fast iteration on host)
- [ ] AOT cross-compilation for embedded targets (ARM Cortex-M, RISC-V, Xtensa)
- [ ] Adapt online change for native code (requires careful relocation strategy)
- [ ] Benchmark: VM interpreter vs LLVM-compiled cycle times

---

## Phase 14b: Hardware Target Platform System

### Platform registry

- [ ] Discover and load platform extensions from `platforms/` directory
- [ ] `platform.yaml` schema: name, vendor, arch, llvm_target, flash/ram/clock, hal crate, peripherals list, build settings

### Peripheral definitions

- [ ] Peripheral YAML schema for GPIO, UART, SPI, I2C, ADC, DAC, PWM, timer
- [ ] GPIO: pin number, mode (input/output/alternate), pull-up/down, alias
- [ ] UART: channel, baud, tx/rx pins, alias
- [ ] SPI: bus, clk/miso/mosi/cs pins, speed, alias
- [ ] I2C: bus, sda/scl pins, speed, alias
- [ ] ADC: channel, pin, resolution, attenuation, alias
- [ ] DAC: channel, pin, alias
- [ ] PWM: channel, pin, frequency, alias

### Config-to-ST generator

- [ ] Read user's `plc-project.yaml` peripheral config
- [ ] Generate `VAR_GLOBAL` declarations with hardware-mapped names
- [ ] Auto-include generated globals in compilation

### LLVM cross-compilation

- [ ] Target triple selection from platform.yaml
- [ ] Linker script and startup code integration
- [ ] `st-cli build --target esp32` compiles to flashable binary

### Platform HAL runtime

- [ ] Scan cycle integration: read physical inputs → execute program → write physical outputs
- [ ] Map ST global variable slots to hardware register addresses
- [ ] Interrupt-safe I/O access

### Platform-specific stdlib

- [ ] Each platform can ship additional `.st` files (e.g., WiFi FBs, BLE FBs)

### CLI integration

- [ ] `st-cli build --target esp32` — cross-compile for target
- [ ] `st-cli flash --target esp32` — compile and flash to device
- [ ] `st-cli targets` — list available platform extensions
- [ ] `st-cli target-info esp32` — show peripherals, pins, capabilities

### Initial platform implementations

- [ ] ESP32 (Xtensa, via esp-hal)
- [ ] STM32F103 (ARM Cortex-M3, via stm32f1xx-hal)
- [ ] Raspberry Pi (Linux/ARM64, via rppal)
- [ ] Raspberry Pi Pico / RP2040 (ARM Cortex-M0+, via embassy-rp)
- [ ] Generic RISC-V (via riscv-hal)

### Tests

- [ ] Platform discovery and loading
- [ ] Peripheral config parsing and validation
- [ ] Config-to-ST generation (verify correct VAR_GLOBAL output)
- [ ] Cross-compilation smoke test (compile to ELF, verify target arch)
- [ ] Platform-specific stdlib compilation

### Documentation

- [ ] "Creating a Platform Extension" guide
- [ ] Per-platform quickstart (ESP32, STM32, RPi, Pico)
- [ ] Peripheral configuration reference
- [ ] Hardware I/O mapping tutorial