/**
 * YAML Schema E2E Test Runner.
 *
 * Boots a headless VS Code with the iec61131-st extension loaded AND the
 * required `redhat.vscode-yaml` extension pre-installed into the test
 * profile. Then runs the `schema.test.ts` suite, which exercises the
 * real YAML language server against the bundled schemas.
 *
 * Unlike the other E2E runners (which pass `--disable-extensions`),
 * this one needs `redhat.vscode-yaml` enabled because `yamlValidation`
 * contributions are read by that extension's language server.
 *
 * Usage:
 *   cd editors/vscode && npm run compile
 *   ST_E2E_SCHEMA=1 node ./out/test/runSchemaTest.js
 *
 *   # Visible window for debugging:
 *   ST_E2E_SCHEMA=1 ST_HEADED=1 node ./out/test/runSchemaTest.js
 */

import * as path from "path";
import { spawnSync } from "child_process";
import {
  runTests,
  downloadAndUnzipVSCode,
  resolveCliArgsFromVSCodeExecutablePath,
} from "@vscode/test-electron";

async function main() {
  try {
    const extensionDevelopmentPath = path.resolve(__dirname, "../../");
    const extensionTestsPath = path.resolve(__dirname, "./suite/index-schema");
    const testWorkspace = path.resolve(__dirname, "../../../playground");

    // Download (or reuse) the test VS Code build, then install the YAML
    // extension into its user-data dir BEFORE the runTests() launch.
    // Without this step the iec61131-st extension fails to load (its
    // `extensionDependencies` lists redhat.vscode-yaml).
    const vscodeExecutablePath = await downloadAndUnzipVSCode();
    const [cliPath, ...cliArgs] = resolveCliArgsFromVSCodeExecutablePath(
      vscodeExecutablePath,
    );

    console.log("Installing redhat.vscode-yaml into test VS Code profile...");
    const install = spawnSync(
      cliPath,
      [...cliArgs, "--install-extension", "redhat.vscode-yaml"],
      { encoding: "utf-8", stdio: "inherit" },
    );
    if (install.status !== 0) {
      throw new Error(
        `redhat.vscode-yaml install failed with exit code ${install.status}`,
      );
    }

    const env: Record<string, string> = {
      ...(process.env as Record<string, string>),
      ST_E2E_SCHEMA: process.env.ST_E2E_SCHEMA || "1",
    };

    // NOTE: no `--disable-extensions` here — we need the YAML extension
    // active for the `yamlValidation` contribution to take effect.
    const launchArgs = [testWorkspace];
    if (process.env.ST_HEADED !== "1") {
      launchArgs.push("--disable-gpu");
    }

    console.log("Starting YAML Schema E2E Tests...");
    console.log(`  Extension: ${extensionDevelopmentPath}`);
    console.log(`  Workspace: ${testWorkspace}`);

    await runTests({
      vscodeExecutablePath,
      extensionDevelopmentPath,
      extensionTestsPath,
      launchArgs,
      extensionTestsEnv: env,
    });

    console.log("YAML Schema E2E Tests PASSED");
  } catch (err) {
    console.error("YAML Schema E2E Tests FAILED:", err);
    process.exit(1);
  }
}

main();
