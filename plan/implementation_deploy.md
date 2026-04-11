# Remote Deployment & Online Management — Progress Tracker

> **Design document:** [design_deploy.md](design_deploy.md) — architecture, agent API, transport, security.
> **Parent plan:** [implementation.md](implementation.md) — core platform progress tracker.
> **See also:**
> - [implementation_comm.md](implementation_comm.md) — communication layer (Phase 13)
> - [implementation_native.md](implementation_native.md) — native compilation (Phase 14)

## Status Summary

| Phase | Status | What |
|-------|--------|------|
| **15a** | **Done** | Program bundler, target config, bundle modes, IP protection, debug stripping |
| **15b** | **Done** | Target agent (HTTP API, runtime manager, watchdog, auth) — 55 tests |
| **15c** | **Mostly done** | SSH transport + install — superseded by Phase 15g |
| **15d** | **Done** | DAP proxy (TCP bridge), VS Code remote attach — 17 tests |
| **15e** | Pending | Online update & hot-reload via agent |
| **15f** | Pending | Network discovery, target management CLI |
| **15g** | **Done** | Static binary (4MB musl), one-command installer, systemd — 26 QEMU E2E tests |
| **15g+** | Pending | TLS, log rotation, additional hardening |
| **15h** | **Mostly done** | QEMU E2E infrastructure, test fixtures, DAP/debug/installer tests |
| **Docs** | **Done** | 8 deployment doc pages (quickstart, bundles, targets, updating, security, CI/CD, commands, troubleshooting) |

**Crate inventory:**

| Crate | Type | Purpose |
|-------|------|---------|
| `st-deploy` | Library | Bundle creation, SSH transport, installer, target config |
| `st-target-agent` | Library + Binary | Agent HTTP API, runtime manager, DAP proxy, watchdog |
| `st-plc-runtime` | Binary | Unified static binary for target (agent + debug + run + check) |

**Test counts:**

| Suite | Tests |
|-------|-------|
| st-deploy unit + E2E | 73 (27 unit + 22 bundle E2E + 19 mode E2E + 5 SSH) |
| st-target-agent unit + integration | 59 (18 unit + 18 API + 3 DAP + 20 QEMU) |
| st-plc-runtime QEMU installer | 26 (all live against Debian 12 QEMU/KVM) |
| VS Code remote debug | 10 (gated by ST_E2E_REMOTE=1) |
| **Total deployment tests** | **168** |

---

## Phase 15a: Program Bundler & Target Configuration

Foundation work — create the bundle format, parse target configs, and build the
developer-side deployment crate.

### st-deploy crate (developer side)

- [x] Crate scaffolding (`crates/st-deploy/`)
- [x] `Target` struct + parser for `targets:` section in `plc-project.yaml`
- [x] `default_target` resolution logic
- [x] `ProgramBundle` struct — manifest, bytecode, source files, project config, profiles
- [x] Bundle creation: compile project → collect artifacts → create `.st-bundle` (tar.gz)
- [x] Bundle verification: SHA-256 checksum, manifest validation
- [x] Bundle extraction + inspection (`st-cli bundle inspect`)
- [x] Unit tests: bundle create/extract round-trip, target config parsing (20 unit + 22 E2E)

### Bundle modes (IP protection)

- [x] `--release` mode: exclude `source/` directory and `debug.map` from bundle
- [x] `--release-debug` mode: include obfuscated `debug.map` (line maps only, variable names replaced with indices `v0`, `v1`, ...)
- [x] Default (development) mode: include full source + debug info + full `debug.map`
- [x] `manifest.yaml` includes `mode: development | release | release-debug` + `has_debug_map`
- [ ] `--obfuscate-names` flag: replace POU names with hashes in bytecode + debug map
- [x] Debug info stripping: `strip_module()` removes variable names, source maps, type names from bytecode
- [x] Debug info stripping: `strip_module_keep_source_maps()` for release-debug (keeps line maps)
- [x] `DebugMap` struct: extracted from Module before stripping, serialized as `debug.map` in archive
- [x] Obfuscated debug map: `obfuscate_debug_map()` replaces var names with `v0`/`g0`/`t0`, keeps POU names + source maps
- [ ] Agent respects bundle mode: disables DAP attach for `release` bundles — *deferred to Phase 15b/15d*
- [ ] Runtime respects bundle mode: skips debug hook setup for `release` bundles — *deferred to Phase 15b*
- [x] Unit tests: debug_info module (7 tests: extract, obfuscate, strip, JSON round-trip, no original names in stripped JSON)
- [x] E2E tests: receiver-side verification (19 tests: all 3 modes, bytecode inspection, archive contents, IP protection)

