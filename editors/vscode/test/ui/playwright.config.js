// @ts-check
const { defineConfig } = require("@playwright/test");
const path = require("path");

const fixtureFile = path.resolve(__dirname, "..", "monitor-panel-visual.html");

module.exports = defineConfig({
  testDir: ".",
  testMatch: "*.spec.js",
  timeout: 30000,
  retries: 0,
  use: {
    headless: true,
    screenshot: "only-on-failure",
    // On NixOS, Playwright's downloaded Chromium won't run (dynamically linked).
    // Use the system Chrome/Chromium instead via the "channel" option.
    ...(process.env.PLAYWRIGHT_CHANNEL
      ? { channel: process.env.PLAYWRIGHT_CHANNEL }
      : {}),
    ...(process.env.PLAYWRIGHT_EXECUTABLE_PATH
      ? { launchOptions: { executablePath: process.env.PLAYWRIGHT_EXECUTABLE_PATH } }
      : {}),
  },
  // The test file starts its own servers (Rust monitor + HTML fixture)
  // so we don't need a webServer config here.
  reporter: [["list"], ["html", { open: "never" }]],
});
