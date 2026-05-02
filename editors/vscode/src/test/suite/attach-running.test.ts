import * as assert from "assert";
import * as vscode from "vscode";
import * as path from "path";
import * as fs from "fs";
import * as os from "os";
import * as http from "http";
import * as child_process from "child_process";

/**
 * Acceptance test for non-intrusive debug attach.
 *
 * The contract this test pins down (TDD-style — written before the
 * implementation is reviewed/fixed):
 *
 *   1. Given a `st-target-agent` with a deployed program that's
 *      *currently running* (cycle counter advancing),
 *   2. when a VS Code debug client attaches via DAP `request: "attach"`
 *      with `stopOnEntry: false`,
 *   3. then the engine **must not pause** — `cycle_count` MUST keep
 *      growing while the debug session is connected.
 *   4. Setting a breakpoint mid-program MUST eventually pause execution
 *      (cycle count freezes).
 *   5. Sending `continue` MUST resume execution (cycle count grows
 *      again).
 *   6. Disconnecting the debugger MUST leave the engine running with
 *      cycles still advancing.
 *
 * This is the failure mode the previous implementation attempt hit:
 * attaching pulled the engine into a paused state even with
 * `stopOnEntry: false`, which broke the "live debug" use case. The test
 * is here to keep that regression locked out.
 *
 * Gated by `ST_E2E_ATTACH=1` so it runs in the dedicated electron job
 * but doesn't slow down the default `npm test` loop.
 */

const AGENT_HOST = process.env.ST_AGENT_HOST || "127.0.0.1";
const AGENT_PORT = parseInt(process.env.ST_AGENT_PORT || "4860", 10);
const DAP_PORT = AGENT_PORT + 1;

function enabled(): boolean {
  return process.env.ST_E2E_ATTACH === "1" || process.env.ST_E2E_REMOTE === "1";
}

let agentProcess: child_process.ChildProcess | null = null;
let tmpDir: string;
let projectDir: string;
let bundlePath: string;
let stCli: string;
let agentBin: string;

const COUNTER_PROGRAM = `PROGRAM Main
VAR
    counter : INT := 0;
    branch : INT := 0;
END_VAR
    counter := counter + 1;
    branch := counter * 2;
END_PROGRAM
`;

