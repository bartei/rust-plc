# Remote Deployment & Online Management — Progress Tracker

> **Design document:** [design_deploy.md](design_deploy.md) — architecture, agent API, transport, security.
> **Parent plan:** [implementation.md](implementation.md) — core platform progress tracker.
> **See also:**
> - [implementation_comm.md](implementation_comm.md) — communication layer (Phase 13)
> - [implementation_native.md](implementation_native.md) — native compilation (Phase 14)

---

## Phase 15a: Program Bundler & Target Configuration

### st-deploy crate

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
- [x] `--release-debug` mode: include obfuscated `debug.map` (line maps only, variable names replaced with indices)
- [x] Default (development) mode: include full source + debug info + full `debug.map`
- [x] `manifest.yaml` includes `mode: development | release | release-debug` + `has_debug_map`
- [ ] `--obfuscate-names` flag: replace POU names with hashes in bytecode + debug map
- [x] Debug info stripping: `strip_module()` removes variable names, source maps, type names from bytecode
- [x] Debug info stripping: `strip_module_keep_source_maps()` for release-debug (keeps line maps)
- [x] `DebugMap` struct: extracted from Module before stripping, serialized as `debug.map` in archive
- [x] Obfuscated debug map: `obfuscate_debug_map()` replaces var names with `v0`/`g0`/`t0`, keeps POU names + source maps
- [ ] Agent respects bundle mode: disables DAP attach for `release` bundles
- [ ] Runtime respects bundle mode: skips debug hook setup for `release` bundles
- [x] Unit tests: debug_info module (7 tests)
- [x] E2E tests: receiver-side verification (19 tests)


### CLI: bundle command

- [x] `st-cli bundle` — compile + create `.st-bundle` (development mode)
- [x] `st-cli bundle --release` — compile + create release bundle (no source, stripped debug)
- [x] `st-cli bundle --release-debug` — release with obfuscated debug info
- [x] `st-cli bundle --output <path>` — custom output path
- [x] `st-cli bundle inspect <path>` — show manifest, mode, file list, sizes, signature status
- [x] `st-cli target list` — show configured targets from `plc-project.yaml`

---

## Phase 15b: Target Agent Core

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
- [ ] Old bundle cleanup (keep last N versions)
- [ ] File integrity check on startup

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
- [ ] `GET /api/v1/logs/stream` — SSE stream of live log events
- [x] Error responses: consistent JSON `{ "error": "...", "code": "..." }` format
- [ ] Request logging middleware (tower-http TraceLayer)

### Authentication

- [x] `Authorization: Bearer <token>` header validation
- [x] `auth.mode: none` — no auth (development only)
- [x] `auth.mode: token` — shared secret from `agent.yaml`
- [x] `auth.read_only: true` — reject mutating endpoints
- [x] Reject with 401/403 and clear error message
- [x] Health endpoint exempt from auth

### Tests

- [x] Unit tests: config (5), error (2), program store (6), runtime manager (4) — 18 total
- [x] Integration tests: HTTP API via reqwest on random port — 18 tests
- [x] QEMU E2E tests: x86_64 (21 tests) + aarch64 (4 tests) — 25 tests (gated by `ST_E2E_QEMU=1`)
- [x] QEMU infrastructure: VM scripts, cloud-init, test fixtures (v1/v2/v3/native-fb)
- [x] Native FB e2e: device profile project compiled, bundled, deployed, verified on both architectures

---

## Phase 15c: SSH Transport & Agent Bootstrap

### SSH transport

- [x] SSH connection management via system `ssh`/`scp` binaries (`st-deploy/src/ssh.rs`)
- [x] Key-based authentication (SSH agent, explicit key file)
- [x] SCP file upload (binary, bundles)
- [x] Connection timeout (30s) and error handling

### Agent bootstrap (superseded by `st-cli target install`)

- [x] OS + arch detection via SSH `uname -s -m`
- [x] Locate static binary for target platform
- [x] Upload binary via SCP
- [x] Generate default `agent.yaml`
- [x] Install systemd service (Linux): generate unit file, enable + start
- [x] Verify agent healthy
- [x] `st-cli target install user@host` (replaces `target bootstrap`)

### CLI: deploy & connect commands

- [ ] `st-cli deploy --target <name>` — compile + bundle + upload + start
- [ ] `st-cli deploy` — uses `default_target` from `plc-project.yaml`
- [ ] `st-cli target connect <name>` — verify agent reachable, show target info
- [ ] Progress output: compile → bundle → upload → start → done

---

## Phase 15d: DAP & Monitor Proxy

### DAP proxy (agent side)

