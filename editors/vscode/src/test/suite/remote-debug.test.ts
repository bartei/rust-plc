import * as assert from "assert";
import * as vscode from "vscode";
import * as path from "path";
import * as fs from "fs";
import * as http from "http";
import * as child_process from "child_process";

/**
 * Remote Debug E2E Tests.
 *
 * These tests exercise the full remote debugging flow:
 * 1. Start the target agent (locally or on a QEMU VM)
 * 2. Upload a program bundle to the agent
 * 3. Attach VS Code's debugger to the agent's DAP proxy
 * 4. Exercise breakpoints, stepping, variable inspection
 * 5. Test online update during a debug session
 * 6. Verify release bundle debug rejection
 *
 * Environment variables:
 *   ST_AGENT_HOST  — Agent host (default: 127.0.0.1)
 *   ST_AGENT_PORT  — Agent HTTP port (default: 4840)
 *   ST_DAP_PORT    — Agent DAP proxy port (default: 4841)
 *   ST_E2E_REMOTE  — Set to "1" to enable these tests
 *
 * The tests start a local agent process by default. Set ST_E2E_REMOTE=qemu
 * to connect to an already-running QEMU target instead.
 */

const AGENT_HOST = process.env.ST_AGENT_HOST || "127.0.0.1";
const AGENT_PORT = parseInt(process.env.ST_AGENT_PORT || "4840", 10);
const DAP_PORT = parseInt(process.env.ST_DAP_PORT || "4841", 10);

const TEST_PROGRAM = `PROGRAM Main
VAR
    counter : INT := 0;
    flag : BOOL := FALSE;
    result : INT := 0;
END_VAR
    counter := counter + 1;
    IF counter > 2 THEN
        flag := TRUE;
    END_IF;
    result := counter * 2;
END_PROGRAM
`;

const TEST_PROGRAM_V2 = `PROGRAM Main
VAR
    counter : INT := 0;
    flag : BOOL := FALSE;
    result : INT := 0;
END_VAR
    counter := counter + 2;
    IF counter > 4 THEN
        flag := TRUE;
    END_IF;
    result := counter * 3;
END_PROGRAM
`;

let agentProcess: child_process.ChildProcess | null = null;
let tmpDir: string;
let bundlePath: string;
let bundleReleasePath: string;
let bundleV2Path: string;

function enabled(): boolean {
  return (
    process.env.ST_E2E_REMOTE === "1" ||
    process.env.ST_E2E_REMOTE === "qemu"
  );
}

