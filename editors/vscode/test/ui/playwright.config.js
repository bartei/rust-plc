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
  },
  // The test file starts its own servers (Rust monitor + HTML fixture)
  // so we don't need a webServer config here.
  reporter: [["list"], ["html", { open: "never" }]],
});