- [x] TCP DAP proxy on configurable port (default: HTTP port + 1 = 4841)
- [x] Spawn `st-cli debug` subprocess for the deployed program
- [x] Bidirectional TCP↔stdio byte bridge (Content-Length framing preserved)
- [x] Session lifecycle: create on connect, destroy on disconnect, kill subprocess
- [x] Bundle mode enforcement: reject release bundles (no source/debug info)
- [x] Bundle mode enforcement: reject when no program deployed
- [x] Source files extracted from development bundles to disk for DAP access
- [x] Single-session enforcement (reject if already connected)

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
- [x] Resolve target connection from `plc-project.yaml`
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
- [ ] Monitor proxy test: telemetry data forwarded correctly
- [ ] Disconnect test: client drops, runtime continues, reconnect works
- [ ] E2E test: VS Code attach to remote agent via Playwright

---

## Phase 15e: Online Update & Hot-Reload

### Online change integration (agent side)

- [x] `POST /api/v1/program/update` — receive new bundle
- [x] Compare old and new bytecode for online-change compatibility
- [x] If compatible: apply online change (zero downtime)
- [x] If incompatible: full restart (stop → replace → start)
- [x] Report update method and downtime in response
- [x] Variable migration during online change
- [ ] Rollback on online-change failure (keep old program running)

### CLI: update command

- [ ] `st-cli target update <name>` — compile + bundle + upload + apply
- [ ] `st-cli target update <name> --dry-run` — preview update method
- [ ] `st-cli target update <name> --force-restart` — skip online change
- [ ] Progress output: compile → bundle → upload → analyzing → applying → done
- [ ] Show update result: method used, downtime, variables migrated

### VS Code extension: update integration

- [x] "Update" command in command palette
- [x] "Update" button in target status bar
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

### Unified static binary (st-runtime)

- [x] `crates/st-runtime/` — unified binary crate with subcommands:
  - [x] `st-runtime agent` — run as daemon (systemd starts this)
  - [x] `st-runtime debug <path>` — DAP debug server (agent spawns this internally)
  - [x] `st-runtime run <path>` — direct execution
  - [x] `st-runtime check <path>` — syntax/semantic check
  - [x] `st-runtime version` — version info
- [x] Delegates to existing crate logic (st-dap, st-engine, st-semantics, etc.)
- [x] DAP proxy spawns self (`std::env::current_exe()`) instead of separate `st-cli`
- [ ] Unit test: all subcommands parse and dispatch correctly

### Static build infrastructure

- [x] `[profile.release-static]` in workspace Cargo.toml
- [x] `scripts/build-static.sh` — build script using `nix-shell -p pkgsCross.musl64.stdenv.cc`
- [x] x86_64 static binary: 4.0 MB, verified `static-pie linked` + `statically linked`
- [x] Runs on Debian 12 QEMU VM (glibc 2.36) — verified live
- [ ] Add musl targets to CI workflow
- [ ] CI job: build static aarch64 binary
- [ ] Test: binary runs on Alpine 3.19 (musl, no glibc)

### One-command installer (st-cli target install)

- [x] `st-cli target install user@host` — full installation:
  - [x] SSH connection (key-based auth via system `ssh` binary)
  - [x] OS + arch detection via `ssh user@host 'uname -s -m'`
  - [x] Select matching static binary (from local build output)
  - [x] Upload binary via SCP to `/opt/st-plc/st-runtime`
  - [x] Create directories: `/opt/st-plc/`, `/etc/st-plc/`, `/var/lib/st-plc/programs/`, `/var/log/st-plc/`
  - [x] Write default `/etc/st-plc/agent.yaml`
  - [x] Generate systemd unit file `/etc/systemd/system/st-runtime.service`
  - [x] `systemctl daemon-reload && systemctl enable --now st-runtime`
  - [x] Wait for agent health check (polls up to 15 times)
  - [x] Report success with connection details + plc-project.yaml snippet
- [x] `--key <path>` — explicit SSH key
- [x] `--port <ssh-port>` — non-standard SSH port
- [x] `--agent-port <port>` — custom agent port
- [x] `--name <name>` — custom agent name
- [x] Progress output: connecting → detecting → uploading → installing → starting → verifying → done

### Upgrade command

- [x] `st-cli target install user@host --upgrade` — in-place upgrade:
  - [x] Backup current binary to `/opt/st-plc/st-runtime.backup`
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

### SSH transport module (st-deploy crate)

- [x] `SshTarget` struct with `parse("user@host")`, `with_port()`, `with_key()`
- [x] `exec(cmd)` — run a command on the target via SSH subprocess
- [x] `sudo_exec(cmd)` — run with sudo
- [x] `upload(local, remote)` — SCP file upload
- [x] `detect_platform()` → `(os, arch)` — uname detection
- [x] `check_sudo()` — verify passwordless sudo
- [x] `test_connection()` — verify SSH works, clear error messages
- [x] 30s connection timeout via `-o ConnectTimeout=30`
- [x] Uses system `ssh`/`scp` binaries
- [x] Unit tests: parse, key/port options, SSH args (5 tests)