### Bundle signing

- [ ] Ed25519 key pair generation (`st-cli bundle keygen --output <name>`)
- [ ] Bundle signing (`--sign-key <path>`) — sign manifest checksum with private key
- [ ] Signature stored in `manifest.yaml` (`signature:` field)
- [ ] Agent signature verification: reject unsigned bundles when `require_signed: true`
- [ ] Agent trusted key store: list of public keys in `agent.yaml` (`security.trusted_keys`)
- [ ] Unit tests: sign/verify round-trip, reject tampered bundle, reject missing signature

### Bundle encryption (stretch)

- [ ] AES-256-GCM encryption of bundle contents (excluding manifest header)
- [ ] `--encrypt-for <target>` uses target's public key for key wrapping
- [ ] Agent decryption with stored key (`security.bundle_key` in `agent.yaml`)
- [ ] Unit tests: encrypt/decrypt round-trip

### CLI: bundle command

- [x] `st-cli bundle` — compile + create `.st-bundle` (development mode, includes source)
- [x] `st-cli bundle --release` — compile + create release bundle (no source, stripped debug)
- [x] `st-cli bundle --release-debug` — release with obfuscated debug info (line maps only)
- [x] `st-cli bundle --output <path>` — custom output path
- [ ] `st-cli bundle --sign-key <path>` — sign the bundle
- [ ] `st-cli bundle --obfuscate-names` — replace POU names with hashes
- [x] `st-cli bundle inspect <path>` — show manifest, mode, file list, sizes, signature status
- [ ] `st-cli bundle verify <path> --key <pubkey>` — verify bundle signature
- [ ] `st-cli bundle keygen --output <name>` — generate Ed25519 signing key pair
- [x] `st-cli target list` — show configured targets from `plc-project.yaml`

---

## Phase 15b: Target Agent Core

Build the standalone `st-target-agent` binary with program storage, runtime management,
and the REST API.

### Agent binary scaffolding

- [x] Crate scaffolding (`crates/st-target-agent/`)
- [x] `main.rs` entry point with CLI args (clap)
- [x] `agent.yaml` config file parser (serde_yaml) — 5 unit tests
- [x] Structured logging to stdout (tracing-subscriber)
- [x] Graceful shutdown handler (SIGTERM / Ctrl+C via tokio::signal)

### Program store

- [x] Bundle extraction to `program_dir` via `st_deploy::bundle::extract_bundle()`
- [x] Current program tracking (in-memory, metadata query)
- [x] Program metadata query (name, version, compiled_at, checksum)
- [ ] Old bundle cleanup (keep last N versions) — *deferred, in-memory store for now*
- [ ] File integrity check on startup — *deferred*

### Runtime manager

- [x] Start PLC runtime from stored bundle (VM mode)
- [x] Stop runtime (graceful shutdown with scan cycle completion)
- [x] Restart runtime (stop → start)
- [x] Runtime state machine: `Idle → Starting → Running → Stopping → Idle`
- [x] Runtime state machine: `Running → Error` (on crash)
- [x] Expose `CycleStats` from the running VM (CycleStatsSnapshot)
- [x] Thread management: runtime runs in dedicated `std::thread`, manager coordinates via `tokio::sync::mpsc`
- [x] `run_one_cycle()` loop with command check between cycles

### Watchdog

- [x] Cycle count monitoring for hang detection
- [x] Crash detection via state polling
- [x] Auto-restart with configurable delay and max retry count
- [x] Reset restart counter on successful sustained run (60s)

### HTTP REST API (axum)

- [x] Server scaffolding (axum, bind address + port from config)
- [x] `POST /api/v1/program/upload` — receive bundle (multipart/form-data)
- [x] `GET /api/v1/program/info` — current program metadata
- [x] `POST /api/v1/program/start` — start runtime
- [x] `POST /api/v1/program/stop` — stop runtime
- [x] `POST /api/v1/program/restart` — stop + start
- [x] `DELETE /api/v1/program` — remove deployed program
- [x] `GET /api/v1/status` — runtime state + cycle stats
- [x] `GET /api/v1/health` — agent health check (200 OK / 503)
- [x] `GET /api/v1/target-info` — OS, arch, agent version, uptime
- [x] `GET /api/v1/logs` — query logs (placeholder, returns agent status)
- [ ] `GET /api/v1/logs/stream` — SSE stream of live log events — *deferred*
- [x] Error responses: consistent JSON `{ "error": "...", "code": "..." }` format
- [ ] Request logging middleware (tower-http TraceLayer) — *deferred*