suite("Non-intrusive debug attach E2E", () => {
  suiteSetup(async function () {
    this.timeout(60000);
    if (!enabled()) {
      console.log("Skipping attach-running tests (set ST_E2E_ATTACH=1)");
      return;
    }

    tmpDir = path.join(os.tmpdir(), "st-attach-running-test-" + Date.now());
    fs.mkdirSync(tmpDir, { recursive: true });

    stCli = findBinary("st-cli");
    agentBin = findBinary("st-target-agent");

    // Build a development bundle (with sources — the DAP attach handler
    // needs them for source-mapped breakpoints).
    projectDir = path.join(tmpDir, "project");
    fs.mkdirSync(projectDir, { recursive: true });
    fs.writeFileSync(
      path.join(projectDir, "plc-project.yaml"),
      "name: AttachRunning\nversion: '1.0.0'\nentryPoint: Main\nengine:\n  cycle_time: 5ms\n",
    );
    fs.writeFileSync(path.join(projectDir, "main.st"), COUNTER_PROGRAM);
    bundlePath = path.join(tmpDir, "program.st-bundle");
    child_process.execSync(`${stCli} bundle ${projectDir} -o ${bundlePath}`, { encoding: "utf8" });

    // Spin up a private agent on AGENT_PORT so this test doesn't collide
    // with other suites running locally.
    const programDir = path.join(tmpDir, "programs");
    fs.mkdirSync(programDir, { recursive: true });
    const agentConfig = path.join(tmpDir, "agent.yaml");
    fs.writeFileSync(
      agentConfig,
      [
        "agent:",
        "  name: attach-running-test",
        "network:",
        `  bind: ${AGENT_HOST}`,
        `  port: ${AGENT_PORT}`,
        "runtime:",
        "  auto_start: false",
        "storage:",
        `  program_dir: ${programDir}`,
        `  log_dir: ${tmpDir}/logs`,
      ].join("\n"),
    );

    // Don't reuse a stale agent — always start a fresh one.
    agentProcess = child_process.spawn(agentBin, ["--config", agentConfig], {
      stdio: ["ignore", "pipe", "pipe"],
      detached: false,
    });
    agentProcess.stderr?.on("data", (d: Buffer) => {
      const line = d.toString().trim();
      if (line) console.log(`[AGENT] ${line}`);
    });

    await waitForAgent(20000);

    // Upload + start the program so subsequent tests find an actively
    // cycling engine.
    await uploadBundle(bundlePath);
    const startResp = await agentPost("/api/v1/program/start");
    assert.strictEqual(startResp.status, 200, `start failed: ${JSON.stringify(startResp)}`);
    await waitForRunning(5000);
  });

  suiteTeardown(async function () {
    this.timeout(15000);
    if (!enabled()) return;
    await forceStopSession();
    if (agentProcess) {
      agentProcess.kill("SIGTERM");
      agentProcess = null;
    }
    if (tmpDir && fs.existsSync(tmpDir)) {
      try { fs.rmSync(tmpDir, { recursive: true, force: true }); } catch { /* ignore */ }
    }
  });

  setup(async function () {
    this.timeout(15000);
    if (!enabled()) return;
    await forceStopSession();
    await sleep(500);
  });

  teardown(async function () {
    this.timeout(15000);
    if (!enabled()) return;
    await forceStopSession();
  });

  // ── Tests ────────────────────────────────────────────────────────────

  test("Attach with stopOnEntry=false does NOT pause the running engine", async function () {
    if (!enabled()) return this.skip();
    this.timeout(40000);

    // Sanity: the engine must already be running before we attach.
    const before = await currentCycleCount();
    assert.ok(before > 0, `engine must be cycling pre-attach, got: ${before}`);

    // Attach with stopOnEntry=false (the default for "live monitoring"
    // users — they want to set breakpoints on demand without halting
    // production execution).
    const session = await startAttachSession({ stopOnEntry: false });

    // Give the DAP session a beat to finish the initialize/attach/
    // configurationDone handshake. After that, the engine MUST be still
    // cycling.
    await sleep(800);
    const afterAttach = await currentCycleCount();
    assert.ok(
      afterAttach > before + 5,
      `engine must keep cycling after attach (stopOnEntry=false), ` +
      `cycles: ${before} → ${afterAttach}`,
    );

    // And it must stay running for at least another second.
    const stillRunning = await currentCycleCount();
    await sleep(1000);
    const after1s = await currentCycleCount();
    assert.ok(
      after1s > stillRunning + 5,
      `engine must KEEP cycling 1s after attach, cycles: ${stillRunning} → ${after1s}`,
    );

    // Status endpoint must report 'running', not 'debugpaused'.
    const status = await agentGet("/api/v1/status");
    assert.strictEqual(
      status.body.status,
      "running",
      `status should remain 'running' after attach, got: ${status.body.status}`,
    );

    await stopSession(session);
  });

  test("Setting a breakpoint freezes the cycle counter, continue resumes it", async function () {
    if (!enabled()) return this.skip();
    this.timeout(60000);

    // Pass localRoot so the agent's PathMapper can translate client paths
    // (`${projectDir}/main.st`) to its on-disk source path
    // (`${programDir}/current_source/main.st`) — which is what the SourceMap
    // and find_source_content keys are indexed by.
    const session = await startAttachSession({
      stopOnEntry: false,
      localRoot: projectDir,
    });
    await sleep(500);

    // Body of the PROGRAM: `counter := counter + 1;` is line 5 (0-indexed)
    // in COUNTER_PROGRAM. DAP setBreakpoints uses 1-indexed lines, so 6.
    const localSource = path.join(projectDir, "main.st");
    await session.customRequest("setBreakpoints", {
      source: { path: localSource },
      breakpoints: [{ line: 6 }],
    });

    // The breakpoint should fire on the next cycle. Give it up to 3s.
    const deadline = Date.now() + 3000;
    let frozenAt = 0;
    while (Date.now() < deadline) {
      const c1 = await currentCycleCount();
      await sleep(250);
      const c2 = await currentCycleCount();
      if (c2 === c1) {
        frozenAt = c2;
        break;
      }
    }
    assert.ok(
      frozenAt > 0,
      "cycle counter never froze — breakpoint did not fire within 3s",
    );

    // Confirm the agent reports 'debugpaused'.
    const paused = await agentGet("/api/v1/status");
    assert.strictEqual(paused.body.status, "debugpaused", `expected debugpaused, got: ${paused.body.status}`);

    // Clearing the breakpoint *and* continuing must let the engine resume
    // freely. (Without clearing, the BP on `counter := counter + 1;` would
    // re-fire every cycle since that line runs once per cycle — so the
    // counter would advance only one tick at a time.)
    await session.customRequest("setBreakpoints", {
      source: { path: localSource },
      breakpoints: [],
    });
    await session.customRequest("continue", { threadId: 1 });
    await sleep(800);
    const after = await currentCycleCount();
    assert.ok(
      after > frozenAt + 10,
      `cycles must advance after clear+continue: ${frozenAt} → ${after}`,
    );

    // Status must return to 'running' after we let go.
    const resumed = await agentGet("/api/v1/status");
    assert.strictEqual(
      resumed.body.status,
      "running",
      `status must return to 'running' after continue, got: ${resumed.body.status}`,
    );

    await stopSession(session);
  });

  test("Disconnecting the debugger leaves the engine running", async function () {
    if (!enabled()) return this.skip();
    this.timeout(40000);

    const session = await startAttachSession({ stopOnEntry: false });
    await sleep(500);
    const before = await currentCycleCount();

    await stopSession(session);

    // Engine must still be cycling — no auto-stop on disconnect.
    await sleep(800);
    const after = await currentCycleCount();
    assert.ok(
      after > before + 5,
      `engine must keep running after disconnect: ${before} → ${after}`,
    );

    const status = await agentGet("/api/v1/status");
    assert.strictEqual(
      status.body.status,
      "running",
      `status must be 'running' after disconnect, got: ${status.body.status}`,
    );
  });
});