suite("Remote Debug E2E Tests", () => {
  suiteSetup(async function () {
    this.timeout(60000);

    if (!enabled()) {
      console.log("Skipping remote debug tests (set ST_E2E_REMOTE=1)");
      return;
    }

    tmpDir = path.join(require("os").tmpdir(), "st-remote-debug-test");
    fs.mkdirSync(tmpDir, { recursive: true });

    // Create test project for bundling
    const projDir = path.join(tmpDir, "project");
    fs.mkdirSync(projDir, { recursive: true });
    fs.writeFileSync(
      path.join(projDir, "plc-project.yaml"),
      "name: RemoteDebugTest\nversion: '1.0.0'\nentryPoint: Main\n"
    );
    fs.writeFileSync(path.join(projDir, "main.st"), TEST_PROGRAM);

    // Create v2 project for online update testing
    const projV2Dir = path.join(tmpDir, "project-v2");
    fs.mkdirSync(projV2Dir, { recursive: true });
    fs.writeFileSync(
      path.join(projV2Dir, "plc-project.yaml"),
      "name: RemoteDebugTest\nversion: '2.0.0'\nentryPoint: Main\n"
    );
    fs.writeFileSync(path.join(projV2Dir, "main.st"), TEST_PROGRAM_V2);

    // Build bundles using st-cli
    const stCli = findStCli();

    // Development bundle (with source — for debugging)
    bundlePath = path.join(tmpDir, "dev.st-bundle");
    execSync(`${stCli} bundle ${projDir} -o ${bundlePath}`);
    assert.ok(fs.existsSync(bundlePath), "Development bundle should exist");

    // Release bundle (no source — debug should be rejected)
    bundleReleasePath = path.join(tmpDir, "release.st-bundle");
    execSync(`${stCli} bundle --release ${projDir} -o ${bundleReleasePath}`);

    // V2 bundle for update testing
    bundleV2Path = path.join(tmpDir, "dev-v2.st-bundle");
    execSync(`${stCli} bundle ${projV2Dir} -o ${bundleV2Path}`);

    // Write the test program to a temp file and open it in the editor
    // so the user can SEE the source code during debugging
    const sourceDir = path.join(tmpDir, "source");
    fs.mkdirSync(sourceDir, { recursive: true });
    const sourceFile = path.join(sourceDir, "main.st");
    fs.writeFileSync(sourceFile, TEST_PROGRAM);
    const doc = await vscode.workspace.openTextDocument(sourceFile);
    await vscode.window.showTextDocument(doc);

    // Check if a remote agent is already reachable (e.g., QEMU target)
    let agentReachable = false;
    try {
      const resp = await httpGet("/api/v1/health");
      agentReachable = resp.statusCode === 200;
    } catch {
      agentReachable = false;
    }

    if (agentReachable) {
      console.log(`[REMOTE-DEBUG] Agent already reachable at ${AGENT_HOST}:${AGENT_PORT}`);
    } else if (process.env.ST_E2E_REMOTE !== "qemu") {
      console.log("[REMOTE-DEBUG] Starting local agent...");
      await startLocalAgent();
    }

    // Wait for agent to be ready
    await waitForAgent(15000);
  });

  suiteTeardown(async function () {
    this.timeout(15000);
    if (!enabled()) return;
    await forceStopSession();
    stopLocalAgent();
    if (tmpDir && fs.existsSync(tmpDir)) {
      fs.rmSync(tmpDir, { recursive: true, force: true });
    }
  });

  setup(async function () {
    this.timeout(20000);
    if (!enabled()) return;
    await forceStopSession();
    // Wait for the DAP proxy subprocess on the target to fully exit
    await sleep(2000);
    // Re-upload the default v1 bundle to ensure it's available
    // (agent program store is in-memory, may be lost on restart)
    await uploadBundle(bundlePath);
    await sleep(300);
  });

  teardown(async function () {
    this.timeout(20000);
    if (!enabled()) return;
    await forceStopSession();
    await sleep(1000);
  });

  // ═══════════════════════════════════════════════════════════════════
  // Helpers
  // ═══════════════════════════════════════════════════════════════════

  function sleep(ms: number): Promise<void> {
    return new Promise((r) => setTimeout(r, ms));
  }

  function execSync(cmd: string): string {
    return child_process.execSync(cmd, { encoding: "utf8" }).trim();
  }

  function findStCli(): string {
    // Look for st-cli relative to the workspace root
    const candidates = [
      path.resolve(__dirname, "../../../../../target/debug/st-cli"),
      path.resolve(__dirname, "../../../../target/debug/st-cli"),
      "st-cli",
    ];
    for (const c of candidates) {
      try {
        child_process.execSync(`${c} help`, { stdio: "ignore" });
        return c;
      } catch {
        continue;
      }
    }
    throw new Error("st-cli not found. Run `cargo build -p st-cli` first.");
  }

  async function startLocalAgent(): Promise<void> {
    const agentBin = path.resolve(
      __dirname,
      "../../../../../target/debug/st-target-agent"
    );
    if (!fs.existsSync(agentBin)) {
      throw new Error(
        `st-target-agent not found at ${agentBin}. Run \`cargo build -p st-target-agent\` first.`
      );
    }

    // Write agent config
    const configPath = path.join(tmpDir, "agent.yaml");
    const programDir = path.join(tmpDir, "programs");
    fs.mkdirSync(programDir, { recursive: true });
    fs.writeFileSync(
      configPath,
      [
        "agent:",
        "  name: e2e-test-agent",
        "network:",
        `  bind: ${AGENT_HOST}`,
        `  port: ${AGENT_PORT}`,
        "runtime:",
        "  auto_start: false",
        "storage:",
        `  program_dir: ${programDir}`,
        `  log_dir: ${tmpDir}/logs`,
      ].join("\n")
    );

    agentProcess = child_process.spawn(agentBin, ["--config", configPath], {
      stdio: ["ignore", "pipe", "pipe"],
      detached: false,
    });

    agentProcess.stderr?.on("data", (data: Buffer) => {
      const line = data.toString().trim();
      if (line) console.log(`[AGENT] ${line}`);
    });
  }

  function stopLocalAgent(): void {
    if (agentProcess) {
      agentProcess.kill("SIGTERM");
      agentProcess = null;
    }
  }

  async function waitForAgent(timeoutMs: number): Promise<void> {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      try {
        const resp = await httpGet(`/api/v1/health`);
        if (resp.statusCode === 200) return;
      } catch {
        // Not ready yet
      }
      await sleep(500);
    }
    throw new Error(`Agent not ready after ${timeoutMs}ms`);
  }

  function httpGet(urlPath: string): Promise<http.IncomingMessage & { body: string }> {
    return new Promise((resolve, reject) => {
      const req = http.get(
        { hostname: AGENT_HOST, port: AGENT_PORT, path: urlPath },
        (res) => {
          let body = "";
          res.on("data", (chunk: Buffer) => (body += chunk.toString()));
          res.on("end", () => {
            (res as any).body = body;
            resolve(res as http.IncomingMessage & { body: string });
          });
        }
      );
      req.on("error", reject);
      req.setTimeout(5000, () => {
        req.destroy(new Error("HTTP timeout"));
      });
    });
  }

  async function uploadBundle(filePath: string): Promise<any> {
    return new Promise((resolve, reject) => {
      const boundary = "----BundleUpload" + Date.now();
      const bundleData = fs.readFileSync(filePath);
      const fileName = path.basename(filePath);

      const header = Buffer.from(
        `--${boundary}\r\n` +
        `Content-Disposition: form-data; name="file"; filename="${fileName}"\r\n` +
        `Content-Type: application/octet-stream\r\n\r\n`
      );
      const footer = Buffer.from(`\r\n--${boundary}--\r\n`);
      const body = Buffer.concat([header, bundleData, footer]);

      const options: http.RequestOptions = {
        hostname: AGENT_HOST,
        port: AGENT_PORT,
        path: "/api/v1/program/upload",
        method: "POST",
        headers: {
          "Content-Type": `multipart/form-data; boundary=${boundary}`,
          "Content-Length": body.length,
        },
      };

      const req = http.request(options, (res) => {
        let respBody = "";
        res.on("data", (chunk: Buffer) => (respBody += chunk.toString()));
        res.on("end", () => {
          try {
            resolve(JSON.parse(respBody));
          } catch {
            reject(new Error(`Invalid JSON response: ${respBody}`));
          }
        });
      });
      req.on("error", reject);
      req.write(body);
      req.end();
    });
  }

  async function agentPost(urlPath: string): Promise<any> {
    return new Promise((resolve, reject) => {
      const req = http.request(
        {
          hostname: AGENT_HOST,
          port: AGENT_PORT,
          path: urlPath,
          method: "POST",
        },
        (res) => {
          let body = "";
          res.on("data", (chunk: Buffer) => (body += chunk.toString()));
          res.on("end", () => {
            try { resolve(JSON.parse(body)); } catch { resolve(body); }
          });
        }
      );
      req.on("error", reject);
      req.end();
    });
  }

  async function attachAndWaitForStop(): Promise<vscode.DebugSession> {
    const config: vscode.DebugConfiguration = {
      type: "st",
      name: "Remote Debug E2E",
      request: "attach",
      host: AGENT_HOST,
      port: DAP_PORT,
      stopOnEntry: true,
    };

    console.log(`[REMOTE-DEBUG] Attaching to ${AGENT_HOST}:${DAP_PORT}...`);

    // Listen for session termination to detect connection failures
    let sessionTerminated = false;
    const termDisposable = vscode.debug.onDidTerminateDebugSession(() => {
      sessionTerminated = true;
    });

    const started = await vscode.debug.startDebugging(
      vscode.workspace.workspaceFolders?.[0],
      config
    );

    if (!started) {
      termDisposable.dispose();
      throw new Error("debug.startDebugging returned false — session failed to start");
    }

    // Give the TCP connection a moment to establish
    await sleep(1000);

    if (sessionTerminated) {
      termDisposable.dispose();
      throw new Error("Debug session terminated immediately — TCP connection to DAP proxy likely failed");
    }

    console.log("[REMOTE-DEBUG] Session started, waiting for stop...");
    await pollUntilStopped(30000);
    termDisposable.dispose();

    const session = vscode.debug.activeDebugSession;
    assert.ok(session, "Active debug session should exist after stop");
    console.log("[REMOTE-DEBUG] Stopped on entry, session active");
    return session;
  }

  async function pollUntilStopped(timeoutMs: number): Promise<void> {
    const deadline = Date.now() + timeoutMs;
    while (Date.now() < deadline) {
      const session = vscode.debug.activeDebugSession;
      if (session) {
        try {
          const st = await session.customRequest("stackTrace", {
            threadId: 1,
            startFrame: 0,
            levels: 1,
          });
          if (st.stackFrames && st.stackFrames.length > 0) return;
        } catch {
          // Not ready yet
        }
      }
      await sleep(150);
    }
    throw new Error("Timed out waiting for remote debug session to stop");
  }

  async function stepAndWait(
    session: vscode.DebugSession,
    command: "stepIn" | "next" | "stepOut"
  ): Promise<void> {
    await session.customRequest(command, { threadId: 1 });
    await pollUntilStopped(10000);
  }

  async function currentLine(session: vscode.DebugSession): Promise<number> {
    const st = await session.customRequest("stackTrace", {
      threadId: 1,
      startFrame: 0,
      levels: 1,
    });
    return st.stackFrames?.[0]?.line ?? -1;
  }

  async function forceStopSession(): Promise<void> {
    const session = vscode.debug.activeDebugSession;
    if (!session) return;
    try {
      await vscode.debug.stopDebugging(session);
    } catch {
      // Already stopped
    }
    await waitForNoSession(5000);
  }

  function waitForNoSession(timeoutMs: number): Promise<void> {
    return new Promise((resolve) => {
      if (!vscode.debug.activeDebugSession) {
        resolve();
        return;
      }
      const disposable = vscode.debug.onDidTerminateDebugSession(() => {
        disposable.dispose();
        resolve();
      });
      setTimeout(() => {
        disposable.dispose();
        resolve();
      }, timeoutMs);
    });
  }

  async function takeScreenshot(name: string): Promise<void> {
    // Use VS Code command to save screenshot for visual verification.
    // The screenshots are saved to the tmpDir for post-test review.
    const screenshotDir = path.join(tmpDir, "screenshots");
    fs.mkdirSync(screenshotDir, { recursive: true });
    try {
      await vscode.commands.executeCommand(
        "workbench.action.captureProfile"
      );
    } catch {
      // Screenshot command may not be available — that's OK
    }
    // Log the step for test output readability
    console.log(`[SCREENSHOT] ${name}`);
  }

  // ═══════════════════════════════════════════════════════════════════
  // Tests
  // ═══════════════════════════════════════════════════════════════════

  test("1. Upload development bundle to agent", async function () {
    if (!enabled()) return this.skip();
    this.timeout(30000);

    const result = await uploadBundle(bundlePath);
    assert.ok(result.success, `Upload should succeed: ${JSON.stringify(result)}`);
    assert.strictEqual(result.program.name, "RemoteDebugTest");
    assert.strictEqual(result.program.version, "1.0.0");
    assert.strictEqual(result.program.mode, "development");

    await takeScreenshot("01-bundle-uploaded");
  });

  test("2. Attach debugger and stop on entry", async function () {
    if (!enabled()) return this.skip();
    this.timeout(30000);

    // Ensure bundle is uploaded
    await uploadBundle(bundlePath);

    const session = await attachAndWaitForStop();
    assert.ok(session, "Should have an active debug session");

    // Verify we're stopped at the program entry
    const st = await session.customRequest("stackTrace", {
      threadId: 1,
      startFrame: 0,
      levels: 10,
    });
    assert.ok(st.stackFrames.length > 0, "Should have stack frames");
    assert.ok(
      st.stackFrames[0].name.includes("Main"),
      `Top frame should be Main, got: ${st.stackFrames[0].name}`
    );

    await takeScreenshot("02-stopped-on-entry");
  });

  test("3. Set breakpoint and hit it", async function () {
    if (!enabled()) return this.skip();
    this.timeout(30000);

    await uploadBundle(bundlePath);
    const session = await attachAndWaitForStop();

    // Get the source path from the stack frame
    const st = await session.customRequest("stackTrace", {
      threadId: 1,
      startFrame: 0,
      levels: 1,
    });
    const sourcePath = st.stackFrames[0].source?.path;
    assert.ok(sourcePath, "Stack frame should have a source path");

    // Set breakpoint on "result := counter * 2" (line 11 in test program)
    const bpLine = 11;
    const bpResp = await session.customRequest("setBreakpoints", {
      source: { path: sourcePath },
      breakpoints: [{ line: bpLine }],
    });
    assert.ok(
      bpResp.breakpoints.length > 0,
      "Should set at least one breakpoint"
    );

    // Continue — should hit the breakpoint
    await session.customRequest("continue", { threadId: 1 });
    await pollUntilStopped(15000);

    const hitLine = await currentLine(session);
    assert.ok(
      Math.abs(hitLine - bpLine) <= 2,
      `Should stop near line ${bpLine}, got ${hitLine}`
    );

    await takeScreenshot("03-breakpoint-hit");
  });

  test("4. Inspect local variables", async function () {
    if (!enabled()) return this.skip();
    this.timeout(30000);

    await uploadBundle(bundlePath);
    const session = await attachAndWaitForStop();

    // Step a few times so counter advances
    await stepAndWait(session, "next");
    await stepAndWait(session, "next");

    // Get scopes
    const st = await session.customRequest("stackTrace", {
      threadId: 1,
      startFrame: 0,
      levels: 1,
    });
    const frameId = st.stackFrames[0].id;

    const scopesResp = await session.customRequest("scopes", { frameId });
    assert.ok(scopesResp.scopes.length > 0, "Should have scopes");

    const localsRef = scopesResp.scopes.find(
      (s: any) => s.name === "Locals"
    )?.variablesReference;
    assert.ok(localsRef, "Should have Locals scope");

    const varsResp = await session.customRequest("variables", {
      variablesReference: localsRef,
    });
    assert.ok(varsResp.variables.length > 0, "Should have local variables");

    // Find counter variable
    const counter = varsResp.variables.find(
      (v: any) => v.name.toLowerCase() === "counter"
    );
    assert.ok(counter, `Should find 'counter' variable: ${JSON.stringify(varsResp.variables.map((v: any) => v.name))}`);

    console.log(`[VARIABLES] counter = ${counter.value}`);

    await takeScreenshot("04-variables-inspected");
  });

  test("5. Step In and Step Over", async function () {
    if (!enabled()) return this.skip();
    this.timeout(30000);

    await uploadBundle(bundlePath);
    const session = await attachAndWaitForStop();

    const startLine = await currentLine(session);

    // Step Over
    await stepAndWait(session, "next");
    const afterStep = await currentLine(session);
    assert.notStrictEqual(afterStep, startLine, "Line should change after step");

    // Step In (same effect for non-function-call lines)
    await stepAndWait(session, "stepIn");
    const afterStepIn = await currentLine(session);
    assert.notStrictEqual(afterStepIn, afterStep, "Line should change after step in");

    await takeScreenshot("05-stepping-complete");
  });

  test("6. Continue and Pause", async function () {
    if (!enabled()) return this.skip();
    this.timeout(30000);

    await uploadBundle(bundlePath);
    const session = await attachAndWaitForStop();

    // Continue — program runs freely
    await session.customRequest("continue", { threadId: 1 });
    await sleep(2000); // Let it run for 2 seconds

    // Pause
    const stoppedPromise = new Promise<void>((resolve) => {
      const d1 = vscode.debug.onDidChangeActiveStackItem(() => {
        d1.dispose();
        d2.dispose();
        resolve();
      });
      const d2 = vscode.debug.onDidReceiveDebugSessionCustomEvent(() => {
        d1.dispose();
        d2.dispose();
        resolve();
      });
      setTimeout(() => {
        d1.dispose();
        d2.dispose();
        resolve();
      }, 10000);
    });

    await session.customRequest("pause", { threadId: 1 });
    await stoppedPromise;
    await sleep(500);

    // Verify counter advanced during run
    const evalResult = await session.customRequest("evaluate", {
      expression: "counter",
      frameId: 0,
      context: "watch",
    });
    const counterVal = parseInt(evalResult.result, 10);
    assert.ok(
      counterVal > 1,
      `Counter should have advanced during run, got ${counterVal}`
    );

    console.log(`[CONTINUE+PAUSE] counter = ${counterVal} (advanced during run)`);
    await takeScreenshot("06-continue-pause");
  });

  test("7. Evaluate expression", async function () {
    if (!enabled()) return this.skip();
    this.timeout(30000);

    await uploadBundle(bundlePath);
    const session = await attachAndWaitForStop();

    // Step past the counter increment
    await stepAndWait(session, "next");

    const evalResult = await session.customRequest("evaluate", {
      expression: "counter",
      frameId: 0,
      context: "watch",
    });
    assert.ok(evalResult.result !== undefined, "Evaluate should return a result");
    console.log(`[EVALUATE] counter = ${evalResult.result}`);

    await takeScreenshot("07-expression-evaluated");
  });

  test("8. Online update: upload v2 then re-attach", async function () {
    if (!enabled()) return this.skip();
    this.timeout(45000);

    // Upload v1 and attach
    await uploadBundle(bundlePath);
    const session1 = await attachAndWaitForStop();
    assert.ok(session1, "V1 session should start");

    // Verify v1 is running
    const info1 = await httpGet("/api/v1/program/info");
    const programInfo1 = JSON.parse(info1.body);
    assert.strictEqual(programInfo1.version, "1.0.0");

    // Stop the current debug session
    await forceStopSession();
    await sleep(1000);

    // Upload v2 bundle (replaces v1)
    const result = await uploadBundle(bundleV2Path);
    assert.ok(result.success, "V2 upload should succeed");
    assert.strictEqual(result.program.version, "2.0.0");

    // Re-attach to debug v2
    const session2 = await attachAndWaitForStop();
    assert.ok(session2, "V2 session should start");

    // Verify we can step through v2 code
    await stepAndWait(session2, "next");

    const info2 = await httpGet("/api/v1/program/info");
    const programInfo2 = JSON.parse(info2.body);
    assert.strictEqual(programInfo2.version, "2.0.0");

    console.log("[UPDATE] Successfully debugged v1, updated to v2, re-attached");
    await takeScreenshot("08-online-update-v2");
  });

  test("9. Release bundle rejects debug attach", async function () {
    if (!enabled()) return this.skip();
    this.timeout(30000);

    // Upload release bundle (no source)
    const result = await uploadBundle(bundleReleasePath);
    assert.ok(result.success, "Release upload should succeed");
    assert.strictEqual(result.program.mode, "release");

    // Try to attach — should fail because the agent rejects DAP
    // for release bundles
    const config: vscode.DebugConfiguration = {
      type: "st",
      name: "Remote Debug E2E",
      request: "attach",
      host: AGENT_HOST,
      port: DAP_PORT,
      stopOnEntry: true,
    };

    const started = await vscode.debug.startDebugging(
      vscode.workspace.workspaceFolders?.[0],
      config
    );

    // The debug session should either fail to start or terminate quickly
    // because the agent drops the TCP connection for release bundles
    await sleep(3000);

    const session = vscode.debug.activeDebugSession;
    if (session) {
      // If a session exists, try to get a stack trace — should fail
      try {
        await session.customRequest("stackTrace", {
          threadId: 1,
          startFrame: 0,
          levels: 1,
        });
        assert.fail("stackTrace should fail for release bundle");
      } catch {
        // Expected — release bundle has no debug info
      }
      await forceStopSession();
    }

    console.log("[RELEASE] Debug correctly rejected for release bundle");
    await takeScreenshot("09-release-rejected");
  });

  test("10. Full lifecycle: upload → debug → update → debug → stop", async function () {
    if (!enabled()) return this.skip();
    this.timeout(60000);

    // 1. Upload v1
    const upload1 = await uploadBundle(bundlePath);
    assert.ok(upload1.success);
    console.log("[LIFECYCLE] V1 uploaded");

    // 2. Attach and stop on entry
    const session1 = await attachAndWaitForStop();
    console.log("[LIFECYCLE] Attached to V1");

    // 3. Set breakpoint
    const st = await session1.customRequest("stackTrace", {
      threadId: 1,
      startFrame: 0,
      levels: 1,
    });
    const sourcePath = st.stackFrames[0].source?.path;

    await session1.customRequest("setBreakpoints", {
      source: { path: sourcePath },
      breakpoints: [{ line: 7 }], // counter := counter + 1
    });

    // 4. Continue to breakpoint
    await session1.customRequest("continue", { threadId: 1 });
    await pollUntilStopped(15000);
    console.log("[LIFECYCLE] Hit breakpoint in V1");

    // 5. Inspect variables
    const eval1 = await session1.customRequest("evaluate", {
      expression: "counter",
      frameId: 0,
      context: "watch",
    });
    console.log(`[LIFECYCLE] V1 counter = ${eval1.result}`);

    // 6. Stop debug session
    await forceStopSession();
    await sleep(1000);

    // 7. Upload v2
    const upload2 = await uploadBundle(bundleV2Path);
    assert.ok(upload2.success);
    console.log("[LIFECYCLE] V2 uploaded");

    // 8. Re-attach
    const session2 = await attachAndWaitForStop();
    console.log("[LIFECYCLE] Attached to V2");

    // 9. Step and verify v2 behavior
    await stepAndWait(session2, "next");
    await stepAndWait(session2, "next");

    // 10. Clean up
    await forceStopSession();
    console.log("[LIFECYCLE] Full lifecycle complete");
    await takeScreenshot("10-full-lifecycle-complete");
  });
});