### Authentication

- [x] `Authorization: Bearer <token>` header validation
- [x] `auth.mode: none` — no auth (development only)
- [x] `auth.mode: token` — shared secret from `agent.yaml`
- [x] `auth.read_only: true` — reject mutating endpoints
- [x] Reject with 401/403 and clear error message
- [x] Health endpoint exempt from auth

### Tests

- [x] Unit tests: config (5), error (2), program store (6), runtime manager (4) — **18 total**
- [x] Integration tests: HTTP API via reqwest on random port — **18 tests** covering all endpoints, auth, read-only, full lifecycle
- [x] QEMU E2E tests: x86_64 (13 tests) + aarch64 (3 tests) — **16 tests** gated by `ST_E2E_QEMU=1`
- [x] QEMU infrastructure: VM scripts (setup-images, start-vm, wait-ssh, stop-vm), cloud-init, test fixtures (v1/v2/v3)

---

## Phase 15c: SSH Transport & Agent Bootstrap

> **Note:** Most of Phase 15c was implemented as part of Phase 15g (one-command
> installer) which supersedes the original bootstrap approach. The SSH transport
> module, SCP upload, systemd installation, and health verification are all
> implemented in `st-deploy/src/ssh.rs` and `st-deploy/src/installer.rs`.

### SSH transport — implemented in Phase 15g

- [x] SSH connection management via system `ssh`/`scp` binaries (`st-deploy/src/ssh.rs`)
- [x] Key-based authentication (SSH agent, explicit key file)
- [x] SCP file upload (binary, bundles)
- [x] Connection timeout (30s) and error handling
- ~~Password authentication~~ — **removed**: insecure, SSH keys are mandatory
- ~~SSH tunnel management in CLI~~ — **removed**: users manage tunnels themselves via `ssh -L` (documented in security guide). Respects the user's SSH config, jump hosts, and ProxyCommand.

### Agent bootstrap — superseded by `st-cli target install`

- [x] OS + arch detection via SSH `uname -s -m`
- [x] Locate static binary for target platform
- [x] Upload binary via SCP
- [x] Generate default `agent.yaml`
- [x] Install systemd service (Linux): generate unit file, enable + start
- [x] Verify agent healthy
- [x] `st-cli target install user@host` (replaces `target bootstrap`)
- [ ] Windows service support — *deferred*

### CLI: deploy & connect commands

- [ ] `st-cli deploy --target <name>` — compile + bundle + upload + start
- [ ] `st-cli deploy` — uses `default_target` from `plc-project.yaml`
- [ ] `st-cli target connect <name>` — verify agent reachable, show target info
- [ ] Progress output: compile → bundle → upload → start → done

---

## Phase 15d: DAP & Monitor Proxy

Enable remote debugging and monitoring through the agent.

### DAP proxy (agent side)

- [x] TCP DAP proxy on configurable port (default: HTTP port + 1 = 4841)
- [x] Spawn `st-cli debug` subprocess for the deployed program
- [x] Bidirectional TCP↔stdio byte bridge (Content-Length framing preserved)
- [x] Session lifecycle: create on connect, destroy on disconnect, kill subprocess
- [x] Bundle mode enforcement: reject release bundles (no source/debug info)
- [x] Bundle mode enforcement: reject when no program deployed
- [x] Source files extracted from development bundles to disk for DAP access
- [ ] Single-session enforcement (reject if already connected) — *deferred*

### Monitor proxy (agent side)

- [ ] `/ws/monitor` WebSocket endpoint
- [ ] Connect to runtime's local monitor WebSocket
- [ ] Bidirectional frame forwarding
- [ ] Multi-client support (fan-out from single runtime connection)
- [ ] Handle client connect/disconnect without affecting runtime
- [ ] Reconnect to runtime monitor if connection drops

### VS Code extension: remote debug

- [x] Detect `"request": "attach"` with `host`/`port` in launch.json
- [x] `StDebugAdapterFactory` returns `DebugAdapterServer(port, host)` for attach mode
- [x] `package.json` updated with `attach` configuration attributes and snippet
- [x] Resolve target connection from `plc-project.yaml` — `"target": "line1-plc"` in launch.json resolves host + DAP port from `targets:` section
- [ ] `sourceFileMap` auto-configuration for path remapping