### E2E tests: one-command installer against QEMU (26 tests)

- [x] Static x86_64 binary exists (4.0 MB)
- [x] `file` output shows "static-pie linked"
- [x] `ldd` output shows "statically linked"
- [x] Binary size < 25MB
- [x] `st-cli target install plc@vm` on fresh Debian 12 VM succeeds
- [x] After install, `/opt/st-plc/st-runtime` exists and is executable
- [x] After install, `/etc/st-plc/agent.yaml` exists with correct defaults
- [x] After install, `systemctl is-active st-runtime` returns "active"
- [x] After install, `systemctl is-enabled st-runtime` returns "enabled"
- [x] After install, health check returns `{"healthy":true}`
- [x] After install, target-info returns correct OS/arch/version
- [x] After install, can upload a bundle and start the runtime (cycles advancing)
- [x] After install, can attach DAP debugger (Initialize succeeds, Disconnect clean)
- [x] Service auto-restarts after crash (kill -9 → systemd restart → healthy in 5s)
- [x] Custom `--name` appears in health response
- [ ] `st-cli target install plc@vm` on aarch64 VM
- [x] Install, then upgrade with `--upgrade` — succeeds, agent healthy after
- [x] Upgrade preserves agent config (custom name survives restart)
- [x] Upgrade preserves deployed program (agent healthy after upgrade)
- [ ] Failed upgrade restores backup
- [x] Install, then uninstall — service stopped, binary removed, config removed
- [x] Uninstall `--purge` removes programs and log directories
- [x] Uninstall on not-installed target → clear "not installed" error
- [x] Install with wrong SSH key → fails with clear error
- [x] Install with unreachable host → fails with clear error
- [x] SSH with explicit `--key` flag works
- [x] SSH with non-standard `--port 2222` works
- [x] Full lifecycle: install → deploy → run → stop → upgrade → uninstall

---

### Logging (COMPLETED)

- [x] Journald logging via `tracing-journald` (no log files — journald handles rotation + compression)
- [x] Fallback to stderr when journald not available (tests, non-systemd)
- [x] `logging.level` config in `agent.yaml` (trace/debug/info/warn/error, default: info)
- [x] `GET /api/v1/log-level` — query current level
- [x] `PUT /api/v1/log-level` — change level at runtime without restart
- [x] Invalid level rejected with 400
- [x] Unit tests: level validation (2 tests)
- [x] Integration tests: get, set, invalid, no-handle fallback (4 tests)
- [x] QEMU E2E: journald writes, config level, runtime change (3 tests)

### Documentation (COMPLETED)

- [x] "Getting Started: Remote Deployment" quickstart guide
- [x] "Agent Installation" reference (one-command installer)
- [x] "Target Configuration" reference (plc-project.yaml targets section)
- [x] Agent API reference (all endpoints)
- [x] "Remote Debugging" tutorial (VS Code attach workflow)
- [x] "Online Update" tutorial (hot-reload vs full restart)
- [x] "Security Configuration" guide (SSH tunnel, tokens, read-only)
- [x] "CI/CD Pipeline" example (build → bundle → deploy → verify)
- [x] Troubleshooting guide (connection issues, firewall, agent logs)

---

## Phase 15h: End-to-End Testing with QEMU/KVM Target VMs

### Test VM infrastructure

- [x] QEMU/KVM helper scripts in `tests/e2e-deploy/vm/`
- [x] Base VM image: Debian 12 cloud images (amd64 + arm64) via `setup-images.sh`
  - [x] SSH server with key-based auth (cloud-init injects ed25519 key)
  - [x] systemd init
  - [x] Pre-authorized SSH public key for test runner
- [x] VM management scripts:
  - [x] `setup-images.sh` — download images, generate SSH keys, create cloud-init seed ISO
  - [x] `start-vm.sh <arch>` — launch QEMU with CoW overlay + port forwarding
  - [x] `wait-ssh.sh <port>` — poll SSH port until accepting connections (90s timeout)
  - [x] `stop-vm.sh <arch>` — graceful SIGTERM + force kill
- [x] QEMU networking: user-mode with port forwarding (SSH:2222, Agent:4840, DAP:4841)
- [x] Cloud-init `user-data.yaml` + `meta-data`
- [x] ARM64 (aarch64) VM variant using `qemu-system-aarch64`
- [x] Disk image caching: CoW overlay per test run

### Test fixture: ST test application

- [x] `tests/e2e-deploy/fixtures/test-project/` — counter + FB, multi-file, 10ms cycle
- [x] `tests/e2e-deploy/fixtures/test-project-v2/` — online update test (counter increments by 2)
- [x] `tests/e2e-deploy/fixtures/test-project-v3/` — incompatible layout (forces full restart)
- [x] `tests/e2e-deploy/fixtures/test-native-fb/` — native FB with SimpleIO device profile

