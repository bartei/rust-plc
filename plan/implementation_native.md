# Native Compilation & Hardware Targets — Implementation Plan

> **Parent plan:** [implementation.md](implementation.md) — core platform, Phases 0-12.
> **See also:** [implementation_comm.md](implementation_comm.md) — communication layer (Phase 13).
> **See also:** [implementation_deploy.md](implementation_deploy.md) — remote deployment (Phase 15).

## Phase 14 (Future): Native Compilation & Hardware Target Platform System

Two major capabilities: (1) LLVM native compilation backend, and (2) a plugin-based platform system
that lets each hardware target define its peripherals, I/O mapping, and compilation settings as a
self-contained extension — no framework recompilation required.

### 14a: LLVM Native Compilation Backend

- [ ] Integrate `inkwell` (Rust LLVM bindings)
- [ ] IR → LLVM IR lowering for all 50+ bytecode instructions
- [ ] JIT compilation for development mode (fast iteration on host)
- [ ] AOT cross-compilation for embedded targets (ARM Cortex-M, RISC-V, Xtensa)
- [ ] Adapt online change for native code (requires careful relocation strategy)
- [ ] Benchmark: VM interpreter vs LLVM-compiled cycle times

### 14b: Hardware Target Platform System

The platform system allows each hardware target (ESP32, STM32, Raspberry Pi, etc.) to be defined
as a **platform extension** — a self-contained package that provides:
1. **Compilation target**: LLVM triple, linker scripts, startup code
2. **Peripheral definitions**: typed ST variables/FBs that map to hardware registers
3. **Configuration schema**: user-configurable pin assignments, clock settings, peripheral modes
4. **Runtime HAL**: hardware abstraction layer bridging ST I/O to physical pins

A platform extension is loaded at compile time — the user selects a target in `plc-project.yaml`
and the platform's peripheral definitions become available as typed variables in their ST code.
No recompilation of the rust-plc framework is needed to add new platforms.

#### Architecture

```
plc-project.yaml
  target: esp32-wroom-32
  peripherals:
    gpio:
      pin_2: { mode: output, alias: LED }
      pin_4: { mode: input, pull: up, alias: BUTTON }
    uart:
      uart0: { baud: 115200, tx: 1, rx: 3 }
    adc:
      adc1_ch0: { pin: 36, attenuation: 11db, alias: TEMP_SENSOR }

↓ Platform extension generates:

VAR_GLOBAL
    LED           : BOOL;        (* GPIO2 output — mapped by platform *)
    BUTTON        : BOOL;        (* GPIO4 input — mapped by platform *)
    TEMP_SENSOR   : INT;         (* ADC1_CH0 — mapped by platform *)
    UART0_TX_DATA : STRING[256]; (* UART0 transmit buffer *)
END_VAR
```

The user's ST program reads/writes these variables like any other global.
The platform runtime maps them to hardware registers in the scan cycle.

#### Platform Extension Structure

```
platforms/
├── esp32/
│   ├── platform.yaml          # Platform metadata + LLVM triple
│   ├── peripherals/
│   │   ├── gpio.yaml          # GPIO pin definitions, modes, pull-up/down
│   │   ├── uart.yaml          # UART channels, baud rates, pin mappings
│   │   ├── spi.yaml           # SPI bus definitions
│   │   ├── i2c.yaml           # I2C bus definitions
│   │   ├── adc.yaml           # ADC channels, resolution, attenuation
│   │   ├── dac.yaml           # DAC channels
│   │   ├── pwm.yaml           # PWM/LEDC channels
│   │   └── timer.yaml         # Hardware timer definitions
│   ├── stdlib/                # Platform-specific ST function blocks
│   │   ├── esp_wifi.st        # WiFi connection FB
│   │   ├── esp_ble.st         # BLE communication FB
│   │   └── esp_sleep.st       # Deep sleep control
│   ├── hal/                   # Rust HAL implementation
│   │   └── lib.rs             # Maps ST globals ↔ hardware registers
│   ├── linker.ld              # Linker script for the target
│   └── startup.s              # Startup / vector table
├── stm32f103/
│   ├── platform.yaml
│   ├── peripherals/
│   │   ├── gpio.yaml          # PA0-PA15, PB0-PB15, PC13, etc.
│   │   ├── uart.yaml          # USART1, USART2, USART3
│   │   ├── spi.yaml           # SPI1, SPI2
│   │   ├── i2c.yaml           # I2C1, I2C2
│   │   ├── adc.yaml           # ADC1 (10 channels)
│   │   ├── pwm.yaml           # TIM1-TIM4 PWM channels
│   │   └── can.yaml           # CAN bus
│   ├── stdlib/
│   │   └── stm32_flash.st     # Flash read/write FB
│   ├── hal/
│   │   └── lib.rs
│   └── linker.ld
├── raspberry-pi/
│   ├── platform.yaml
│   ├── peripherals/
│   │   ├── gpio.yaml          # BCM GPIO 0-27
│   │   ├── uart.yaml          # /dev/ttyAMA0, /dev/ttyS0
│   │   ├── spi.yaml           # SPI0, SPI1
│   │   ├── i2c.yaml           # I2C1
│   │   └── pwm.yaml           # Hardware PWM channels
│   ├── stdlib/
│   │   └── rpi_camera.st      # Camera interface FB
│   └── hal/
│       └── lib.rs             # Uses rppal or embedded-hal
├── raspberry-pico/
│   ├── platform.yaml          # RP2040 / RP2350
│   ├── peripherals/
│   │   ├── gpio.yaml          # GP0-GP29
│   │   ├── uart.yaml          # UART0, UART1
│   │   ├── spi.yaml           # SPI0, SPI1
│   │   ├── i2c.yaml           # I2C0, I2C1
│   │   ├── adc.yaml           # ADC0-ADC3 + temp sensor
│   │   ├── pwm.yaml           # 16 PWM channels
│   │   └── pio.yaml           # Programmable I/O state machines
│   └── hal/
│       └── lib.rs             # Uses embassy-rp or rp-hal
└── risc-v/                    # Generic RISC-V target
    ├── platform.yaml
    └── hal/
        └── lib.rs
```

