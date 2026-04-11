# Program Bundles & IP Protection

A **program bundle** (`.st-bundle`) is a self-contained archive that packages your compiled PLC program for deployment to a target device. Bundles support three modes that control what is included — from full debug information for development to fully stripped binaries for customer delivery.

## Bundle Modes

| Mode | Source Files | Debug Map | Variable Names | Remote Debug | Use Case |
|------|:-----------:|:---------:|:--------------:|:------------:|----------|
| **development** | Yes | Full | Original names | Full | Internal development |
| **release** | No | None | Stripped | Rejected | Customer delivery, production |
| **release-debug** | No | Obfuscated | `v0`, `v1`, ... | Limited | Field diagnostics |

### Development Mode (Default)

```bash
st-cli bundle
```

Includes everything: source files, full debug map with variable names and source locations, unmodified bytecode. This is the mode you use during development — it enables the full VS Code debugging experience on the target.

### Release Mode

```bash
st-cli bundle --release
```

Strips all proprietary information:
- **No source files** in the bundle
- **No debug map** (no `debug.map` file)
- **Variable names replaced** with opaque indices (`g0`, `g1`, `f0_v0`, ...) in the bytecode
- **Source maps cleared** — no line number information
- **Type definition names stripped**

The agent **rejects debug connections** for release bundles. The runtime skips debug hook setup for better performance. This is the mode for shipping to customers or deploying to production.

### Release-Debug Mode

```bash
st-cli bundle --release-debug
```

A middle ground for field support:
- **No source files** in the bundle
- **Obfuscated debug map** — line maps present (for stack traces) but variable names replaced with `v0`, `v1`, ...
- **Source maps kept** in the bytecode (allows line-based breakpoints)
- The agent **allows debug connections** but shows indices instead of variable names

Use this when you need to diagnose issues on deployed systems without exposing your source code.

## Bundle Contents

### Development Bundle

```
my-program.st-bundle (tar.gz)
├── manifest.yaml          # Name, version, mode, checksum, entry point
├── program.stc            # Compiled bytecode (JSON Module)
├── debug.map              # Full debug info (variable names, source maps)
├── plc-project.yaml       # Project configuration
├── _io_map.st             # Auto-generated I/O map
├── source/                # Original ST source files
│   ├── main.st
│   └── helpers.st
└── profiles/              # Device profiles (YAML)
    └── motor_drive.yaml
```

### Release Bundle

```
my-program.st-bundle (tar.gz)
├── manifest.yaml          # Name, version, mode, checksum
├── program.stc            # Stripped bytecode (no var names, no source maps)
├── plc-project.yaml       # Runtime configuration
├── _io_map.st             # I/O map (field names from device profiles only)
└── profiles/              # Device profiles
    └── motor_drive.yaml
```

No `source/` directory. No `debug.map`. The bytecode contains no human-readable identifiers from the original source.

## Creating Bundles

```bash
# Development (default)
st-cli bundle

# Release (stripped, no source)
st-cli bundle --release

# Release with obfuscated debug info
st-cli bundle --release-debug

# Custom output path
st-cli bundle --release -o dist/my-program.st-bundle
```

## Inspecting Bundles

```bash
st-cli bundle inspect my-program.st-bundle
```

Output:
```
Bundle: my-program.st-bundle
  Name:     BottleFillingLine
  Version:  1.0.0
  Mode:     release
  Compiled: 2026-04-10T14:30:00Z
  Compiler: 0.1.1
  Entry:    Main
  Checksum: 166a5025cf03ffbd (valid)
  Size:     2850 bytes

Files:
    257 B  manifest.yaml
    1.1 KB  plc-project.yaml
   86.8 KB  program.stc
```

The checksum is verified on extraction — the agent rejects tampered bundles.

## What Gets Stripped

When you build a release bundle, the following transformations are applied to the compiled bytecode before packaging:

| Element | Development | Release | Release-Debug |
|---------|:-----------:|:-------:|:-------------:|
| POU names (`Main`, `Helper`) | Original | Kept (runtime needs them) | Kept |
| Local variable names | Original | `f0_v0`, `f0_v1`, ... | `f0_v0`, `f0_v1`, ... |
| Global variable names | Original | `g0`, `g1`, ... | `g0`, `g1`, ... |
| Type/struct names | Original | `t0`, `t1`, ... | `t0`, `t1`, ... |
| Struct field names | Original | `f0`, `f1`, ... | `f0`, `f1`, ... |
| Source maps (line info) | Present | Cleared | Present |
| Instructions | Unchanged | Unchanged | Unchanged |

POU names (`Main`, `Helper`, etc.) are always preserved because the runtime needs them to find the entry point program. Everything else that could reveal proprietary logic is stripped or replaced with opaque indices.

## Debug Capabilities by Bundle Mode

| Capability | Development | Release-Debug | Release |
|------------|:-----------:|:-------------:|:-------:|
| Set breakpoints (by line) | Yes | Yes | No |
| Step In / Over / Out | Yes | Yes | No |
| View source in editor | Yes | No | No |
| Variable names in locals | Yes | Indices only | No |
| Stack traces with line numbers | Yes | Yes | No |
| Cycle stats & monitoring | Yes | Yes | Yes |
| Start / stop / restart | Yes | Yes | Yes |