### VS Code extension: remote monitor

- [ ] Detect active remote debug session
- [ ] Connect monitor panel to agent's `/ws/monitor` instead of local WS
- [ ] All existing monitor features work transparently
- [ ] Reconnect on network interruption

### CLI: remote debug support

- [ ] `st-cli target start <name> --debug` — start runtime in debug-ready mode
- [ ] `st-cli target attach <name>` — launch terminal-based DAP client (stretch)

### Tests

- [x] DAP proxy integration test: full protocol (Initialize→Launch→Stopped→StackTrace→Scopes→Variables→Step→Disconnect)
- [x] DAP proxy: release bundle rejection (no debug info)
- [x] DAP proxy: no program deployed rejection
- [ ] Monitor proxy test: telemetry data forwarded correctly — *deferred (monitor proxy not yet implemented)*
- [ ] Disconnect test: client drops, runtime continues, reconnect works
- [ ] E2E test: VS Code attach to remote agent via Playwright — *planned, see Phase 15h*

---

## Phase 15e: Online Update & Hot-Reload

Upload new program versions to a running target with minimal or zero downtime.

### Online change integration (agent side)

- [ ] `POST /api/v1/program/update` — receive new bundle
- [ ] Compare old and new bytecode for online-change compatibility
- [ ] If compatible: apply online change (Phase 9 hot-reload, zero downtime)
- [ ] If incompatible: full restart (stop → replace → start)
- [ ] Report update method and downtime in response
- [ ] Variable migration during online change (Phase 9)
- [ ] Rollback on online-change failure (keep old program running)

### CLI: update command

- [ ] `st-cli target update <name>` — compile + bundle + upload + apply
- [ ] `st-cli target update <name> --dry-run` — preview update method
- [ ] `st-cli target update <name> --force-restart` — skip online change
- [ ] Progress output: compile → bundle → upload → analyzing → applying → done
- [ ] Show update result: method used, downtime, variables migrated

### VS Code extension: update integration

- [ ] "Update" command in command palette
- [ ] "Update" button in target status bar
- [ ] Notification with update result (online change vs restart, downtime)
- [ ] Debug session survives online change (DAP session stays connected)
- [ ] Monitor panel continues after update (watch list preserved)

### Tests

- [ ] Online change via agent: compatible update, variables migrated
- [ ] Full restart via agent: incompatible update, clean restart
- [ ] Rollback test: online change fails, old program continues
- [ ] Update during debug: DAP session survives
- [ ] Update during monitor: telemetry continues after brief pause
- [ ] Dry-run test: reports correct method without applying

---

## Phase 15f: Network Discovery & Target Management

Quality-of-life features for managing multiple targets.

### Agent discovery

- [ ] UDP broadcast responder in agent (`ST-AGENT-DISCOVER` → reply with name + port)
- [ ] `st-cli target scan` — discover agents on local network
- [ ] `st-cli target scan --subnet 192.168.1.0/24` — targeted subnet scan
- [ ] Discovery timeout (default 3s)
- [ ] JSON output mode (`--json`) for scripting

### Target status & management

- [ ] `st-cli target status <name>` — show runtime state, cycle stats, uptime
- [ ] `st-cli target status <name> --wait-running --timeout 30s` — wait for running state
- [ ] `st-cli target start <name>` — start the program on the target
- [ ] `st-cli target stop <name>` — stop the program on the target
- [ ] `st-cli target restart <name>` — restart the program
- [ ] `st-cli target info <name>` — show system info (OS, arch, CPU, RAM, disk)
- [ ] `st-cli target logs <name>` — query logs
- [ ] `st-cli target logs <name> --follow` — tail logs in real-time (SSE)

### VS Code extension: target picker

- [ ] Target selector in status bar (shows current target + state)
- [ ] Quick-pick dropdown with all configured targets
- [ ] Connection status indicators (running/stopped/offline)
- [ ] "Add new target" option in quick-pick
- [ ] Target state auto-refresh (poll agent health every 30s)

### Tests

- [ ] Discovery test: agent responds to broadcast, CLI finds it
- [ ] Target status test: correct state reported for running/stopped programs
- [ ] Multi-target test: manage two agents simultaneously

---

## Phase 15g: Static Binary & One-Command Target Installer

