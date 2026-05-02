/**
 * Mocha entrypoint that loads ONLY the `attach-running.test.js` suite.
 *
 * Mirrors the pattern used by `index-update.ts`: gives the attach-running
 * runner a clean slate so the slower extension-activation / monitor-panel
 * tests don't run alongside it. Useful when iterating on the
 * non-intrusive-attach feature.
 */
import * as path from "path";
import Mocha from "mocha";

export async function run(): Promise<void> {
  const mocha = new Mocha({ ui: "tdd", color: true, timeout: 90000 });
  const testsRoot = path.resolve(__dirname);
  mocha.addFile(path.join(testsRoot, "attach-running.test.js"));

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
