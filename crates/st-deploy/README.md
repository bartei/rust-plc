# st-deploy

Program bundler and remote deployment for PLC targets.

## Purpose

Packages compiled PLC programs into self-contained `.st-bundle` archives for deployment to remote targets. Handles bundle creation, verification, signing, and the SSH-based installation of the runtime agent on target devices.

## How to Use

### Create a Bundle

```bash
# Development bundle (includes source for debugging)
st-cli bundle .

# Release bundle (no source — protects IP)
st-cli bundle . --release

# Release with obfuscated debug info (stack traces without source)
st-cli bundle . --release-debug
```

### Install Runtime on Target

```bash
st-cli target install plc@192.168.1.50
st-cli target install plc@192.168.1.50 --port 2222 --key ~/.ssh/plc_key
```

### Configure Targets

In `plc-project.yaml`:

```yaml
targets:
  - name: line1-plc
    host: 192.168.1.50
    user: plc
  - name: line2-plc
    host: 192.168.1.51
    user: plc
    agent_port: 5000
```

## Public API

### Bundle Creation

```rust
use st_deploy::bundle::{create_bundle, BundleOptions};

let options = BundleOptions {
    project_path: Path::new("."),
    output: Path::new("out.st-bundle"),
    mode: BundleMode::Development,
    sign_key: None,
};
create_bundle(&options)?;
```

### Bundle Inspection

```rust
use st_deploy::bundle::inspect_bundle;

let info = inspect_bundle(Path::new("MyProject.st-bundle"))?;
println!("Name: {}, Mode: {}", info.manifest.name, info.manifest.mode);
```

### Key Types

- `ProgramBundle` — Bundle abstraction (manifest + bytecode + optional source/debug)
- `BundleManifest` — Metadata: name, version, mode, checksum, compiler version
- `BundleMode` — `Development`, `Release`, `ReleaseDebug`
- `DebugMap` — Source-to-bytecode mapping (line maps, variable names)
- `Target` — Deployment target (host, user, port, auth)
- `InstallOptions` / `InstallResult` — Installation parameters and status

## Bundle Formats

### Development (default)

```
program.st-bundle (tar.gz)
  manifest.yaml
  program.stc                 # Compiled bytecode
  debug.map                   # Full debug info
  source/                     # Original .st files
  plc-project.yaml
  _io_map.st
  profiles/                   # Device profiles
```

### Release (--release)

```
program.st-bundle (tar.gz)
  manifest.yaml
  program.stc                 # Bytecode only — no source
  plc-project.yaml
  _io_map.st
  profiles/
```

### Release-Debug (--release-debug)

```
program.st-bundle (tar.gz)
  manifest.yaml
  program.stc
  debug.map                   # Obfuscated: line maps only, no var names
  plc-project.yaml
  _io_map.st
  profiles/
```

## IP Protection

- **Source stripping** — Release bundles never include `.st` files
- **Debug stripping** — Variable names replaced with opaque indices (`v0`, `v1`)
- **Bundle signing** — Optional Ed25519 signature; agent rejects unsigned bundles when configured
- **Checksum verification** — SHA-256 integrity check on extraction

## Dependencies

| Crate | Purpose |
|-------|---------|
| `st-syntax`, `st-semantics`, `st-compiler`, `st-ir` | Compilation pipeline |
| `st-comm-api` | Device profiles for bundling |
| `sha2` | SHA-256 checksums |
| `tar`, `flate2` | Archive creation/extraction |
| `chrono` | Timestamps |
| `serde`, `serde_json`, `serde_yaml` | Manifest serialization |