> Design: [design_deploy.md § Static Binary Strategy](design_deploy.md#static-binary-strategy--zero-dependencies-on-target)
> Design: [design_deploy.md § One-Command Target Installer](design_deploy.md#one-command-target-installer)

The target preparation must be a single command: `st-cli target install user@host`.
No manual SCP, no library dependencies, no config editing, no systemd wrangling.

### Unified static binary (`st-plc-runtime`)

Merge `st-target-agent` + `st-cli` into a single binary with subcommands. Built
as a fully static ELF (musl libc) with zero runtime dependencies on the target.

- [x] Create `crates/st-plc-runtime/` — unified binary crate with subcommands:
  - [x] `st-plc-runtime agent` — run as daemon (systemd starts this)
  - [x] `st-plc-runtime debug <path>` — DAP debug server (agent spawns this internally)
  - [x] `st-plc-runtime run <path>` — direct execution
  - [x] `st-plc-runtime check <path>` — syntax/semantic check
  - [x] `st-plc-runtime version` — version info
- [x] Delegates to existing crate logic (st-dap, st-runtime, st-semantics, etc.)
- [x] DAP proxy spawns self (`std::env::current_exe()`) instead of separate `st-cli`
- [ ] Unit test: all subcommands parse and dispatch correctly — *TODO*

### Static build infrastructure

- [x] `[profile.release-static]` in workspace Cargo.toml: `opt-level="s"`, `lto=true`, `strip=true`, `panic="abort"`, `codegen-units=1`
- [x] `scripts/build-static.sh` — build script using `nix-shell -p pkgsCross.musl64.stdenv.cc`
- [x] x86_64 static binary: **4.0 MB**, verified `static-pie linked` + `statically linked`
- [x] Runs on Debian 12 QEMU VM (glibc 2.36) — verified live
- [ ] Add musl targets to CI workflow
- [ ] CI job: build static aarch64 binary
- [ ] Test: binary runs on Alpine 3.19 (musl, no glibc)

### One-command installer (`st-cli target install`)

The developer runs `st-cli target install user@host` from their workstation.
Everything else is automated over SSH.

- [x] `st-cli target install user@host` — full installation:
  - [x] SSH connection (key-based auth via system `ssh` binary)
  - [x] OS + arch detection via `ssh user@host 'uname -s -m'`
  - [x] Select matching static binary (from local build output)
  - [x] Upload binary via SCP to `/opt/st-plc/st-plc-runtime`
  - [x] Create directories: `/opt/st-plc/`, `/etc/st-plc/`, `/var/lib/st-plc/programs/`, `/var/log/st-plc/`
  - [x] Write default `/etc/st-plc/agent.yaml`
  - [x] Generate systemd unit file `/etc/systemd/system/st-plc-runtime.service`
  - [x] `systemctl daemon-reload && systemctl enable --now st-plc-runtime`
  - [x] Wait for agent health check (polls `curl localhost:4840/api/v1/health` up to 15 times)
  - [x] Report success with connection details + plc-project.yaml snippet
- [x] `st-cli target install user@host --key <path>` — explicit SSH key
- [x] `st-cli target install user@host --port <ssh-port>` — non-standard SSH port
- [x] `st-cli target install user@host --agent-port <port>` — custom agent port
- [x] `st-cli target install user@host --name <name>` — custom agent name
- [x] Progress output: connecting → detecting → uploading → installing → starting → verifying → done
- [x] **VERIFIED LIVE**: fresh Debian 12 QEMU VM → one command → agent healthy in seconds

### Upgrade command

- [x] `st-cli target install user@host --upgrade` — in-place upgrade:
  - [x] Backup current binary to `/opt/st-plc/st-plc-runtime.backup`
  - [x] Stop the service
  - [x] Upload new binary
  - [x] Start the service
  - [x] Verify new version is running
  - [x] On failure: restore backup, restart, report error
- [x] Preserves existing config and deployed programs

### Uninstall command

- [x] `st-cli target uninstall user@host` — clean removal:
  - [x] Stop and disable the service
  - [x] Remove binary, config, service unit
  - [x] Optionally remove data/logs (`--purge` flag)

### SSH transport module (`st-deploy` crate)

- [x] `SshTarget` struct with `parse("user@host")`, `with_port()`, `with_key()`
- [x] `exec(cmd)` — run a command on the target via SSH subprocess
- [x] `sudo_exec(cmd)` — run with sudo
- [x] `upload(local, remote)` — SCP file upload
- [x] `detect_platform()` → `(os, arch)` — uname detection
- [x] `check_sudo()` — verify passwordless sudo
- [x] `test_connection()` — verify SSH works, clear error messages
- [x] 30s connection timeout via `-o ConnectTimeout=30`
- [x] Uses system `ssh`/`scp` binaries — user's SSH config, agent, keys just work
- [x] Unit tests: parse, key/port options, SSH args (5 tests)

### E2E tests: one-command installer against QEMU

26 tests in `crates/st-plc-runtime/tests/e2e_installer.rs`, gated by `ST_E2E_QEMU=1`.
All verified live against Debian 12 QEMU/KVM VMs. Full suite: ~12 minutes.

**Prerequisite tests (verify static binary works):**

- [x] Test: static x86_64 binary exists (4.0 MB)
- [x] Test: `file` output shows "static-pie linked"
- [x] Test: `ldd` output shows "statically linked"
- [x] Test: binary size < 25MB

**Fresh install tests (x86_64) — all passing:**

- [x] Test: `st-cli target install plc@vm` on fresh Debian 12 VM succeeds
- [x] Test: after install, `/opt/st-plc/st-plc-runtime` exists and is executable
- [x] Test: after install, `/etc/st-plc/agent.yaml` exists with correct defaults
- [x] Test: after install, `systemctl is-active st-plc-runtime` returns "active"
- [x] Test: after install, `systemctl is-enabled st-plc-runtime` returns "enabled"
- [x] Test: after install, health check returns `{"healthy":true}`
- [x] Test: after install, target-info returns correct OS/arch/version
- [x] Test: after install, can upload a bundle and start the runtime (cycles advancing)
- [x] Test: after install, can attach DAP debugger (Initialize succeeds, Disconnect clean)
- [x] Test: service auto-restarts after crash (kill -9 → systemd restart → healthy in 5s)
- [x] Test: custom `--name` appears in health response

**Fresh install tests (aarch64):**

- [ ] Test: `st-cli target install plc@vm` on aarch64 VM — *deferred, needs aarch64 static binary + image*

**Upgrade tests — all passing:**

- [x] Test: install, then upgrade with `--upgrade` — succeeds, agent healthy after
- [x] Test: upgrade preserves agent config (custom name survives restart)
- [x] Test: upgrade preserves deployed program (agent healthy after upgrade)
- [ ] Test: failed upgrade restores backup — *deferred, needs intentionally broken binary*

**Uninstall tests — all passing:**

- [x] Test: install, then uninstall — service stopped, binary removed, config removed
- [x] Test: uninstall `--purge` removes programs and log directories
- [x] Test: uninstall on not-installed target → clear "not installed" error

**Error handling tests — all passing:**

- [x] Test: install with wrong SSH key → fails with clear error
- [x] Test: install with unreachable host (192.0.2.1) → fails with clear error

**SSH transport tests — all passing:**

- [x] Test: SSH with explicit `--key` flag works
- [x] Test: SSH with non-standard `--port 2222` works

**Full lifecycle test — passing:**

- [x] Test: install → deploy → run → stop → upgrade → uninstall (all in sequence)

---

## Phase 15g+: Additional Production Hardening (Future)

### TLS support

- [ ] TLS configuration in `agent.yaml` (cert + key paths)
- [ ] HTTPS for REST API
- [ ] WSS for WebSocket endpoints
- [ ] Self-signed certificate generation helper (`st-cli target tls-init`)

### Logging

- [x] Journald logging via `tracing-journald` (no log files — journald handles rotation + compression)
- [x] Fallback to stderr when journald not available (tests, non-systemd)
- [x] `logging.level` config in `agent.yaml` (trace/debug/info/warn/error, default: info)
- [x] `GET /api/v1/log-level` — query current level
- [x] `PUT /api/v1/log-level` — change level at runtime without restart
- [x] Invalid level rejected with 400
- [x] Unit tests: level validation (2 tests)
- [x] Integration tests: get, set, invalid, no-handle fallback (4 tests)
- [x] QEMU E2E: journald writes, config level, runtime change (3 tests)

### Documentation

- [x] "Getting Started: Remote Deployment" quickstart guide — `docs/src/deployment/quickstart.md`
- [x] "Agent Installation" reference (one-command installer) — `docs/src/deployment/targets.md`
- [x] "Target Configuration" reference (plc-project.yaml targets section) — `docs/src/deployment/targets.md` + `docs/src/cli/project-configuration.md`
- [x] Agent API reference (all endpoints) — `docs/src/deployment/targets.md` § Agent HTTP API
- [x] "Remote Debugging" tutorial (VS Code attach workflow) — `docs/src/deployment/quickstart.md` § Debug Remotely
- [x] "Online Update" tutorial (hot-reload vs full restart) — `docs/src/deployment/updating.md`
- [x] "Security Configuration" guide (SSH tunnel, tokens, read-only) — `docs/src/deployment/security.md`
- [x] "CI/CD Pipeline" example (build → bundle → deploy → verify) — `docs/src/deployment/ci-cd.md`
- [x] Troubleshooting guide (connection issues, firewall, agent logs) — `docs/src/deployment/troubleshooting.md`

---

## Phase 15h: End-to-End Testing with QEMU/KVM Target VMs

Validate the entire remote deployment pipeline against real virtual machines running
Linux, accessed via real SSH, with real systemd services. No mocking — the tests
exercise the exact same code paths as a production deployment to a physical device.

### Test VM infrastructure

- [x] QEMU/KVM helper scripts in `tests/e2e-deploy/vm/`
- [x] Base VM image: Debian 12 cloud images (amd64 + arm64) via `setup-images.sh`
  - [x] SSH server with key-based auth (cloud-init injects ed25519 key)
  - [x] systemd init (Debian cloud images use systemd)
  - [x] Pre-authorized SSH public key for test runner
- [x] VM management scripts:
  - [x] `setup-images.sh` — download images, generate SSH keys, create cloud-init seed ISO
  - [x] `start-vm.sh <arch>` — launch QEMU with CoW overlay + port forwarding (SSH + agent + DAP)
  - [x] `wait-ssh.sh <port>` — poll SSH port until accepting connections (90s timeout)
  - [x] `stop-vm.sh <arch>` — graceful SIGTERM + force kill
- [x] QEMU networking: user-mode with port forwarding (SSH:2222, Agent:4840, DAP:4841)
- [x] Cloud-init `user-data.yaml` + `meta-data`
- [x] ARM64 (aarch64) VM variant using `qemu-system-aarch64`
- [x] Disk image caching: CoW overlay per test run

### Test fixture: ST test application

- [x] `tests/e2e-deploy/fixtures/test-project/` — counter + FB, multi-file, 10ms cycle
  - [x] `plc-project.yaml` with name, version, entryPoint, cycle_time
  - [x] `main.st` — counter + cycle_active variables
  - [x] `helper.st` — Accumulator function block
- [x] `tests/e2e-deploy/fixtures/test-project-v2/` — online update test
  - [x] Same variable layout as v1 (compatible for online change)
  - [x] Counter increments by 2 instead of 1
- [x] `tests/e2e-deploy/fixtures/test-project-v3/` — incompatible layout
  - [x] Added new_sensor_value + alarm_flag (forces full restart)

### E2E: Agent install via SSH — superseded by Phase 15g

> These tests are now covered by Phase 15g's 26 QEMU installer tests
> (`crates/st-plc-runtime/tests/e2e_installer.rs`).

- [x] Test: `st-cli target install` installs static binary on fresh VM via SCP
- [x] Test: agent binary exists and is executable (`/opt/st-plc/st-plc-runtime`)
- [x] Test: systemd unit file created and enabled
- [x] Test: agent service starts and is reachable (health check 200)
- [x] Test: target-info returns correct OS/arch from the VM
- [x] Test: upgrade replaces binary, restarts service, preserves config
- [x] Test: after install, can upload bundle + start runtime + verify cycles

### E2E: Remote deployment via HTTP API

- [x] Test: upload bundle via `/api/v1/program/upload` (in-process + QEMU)
- [x] Test: program info matches expected name, version, checksum
- [x] Test: start runtime → status shows `running`, cycle count advances
- [x] Test: stop runtime → status shows `idle`
- [x] Test: restart cycles the runtime cleanly
- [x] Test: upload replaces existing program
- [ ] Test: `st-cli deploy --target <name>` one-command deploy — *deferred, needs CLI deploy command*
- [ ] Test: deploy a signed bundle — agent accepts valid signature — *deferred, signing not implemented*
- [ ] Test: SSH tunnel mode — agent bound to localhost, CLI tunnels through SSH — *deferred*

### E2E: Remote debugging (DAP proxy)

**In-process integration tests (3 tests, no QEMU):**
- [x] Test: full DAP protocol via TCP proxy: Initialize → Launch → Stopped → StackTrace → Scopes → Variables → Step → Disconnect
- [x] Test: release bundle → DAP connection rejected (no debug info)
- [x] Test: no program deployed → DAP connection rejected

**QEMU E2E tests (4 tests, gated by ST_E2E_QEMU=1):**
- [x] Test: remote debug via direct port forwarding — Initialize, Launch, Stopped(entry), StackTrace(Main), Scopes, Variables(counter), Step, Disconnect
- [x] Test: remote debug via SSH tunnel — same protocol through `ssh -L`, verifying full transport path
- [x] Test: release bundle debug rejected on QEMU target
- [x] Test: debug update during session — debug v1, disconnect, upload v2, re-attach, verify v2

**VS Code extension E2E tests (10 tests, gated by ST_E2E_REMOTE=1):**
- [x] Test: upload development bundle to agent
- [x] Test: attach debugger and stop on entry (via DebugAdapterServer)
- [x] Test: set breakpoint and hit it
- [x] Test: inspect local variables (counter, flag, result)
- [x] Test: step in and step over
- [x] Test: continue and pause (counter advances during run)
- [x] Test: evaluate expression
- [x] Test: online update — upload v2, re-attach, verify new code
- [x] Test: release bundle → debug attach rejected
- [x] Test: full lifecycle — upload → debug → update → debug → stop

**Not yet covered:**
- [ ] Test: debug with release-debug bundle — line-based breakpoints work, variable names are indices
- [ ] Test: force/unforce variable during remote debug
- [ ] Test: cross-file debugging (step into helper.st)

### E2E: Remote variable watch (monitor proxy)

- [ ] Test: connect to `/ws/monitor` on remote agent, receive initial catalog
- [ ] Test: `addWatch` for a global variable — telemetry arrives with correct value
- [ ] Test: watch multiple variables — all values update each cycle
- [ ] Test: watch FB instance — hierarchical tree expansion works remotely
- [ ] Test: `removeWatch` — variable no longer in telemetry
- [ ] Test: `clearWatch` — all watches removed
- [ ] Test: cycle stats arrive via monitor — cycle count, timing, jitter
- [ ] Test: force variable via monitor REPL — value changes on target
- [ ] Test: two monitor clients simultaneously — both receive telemetry
- [ ] Test: monitor client disconnects and reconnects — watches re-established
- [ ] Test: monitor with release bundle — cycle stats available, variable names from I/O map only

### E2E: Remote online code update

- [ ] Test: deploy v1, start, verify counter increments by 1
- [ ] Test: update to v2 (compatible) — online change applied, zero downtime reported
- [ ] Test: after v2 update — counter now increments by 2, existing variables preserved
- [ ] Test: update to v3 (incompatible) — full restart applied, downtime reported
- [ ] Test: after v3 update — runtime running with new variable layout
- [ ] Test: update with `--dry-run` — reports method but does not apply
- [ ] Test: update with `--force-restart` — full restart even for compatible changes
- [ ] Test: update during active debug session — DAP session survives online change
- [ ] Test: update during active monitor session — telemetry resumes after brief pause
- [ ] Test: rollback on failed online change — old program continues running
- [ ] Test: rapid successive updates (v1 → v2 → v1) — each correctly applied

### E2E: Agent resilience & edge cases

- [ ] Test: kill agent process on VM — systemd restarts it, program auto-starts
- [ ] Test: kill runtime process on VM — agent detects crash, auto-restarts runtime
- [ ] Test: network interruption (drop port forwarding, restore) — agent and runtime unaffected
- [ ] Test: deploy while runtime is crashing — agent handles concurrent upload + restart
- [ ] Test: agent with `read_only: true` — deploy rejected, status/monitor still works
- [ ] Test: agent `max_restarts` exceeded — enters error state, stops retrying
- [ ] Test: VM reboot — agent starts on boot, auto-starts last deployed program
- [ ] Test: disk full simulation — agent returns clear error on bundle upload
- [ ] Test: large bundle upload (>50MB) — transfer completes, no timeout

### CI integration

- [ ] QEMU/KVM available in CI runner (GitHub Actions `ubuntu-latest` supports KVM via `/dev/kvm`)
- [ ] CI job: download + cache base VM image (avoid re-downloading each run)
- [ ] CI job: build agent binary + test project before launching VM
- [ ] CI job: start VM → run E2E test suite → stop VM
- [ ] CI timeout: 15-minute cap for full E2E suite
- [ ] CI artifact: upload VM serial console log + agent logs on test failure
- [ ] CI gate: E2E tests required to pass before merge (can be separate workflow from unit tests)
- [ ] Optional: ARM64 E2E tests (QEMU aarch64 emulation, slower, run nightly not per-PR)
