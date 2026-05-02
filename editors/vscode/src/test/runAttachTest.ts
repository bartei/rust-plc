/**
 * Non-intrusive Debug Attach E2E Test Runner.
 *
 * Boots a headless VS Code with the extension loaded and runs ONLY the
 * `attach-running.test.ts` suite. The suite spawns its own private
 * `st-target-agent` instance (port 4860) so it doesn't collide with
 * other electron suites that use the default 4840.
 *
 * Usage:
 *   cargo build -p st-cli -p st-target-agent
 *   cd editors/vscode && npm run compile
 *   ST_E2E_ATTACH=1 node ./out/test/runAttachTest.js
 *
 *   # Visible window for debugging:
 *   ST_E2E_ATTACH=1 ST_HEADED=1 node ./out/test/runAttachTest.js
 */

import * as path from "path";
import { runTests } from "@vscode/test-electron";

async function main() {
  try {
    const extensionDevelopmentPath = path.resolve(__dirname, "../../");
    const extensionTestsPath = path.resolve(__dirname, "./suite/index-attach");
    const testWorkspace = path.resolve(__dirname, "../../../playground");

    const env: Record<string, string> = {
      ...process.env as Record<string, string>,
      ST_E2E_ATTACH: process.env.ST_E2E_ATTACH || "1",
    };

    const launchArgs = [testWorkspace, "--disable-extensions"];
    if (process.env.ST_HEADED !== "1") {
      launchArgs.push("--disable-gpu");
    }

    console.log("Starting Non-intrusive Attach E2E Tests...");
    await runTests({
      extensionDevelopmentPath,
      extensionTestsPath,
      launchArgs,
      extensionTestsEnv: env,
    });
    console.log("Non-intrusive Attach E2E Tests PASSED");
  } catch (err) {
    console.error("Non-intrusive Attach E2E Tests FAILED:", err);
    process.exit(1);
  }
}

main();
