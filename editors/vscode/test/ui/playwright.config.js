// @ts-check
const { defineConfig } = require("@playwright/test");
const path = require("path");

module.exports = defineConfig({
  testDir: ".",
  testMatch: "*.spec.js",
  timeout: 30000,
  retries: 0,
  use: {
    // Use the visual test fixture HTML as the base page
    baseURL: `file://${path.resolve(__dirname, "..", "monitor-panel-visual.html")}`,
    headless: true,
    screenshot: "only-on-failure",
  },
  reporter: [["list"], ["html", { open: "never" }]],
});