// ── helpers ────────────────────────────────────────────────────────────

function findBinary(name: string): string {
  const candidates = [
    path.resolve(__dirname, `../../../../../target/debug/${name}`),
    path.resolve(__dirname, `../../../../target/debug/${name}`),
    name,
  ];
  for (const c of candidates) {
    try {
      child_process.execSync(`${c} --help`, { stdio: "ignore" });
      return c;
    } catch { /* try next */ }
  }
  throw new Error(`${name} not found. Run \`cargo build -p ${name}\` first.`);
}

async function startAttachSession(opts: {
  stopOnEntry: boolean;
  localRoot?: string;
}): Promise<vscode.DebugSession> {
  const config: vscode.DebugConfiguration = {
    type: "st",
    name: "Attach Running",
    request: "attach",
    host: AGENT_HOST,
    port: DAP_PORT,
    stopOnEntry: opts.stopOnEntry,
    ...(opts.localRoot ? { localRoot: opts.localRoot } : {}),
  };

  const sessionStarted = new Promise<vscode.DebugSession>((resolve, reject) => {
    const startD = vscode.debug.onDidStartDebugSession((s) => {
      startD.dispose();
      termD.dispose();
      resolve(s);
    });
    const termD = vscode.debug.onDidTerminateDebugSession(() => {
      startD.dispose();
      termD.dispose();
      reject(new Error("debug session terminated before start"));
    });
    setTimeout(() => {
      startD.dispose();
      termD.dispose();
      reject(new Error("startDebugSession timed out"));
    }, 8000);
  });

  const launched = await vscode.debug.startDebugging(undefined, config);
  assert.ok(launched, "startDebugging returned false");
  return await sessionStarted;
}

async function stopSession(session: vscode.DebugSession | undefined) {
  if (!session) return;
  try {
    await vscode.debug.stopDebugging(session);
  } catch { /* ignore */ }
  await waitForNoSession(5000);
}

async function forceStopSession() {
  const session = vscode.debug.activeDebugSession;
  if (session) {
    try { await vscode.debug.stopDebugging(session); } catch { /* ignore */ }
    await waitForNoSession(5000);
  }
}

