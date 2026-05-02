/**
 * Online Update E2E Test Runner.
 *
 * Launches a headless VS Code with the extension loaded and runs the
 * `online-update.test.ts` suite against a local target agent process.
 *
 * Usage:
 *   # Build prerequisites first:
 *   cargo build -p st-cli -p st-target-agent
 *   cd editors/vscode && npm run compile
 *
 *   # Run against an auto-spawned local agent:
 *   ST_E2E_UPDATE=1 node out/test/runUpdateTest.js
 *
 *   # Run against an already-running QEMU target:
 *   ST_E2E_UPDATE=1 ST_AGENT_HOST=127.0.0.1 ST_AGENT_PORT=4840 \
 *       node out/test/runUpdateTest.js
 *
 *   # Visible window for debugging the headless run:
 *   ST_E2E_UPDATE=1 ST_HEADED=1 node out/test/runUpdateTest.js
 */

import * as path from "path";
import { runTests } from "@vscode/test-electron";

async function main() {
  try {
    const extensionDevelopmentPath = path.resolve(__dirname, "../../");
    const extensionTestsPath = path.resolve(__dirname, "./suite/index-update");
    const testWorkspace = path.resolve(__dirname, "../../../playground");

    const env: Record<string, string> = {
      ...process.env as Record<string, string>,
      ST_E2E_UPDATE: process.env.ST_E2E_UPDATE || "1",
    };
    if (process.env.ST_AGENT_HOST) env.ST_AGENT_HOST = process.env.ST_AGENT_HOST;
    if (process.env.ST_AGENT_PORT) env.ST_AGENT_PORT = process.env.ST_AGENT_PORT;

    const launchArgs = [testWorkspace, "--disable-extensions"];
    if (process.env.ST_HEADED !== "1") {
      launchArgs.push("--disable-gpu");
    }

    console.log("Starting Online Update E2E Tests...");
    console.log(`  Extension: ${extensionDevelopmentPath}`);
    console.log(`  Workspace: ${testWorkspace}`);
    if (process.env.ST_AGENT_HOST) {
      console.log(`  Agent (external): ${process.env.ST_AGENT_HOST}:${process.env.ST_AGENT_PORT || "4855"}`);
    }

    await runTests({
      extensionDevelopmentPath,
      extensionTestsPath,
      launchArgs,
      extensionTestsEnv: env,
    });

    console.log("Online Update E2E Tests PASSED");
  } catch (err) {
    console.error("Online Update E2E Tests FAILED:", err);
    process.exit(1);
  }
}

main();
