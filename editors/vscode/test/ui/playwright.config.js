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
    baseURL: "http://localhost:9347",
    headless: true,
    screenshot: "only-on-failure",
  },
  // Serve the test fixture via a local HTTP server so headless browsers
  // don't block file:// URLs.
  webServer: {
    command: `node -e "const fs=require('fs'),http=require('http');http.createServer((q,r)=>{r.writeHead(200,{'Content-Type':'text/html; charset=utf-8'});fs.createReadStream('${fixtureFile.replace(/\\/g, "\\\\")}').pipe(r)}).listen(9347,()=>console.log('listening on 9347'))"`,
    url: "http://localhost:9347",
    reuseExistingServer: false,
  },
  reporter: [["list"], ["html", { open: "never" }]],
});