function waitForNoSession(timeoutMs: number): Promise<void> {
  return new Promise((resolve) => {
    if (!vscode.debug.activeDebugSession) {
      resolve();
      return;
    }
    const d = vscode.debug.onDidTerminateDebugSession(() => {
      d.dispose();
      resolve();
    });
    setTimeout(() => { d.dispose(); resolve(); }, timeoutMs);
  });
}

async function uploadBundle(filePath: string): Promise<any> {
  return new Promise((resolve, reject) => {
    const boundary = "----AttachRunningUpload" + Date.now();
    const data = fs.readFileSync(filePath);
    const head = Buffer.from(
      `--${boundary}\r\nContent-Disposition: form-data; name="file"; filename="${path.basename(filePath)}"\r\n` +
      `Content-Type: application/octet-stream\r\n\r\n`,
    );
    const tail = Buffer.from(`\r\n--${boundary}--\r\n`);
    const body = Buffer.concat([head, data, tail]);
    const req = http.request(
      {
        hostname: AGENT_HOST,
        port: AGENT_PORT,
        path: "/api/v1/program/upload",
        method: "POST",
        headers: {
          "Content-Type": `multipart/form-data; boundary=${boundary}`,
          "Content-Length": body.length,
        },
      },
      (res) => {
        let raw = "";
        res.on("data", (c) => { raw += c.toString(); });
        res.on("end", () => {
          try { resolve(JSON.parse(raw)); }
          catch { reject(new Error(`upload bad json: ${raw}`)); }
        });
      },
    );
    req.on("error", reject);
    req.write(body);
    req.end();
  });
}

function agentGet(urlPath: string): Promise<{ status: number; body: any }> {
  return new Promise((resolve, reject) => {
    const req = http.get({ hostname: AGENT_HOST, port: AGENT_PORT, path: urlPath }, (res) => {
      let raw = "";
      res.on("data", (c) => { raw += c.toString(); });
      res.on("end", () => {
        try { resolve({ status: res.statusCode || 0, body: raw ? JSON.parse(raw) : null }); }
        catch { resolve({ status: res.statusCode || 0, body: raw }); }
      });
    });
    req.on("error", reject);
    req.setTimeout(5000, () => req.destroy(new Error("HTTP timeout")));
  });
}

function agentPost(urlPath: string): Promise<{ status: number; body: any }> {
  return new Promise((resolve, reject) => {
    const req = http.request(
      { hostname: AGENT_HOST, port: AGENT_PORT, path: urlPath, method: "POST" },
      (res) => {
        let raw = "";
        res.on("data", (c) => { raw += c.toString(); });
        res.on("end", () => {
          try { resolve({ status: res.statusCode || 0, body: raw ? JSON.parse(raw) : null }); }
          catch { resolve({ status: res.statusCode || 0, body: raw }); }
        });
      },
    );
    req.on("error", reject);
    req.end();
  });
}

async function isAgentReachable(): Promise<boolean> {
  return new Promise((resolve) => {
    const req = http.get(
      { hostname: AGENT_HOST, port: AGENT_PORT, path: "/api/v1/health" },
      (res) => { res.resume(); resolve(res.statusCode === 200); },
    );
    req.on("error", () => resolve(false));
    req.setTimeout(800, () => { req.destroy(); resolve(false); });
  });
}

async function waitForAgent(timeoutMs: number): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await isAgentReachable()) return;
    await sleep(250);
  }
  throw new Error(`agent at ${AGENT_HOST}:${AGENT_PORT} not ready within ${timeoutMs}ms`);
}

async function waitForRunning(timeoutMs: number): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    const r = await agentGet("/api/v1/status").catch(() => undefined);
    if (r && r.body?.status === "running") return;
    await sleep(150);
  }
  throw new Error(`engine did not reach 'running' within ${timeoutMs}ms`);
}

async function currentCycleCount(): Promise<number> {
  const r = await agentGet("/api/v1/status");
  return Number(r.body?.cycle_stats?.cycle_count ?? 0);
}

function sleep(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}