#### platform.yaml Schema

```yaml
name: ESP32-WROOM-32
vendor: Espressif
arch: xtensa
llvm_target: xtensa-esp32-none-elf
flash_size: 4MB
ram_size: 520KB
clock_speed: 240MHz

# Rust HAL crate to use for the runtime
hal_crate: esp-hal
hal_version: "0.22"

# Supported peripherals (references files in peripherals/)
peripherals:
  - gpio
  - uart
  - spi
  - i2c
  - adc
  - dac
  - pwm
  - timer

# Build settings
build:
  toolchain: esp       # rustup toolchain
  runner: espflash      # flash tool
  flash_command: "espflash flash --monitor"
```

#### User Configuration in plc-project.yaml

```yaml
name: MyIoTProject
target: esp32

peripherals:
  gpio:
    pin_2:  { mode: output, alias: STATUS_LED }
    pin_4:  { mode: input, pull: up, alias: START_BUTTON }
    pin_5:  { mode: output, alias: MOTOR_EN }
    pin_18: { mode: alternate, function: spi_clk }
    pin_19: { mode: alternate, function: spi_miso }
    pin_23: { mode: alternate, function: spi_mosi }
  uart:
    uart0: { baud: 115200, tx: 1, rx: 3, alias: DEBUG }
    uart2: { baud: 9600, tx: 17, rx: 16, alias: MODBUS }
  adc:
    adc1_ch0: { pin: 36, attenuation: 11db, alias: TEMP_SENSOR }
    adc1_ch3: { pin: 39, attenuation: 11db, alias: PRESSURE }
  spi:
    spi2: { clk: 18, miso: 19, mosi: 23, cs: 15, speed: 1000000, alias: DISPLAY }
```

This generates auto-included ST globals:
```st
(* Auto-generated from platform config — DO NOT EDIT *)
VAR_GLOBAL
    STATUS_LED    : BOOL;    (* GPIO2 output *)
    START_BUTTON  : BOOL;    (* GPIO4 input, pull-up *)
    MOTOR_EN      : BOOL;    (* GPIO5 output *)
    TEMP_SENSOR   : INT;     (* ADC1_CH0, 12-bit, 0-3.3V *)
    PRESSURE      : INT;     (* ADC1_CH3, 12-bit, 0-3.3V *)
END_VAR
```

#### Implementation Plan

- [ ] **Platform registry**: discover and load platform extensions from `platforms/` directory
- [ ] **Peripheral YAML schema**: define the configuration grammar for GPIO, UART, SPI, I2C, ADC, DAC, PWM
- [ ] **Config-to-ST generator**: read user's `plc-project.yaml` peripheral config, generate `VAR_GLOBAL` declarations with hardware-mapped names
- [ ] **LLVM cross-compilation**:
  - [ ] Target triple selection from platform.yaml
  - [ ] Linker script and startup code integration
  - [ ] `st-cli build --target esp32` compiles to flashable binary
- [ ] **Platform HAL runtime**:
  - [ ] Scan cycle integration: read physical inputs → execute program → write physical outputs
  - [ ] Map ST global variable slots to hardware register addresses
  - [ ] Interrupt-safe I/O access
- [ ] **Platform-specific stdlib**: each platform can ship additional `.st` files (e.g., WiFi FBs, BLE FBs)
- [ ] **CLI integration**:
  - [ ] `st-cli build --target esp32` — cross-compile for target
  - [ ] `st-cli flash --target esp32` — compile and flash to device
  - [ ] `st-cli targets` — list available platform extensions
  - [ ] `st-cli target-info esp32` — show peripherals, pins, capabilities
- [ ] **Initial platform implementations**:
  - [ ] ESP32 (Xtensa, via esp-hal)
  - [ ] STM32F103 (ARM Cortex-M3, via stm32f1xx-hal)
  - [ ] Raspberry Pi (Linux/ARM64, via rppal)
  - [ ] Raspberry Pi Pico / RP2040 (ARM Cortex-M0+, via embassy-rp)
  - [ ] Generic RISC-V (via riscv-hal)
- [ ] **Tests**:
  - [ ] Platform discovery and loading
  - [ ] Peripheral config parsing and validation
  - [ ] Config-to-ST generation (verify correct VAR_GLOBAL output)
  - [ ] Cross-compilation smoke test (compile to ELF, verify target arch)
  - [ ] Platform-specific stdlib compilation
- [ ] **Documentation**:
  - [ ] "Creating a Platform Extension" guide
  - [ ] Per-platform quickstart (ESP32, STM32, RPi, Pico)
  - [ ] Peripheral configuration reference
  - [ ] Hardware I/O mapping tutorial