### E2E: Remote deployment via HTTP API

- [x] Upload bundle via `/api/v1/program/upload` (in-process + QEMU)
- [x] Program info matches expected name, version, checksum
- [x] Start runtime → status shows `running`, cycle count advances
- [x] Stop runtime → status shows `idle`
- [x] Restart cycles the runtime cleanly
- [x] Upload replaces existing program
- [ ] `st-cli deploy --target <name>` one-command deploy
- [ ] Deploy a signed bundle — agent accepts valid signature
- [ ] SSH tunnel mode — agent bound to localhost, CLI tunnels through SSH

### E2E: Remote debugging (DAP proxy)

- [x] Full DAP protocol via TCP proxy: Initialize → Launch → Stopped → StackTrace → Scopes → Variables → Step → Disconnect
- [x] Release bundle → DAP connection rejected (no debug info)
- [x] No program deployed → DAP connection rejected
- [x] Remote debug via direct port forwarding
- [x] Remote debug via SSH tunnel
- [x] Release bundle debug rejected on QEMU target
- [x] Debug update during session — debug v1, disconnect, upload v2, re-attach, verify v2
- [x] VS Code E2E: upload development bundle to agent
- [x] VS Code E2E: attach debugger and stop on entry
- [x] VS Code E2E: set breakpoint and hit it
- [x] VS Code E2E: inspect local variables
- [x] VS Code E2E: step in and step over
- [x] VS Code E2E: continue and pause
- [x] VS Code E2E: evaluate expression
- [x] VS Code E2E: online update — upload v2, re-attach, verify new code
- [x] VS Code E2E: release bundle → debug attach rejected
- [x] VS Code E2E: full lifecycle — upload → debug → update → debug → stop
- [ ] Debug with release-debug bundle — line-based breakpoints work, variable names are indices
- [ ] Force/unforce variable during remote debug
- [ ] Cross-file debugging (step into helper.st)

### E2E: Remote variable watch (monitor proxy)

- [ ] Connect to `/ws/monitor` on remote agent, receive initial catalog
- [ ] `addWatch` for a global variable — telemetry arrives with correct value
- [ ] Watch multiple variables — all values update each cycle
- [ ] Watch FB instance — hierarchical tree expansion works remotely
- [ ] `removeWatch` — variable no longer in telemetry
- [ ] `clearWatch` — all watches removed
- [ ] Cycle stats arrive via monitor — cycle count, timing, jitter
- [ ] Force variable via monitor REPL — value changes on target
- [ ] Two monitor clients simultaneously — both receive telemetry
- [ ] Monitor client disconnects and reconnects — watches re-established
- [ ] Monitor with release bundle — cycle stats available, variable names from I/O map only

### E2E: Remote online code update

- [ ] Deploy v1, start, verify counter increments by 1
- [ ] Update to v2 (compatible) — online change applied, zero downtime
- [ ] After v2 update — counter increments by 2, existing variables preserved
- [ ] Update to v3 (incompatible) — full restart applied, downtime reported
- [ ] After v3 update — runtime running with new variable layout
- [ ] Update with `--dry-run` — reports method but does not apply
- [ ] Update with `--force-restart` — full restart even for compatible changes
- [ ] Update during active debug session — DAP session survives
- [ ] Update during active monitor session — telemetry resumes after brief pause
- [ ] Rollback on failed online change — old program continues running
- [ ] Rapid successive updates (v1 → v2 → v1) — each correctly applied

### E2E: Agent resilience & edge cases

- [ ] Kill agent process on VM — systemd restarts it, program auto-starts
- [ ] Kill runtime process on VM — agent detects crash, auto-restarts runtime
- [ ] Network interruption (drop port forwarding, restore) — agent and runtime unaffected
- [ ] Deploy while runtime is crashing — agent handles concurrent upload + restart
- [ ] Agent with `read_only: true` — deploy rejected, status/monitor still works
- [ ] Agent `max_restarts` exceeded — enters error state, stops retrying
- [ ] VM reboot — agent starts on boot, auto-starts last deployed program
- [ ] Disk full simulation — agent returns clear error on bundle upload
- [ ] Large bundle upload (>50MB) — transfer completes, no timeout

### CI integration

- [ ] QEMU/KVM available in CI runner
- [ ] CI job: download + cache base VM image
- [ ] CI job: build agent binary + test project before launching VM
- [ ] CI job: start VM → run E2E test suite → stop VM
- [ ] CI timeout: 15-minute cap for full E2E suite
- [ ] CI artifact: upload VM serial console log + agent logs on test failure
- [ ] CI gate: E2E tests required to pass before merge
- [ ] Optional: ARM64 E2E tests (QEMU aarch64 emulation, run nightly)