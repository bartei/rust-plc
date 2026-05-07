/**
 * Mocha entrypoint that loads ONLY the `schema.test.js` suite. The
 * runSchemaTest.ts runner pre-installs `redhat.vscode-yaml` into the
 * test VS Code's user profile so the yamlValidation contribution can
 * actually fire — but we don't want the rest of the suites running
 * with extensions enabled (they assume `--disable-extensions`).
 */
import * as path from "path";
import Mocha from "mocha";

export async function run(): Promise<void> {
  const mocha = new Mocha({ ui: "tdd", color: true, timeout: 60000 });
  const testsRoot = path.resolve(__dirname);
  mocha.addFile(path.join(testsRoot, "schema.test.js"));

  return new Promise((resolve, reject) => {
    mocha.run((failures) => {
      if (failures > 0) {
        reject(new Error(`${failures} tests failed.`));
      } else {
        resolve();
      }
    });
  });
}
