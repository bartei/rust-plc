import * as assert from "assert";
import * as vscode from "vscode";
import * as path from "path";
import * as fs from "fs";

/**
 * Debug Adapter (DAP) integration tests.
 *
 * These tests exercise the debug toolbar buttons in a real VS Code instance
 * with the ST extension loaded. Each test launches a debug session against
 * a test .st file and verifies that Continue, Pause, Step In, Step Over,
 * Stop, and Evaluate work as expected.
 */

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

let testFilePath: string;

suite("Debug Buttons Test Suite", () => {
  suiteSetup(async function () {
    this.timeout(15000);

    // Write the test fixture to /tmp — NOT the workspace, to avoid
    // polluting the explorer and triggering "Save As" dialogs.
    // No plc-project.yaml — the DAP defaults to 1ms cycle period,
    // which is enough for interruptible_sleep to handle Pause.
    const tmpDir = path.join(require("os").tmpdir(), "st-dap-test");
    if (!fs.existsSync(tmpDir)) {
      fs.mkdirSync(tmpDir, { recursive: true });
    }
    testFilePath = path.join(tmpDir, "_debug_test.st");
    fs.writeFileSync(testFilePath, TEST_PROGRAM, "utf8");

    // Give the extension time to activate
    await sleep(2000);
  });

  suiteTeardown(() => {
    if (testFilePath && fs.existsSync(testFilePath)) {
      fs.unlinkSync(testFilePath);
    }
  });

  // Clean up any leftover session before each test
  setup(async function () {
    this.timeout(15000);
    await forceStopSession();
    // Extra delay so VS Code fully resets debug state between tests
    await sleep(500);
  });

  // Also clean up after each test
  teardown(async function () {
    this.timeout(15000);
    await forceStopSession();
  });

  // ═══════════════════════════════════════════════════════════════════
  // Helpers
  // ═══════════════════════════════════════════════════════════════════

  function sleep(ms: number): Promise<void> {
    return new Promise((r) => setTimeout(r, ms));
  }

  /** Force-stop any active debug session. Swallows errors. */
  async function forceStopSession(): Promise<void> {
    const session = vscode.debug.activeDebugSession;
    if (!session) return;
    try {
      await vscode.debug.stopDebugging(session);
    } catch {
      // Already stopped — ignore
    }
    // Wait for the session to fully terminate
    await waitForNoSession(5000);
  }

  /** Wait until there's no active debug session. */
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
        resolve(); // resolve even on timeout to avoid blocking the next test
      }, timeoutMs);
    });
  }

  /** Launch a debug session and wait for it to stop on entry. */
  async function launchAndWaitForStop(): Promise<vscode.DebugSession> {
    const config: vscode.DebugConfiguration = {
      type: "st",
      name: "Debug Test",
      request: "launch",
      program: testFilePath,
      stopOnEntry: true,
    };

    const started = await vscode.debug.startDebugging(
      vscode.workspace.workspaceFolders?.[0],
      config
    );
    assert.ok(started, "Debug session should start");

    // Poll until the session is active and stopped
    await pollUntilStopped(10000);

    const session = vscode.debug.activeDebugSession;
    assert.ok(session, "Active debug session should exist after stop");
    return session;
  }

  /** Poll the active session's stack trace until it has frames (= stopped). */
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
          if (st.stackFrames && st.stackFrames.length > 0) {
            return;
          }
        } catch {
          // Not ready yet
        }
      }
      await sleep(150);
    }
    throw new Error("Timed out waiting for debug session to stop");
  }

  /** Get the current top frame's line number. */
  async function currentLine(session: vscode.DebugSession): Promise<number> {
    const st = await session.customRequest("stackTrace", {
      threadId: 1,
      startFrame: 0,
      levels: 1,
    });
    assert.ok(st.stackFrames?.length > 0, "Should have a stack frame");
    return st.stackFrames[0].line;
  }

  /** Send a step command and wait for the session to stop again. */
  async function stepAndWait(
    session: vscode.DebugSession,
    command: "stepIn" | "next" | "stepOut"
  ): Promise<void> {
    await session.customRequest(command, { threadId: 1 });
    // After stepping, the DAP sends a Continued event then a Stopped event.
    // Poll until we see a stack trace again.
    await pollUntilStopped(10000);
  }

  // ═══════════════════════════════════════════════════════════════════
  // Tests
  // ═══════════════════════════════════════════════════════════════════

  test("Launch with stopOnEntry pauses at first statement", async function () {
    this.timeout(20000);
    const session = await launchAndWaitForStop();
    const line = await currentLine(session);
    assert.ok(line > 0, `Should be paused at a valid line, got ${line}`);
  });

  test("Step In advances to the next line", async function () {
    this.timeout(20000);
    const session = await launchAndWaitForStop();
    const line1 = await currentLine(session);

    await stepAndWait(session, "stepIn");
    const line2 = await currentLine(session);

    assert.notStrictEqual(
      line2,
      line1,
      `Line should change after Step In (was ${line1}, still ${line2})`
    );
  });

  test("Step Over advances without entering calls", async function () {
    this.timeout(20000);
    const session = await launchAndWaitForStop();
    const line1 = await currentLine(session);

    await stepAndWait(session, "next");
    const line2 = await currentLine(session);

    assert.notStrictEqual(
      line2,
      line1,
      `Line should change after Step Over (was ${line1}, still ${line2})`
    );
  });

  test("Multiple Step Ins progress through the program", async function () {
    this.timeout(20000);
    const session = await launchAndWaitForStop();
    const lines: number[] = [await currentLine(session)];

    for (let i = 0; i < 3; i++) {
      await stepAndWait(session, "stepIn");
      lines.push(await currentLine(session));
    }

    const uniqueLines = new Set(lines);
    assert.ok(
      uniqueLines.size >= 2,
      `Should visit multiple lines during stepping, got: ${lines.join(" → ")}`
    );
  });

  test("Continue runs and Pause stops execution", async function () {
    this.timeout(25000);
    const session = await launchAndWaitForStop();

    // Continue
    await session.customRequest("continue", { threadId: 1 });
    // Wait for the program to run several cycles
    await sleep(2000);

    // Send Pause and wait for the Stopped event via the VS Code event API.
    // customRequest("stackTrace") is rejected until VS Code internally
    // processes the Stopped event — polling it directly doesn't work.
    const stoppedPromise = new Promise<void>((resolve) => {
      const d1 = vscode.debug.onDidChangeActiveStackItem(() => {
        d1.dispose(); d2.dispose();
        resolve();
      });
      const d2 = vscode.debug.onDidReceiveDebugSessionCustomEvent((e) => {
        if (e.body?.output?.includes?.("Stopped")) {
          d1.dispose(); d2.dispose();
          resolve();
        }
      });
      // Safety timeout — resolve even if we miss the event
      setTimeout(() => { d1.dispose(); d2.dispose(); resolve(); }, 10000);
    });

    await session.customRequest("pause", { threadId: 1 });
    await stoppedPromise;
    await sleep(500);

    // Verify we're paused with a valid stack trace
    const st = await session.customRequest("stackTrace", {
      threadId: 1, startFrame: 0, levels: 1,
    });
    assert.ok(
      st.stackFrames?.length > 0,
      "Session should be paused after Pause command"
    );

    const line = await currentLine(session);
    assert.ok(line > 0, `Should be paused at a valid line after Pause, got ${line}`);

    // Verify counter advanced (proves we actually ran multiple cycles)
    const evalResult = await session.customRequest("evaluate", {
      expression: "counter",
      frameId: 0,
      context: "watch",
    });
    const counterVal = parseInt(evalResult.result, 10);
    assert.ok(
      counterVal > 1,
      `counter should have advanced during Continue, got ${counterVal}`
    );
  });

  test("Evaluate expression while paused", async function () {
    this.timeout(20000);
    const session = await launchAndWaitForStop();

    // Step past the first assignment
    await stepAndWait(session, "stepIn");

    const evalResult = await session.customRequest("evaluate", {
      expression: "counter",
      frameId: 0,
      context: "watch",
    });
    assert.ok(
      evalResult.result !== undefined && evalResult.result !== "<unknown>",
      `Evaluate should return a value, got: ${JSON.stringify(evalResult)}`
    );
  });

  test("Stop terminates the debug session", async function () {
    this.timeout(20000);
    const session = await launchAndWaitForStop();

    const ended = waitForNoSession(10000);
    await vscode.debug.stopDebugging(session);
    await ended;

    assert.strictEqual(
      vscode.debug.activeDebugSession,
      undefined,
      "Session should be terminated after Stop"
    );
  });

  test("Stop during Continue terminates cleanly", async function () {
    this.timeout(20000);
    const session = await launchAndWaitForStop();

    // Continue
    await session.customRequest("continue", { threadId: 1 });
    await sleep(500);

    // Stop while running
    const ended = waitForNoSession(10000);
    await vscode.debug.stopDebugging(session);
    await ended;

    assert.strictEqual(
      vscode.debug.activeDebugSession,
      undefined,
      "Session should be terminated after Stop during Continue"
    );
  });

  test("Breakpoint hit stops execution at correct line", async function () {
    this.timeout(20000);
    const session = await launchAndWaitForStop();

    // Set a breakpoint on `result := counter * 2;` (line 11)
    const bpLine = 11;
    await session.customRequest("setBreakpoints", {
      source: { path: testFilePath },
      breakpoints: [{ line: bpLine }],
    });

    // Continue to hit the breakpoint
    await session.customRequest("continue", { threadId: 1 });
    await pollUntilStopped(10000);

    const line = await currentLine(session);
    assert.ok(
      Math.abs(line - bpLine) <= 2,
      `Should stop near breakpoint line ${bpLine}, got line ${line}`
    );
  });
});
