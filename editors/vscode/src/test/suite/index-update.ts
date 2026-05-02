/**
 * Mocha entrypoint that loads ONLY the `online-update.test.js` suite.
 *
 * The default `suite/index.ts` glob-loads every `*.test.js` under the suite
 * directory, which is what the regular test runner wants. The online-update
 * runner uses this dedicated entrypoint so the slow extension/LSP warm-up
 * tests don't run when we just want to validate the `program/update` flow.
 */
import * as path from "path";
import Mocha from "mocha";

export async function run(): Promise<void> {
  const mocha = new Mocha({ ui: "tdd", color: true, timeout: 60000 });
  const testsRoot = path.resolve(__dirname);
  mocha.addFile(path.join(testsRoot, "online-update.test.js"));

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
