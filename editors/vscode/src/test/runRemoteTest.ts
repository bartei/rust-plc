/**
 * Remote Debug E2E Test Runner.
 *
 * Launches VS Code with the extension loaded and runs the remote debug test
 * suite against a local or QEMU-hosted target agent.
 *
 * Usage:
 *   # Run against local agent (auto-started):
 *   ST_E2E_REMOTE=1 node out/test/runRemoteTest.js
 *
 *   # Run against QEMU target (must be running):
 *   ST_E2E_REMOTE=qemu ST_AGENT_HOST=127.0.0.1 ST_AGENT_PORT=4840 node out/test/runRemoteTest.js
 *
 *   # Run headed (visible VS Code window):
 *   ST_E2E_REMOTE=1 ST_HEADED=1 node out/test/runRemoteTest.js
 *
 * Prerequisites:
 *   cargo build -p st-cli -p st-target-agent
 *   cd editors/vscode && npm run compile
 */

import * as path from "path";
import { runTests } from "@vscode/test-electron";

async function main() {
  try {
    const extensionDevelopmentPath = path.resolve(__dirname, "../../");
    const extensionTestsPath = path.resolve(__dirname, "./suite/index");
    const testWorkspace = path.resolve(__dirname, "../../../playground");

    const env: Record<string, string> = {
      ...process.env as Record<string, string>,
      // Only run the remote debug test file
      ST_E2E_REMOTE: process.env.ST_E2E_REMOTE || "1",
    };

    // Forward agent connection settings
    if (process.env.ST_AGENT_HOST) env.ST_AGENT_HOST = process.env.ST_AGENT_HOST;
    if (process.env.ST_AGENT_PORT) env.ST_AGENT_PORT = process.env.ST_AGENT_PORT;
    if (process.env.ST_DAP_PORT) env.ST_DAP_PORT = process.env.ST_DAP_PORT;

    const launchArgs = [testWorkspace, "--disable-extensions"];

    // For headed mode, don't pass --disable-gpu (allows visual inspection)
    if (process.env.ST_HEADED !== "1") {
      launchArgs.push("--disable-gpu");
    }

    console.log("Starting Remote Debug E2E Tests...");
    console.log(`  Extension: ${extensionDevelopmentPath}`);
    console.log(`  Workspace: ${testWorkspace}`);
    console.log(`  Mode: ${process.env.ST_E2E_REMOTE || "local"}`);
    if (process.env.ST_AGENT_HOST) {
      console.log(`  Agent: ${process.env.ST_AGENT_HOST}:${process.env.ST_AGENT_PORT || "4840"}`);
    }

    await runTests({
      extensionDevelopmentPath,
      extensionTestsPath,
      launchArgs,
      extensionTestsEnv: env,
    });

    console.log("Remote Debug E2E Tests PASSED");
  } catch (err) {
    console.error("Remote Debug E2E Tests FAILED:", err);
    process.exit(1);
  }
}

main();
