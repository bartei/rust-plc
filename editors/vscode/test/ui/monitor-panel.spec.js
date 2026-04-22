// @ts-check
const { test, expect } = require("@playwright/test");
const { spawn } = require("child_process");
const http = require("http");
const fs = require("fs");
const path = require("path");

/**
 * PLC Monitor Panel — Full end-to-end Playwright tests (Preact).
 *
 * Architecture:
 * - Starts the REAL Rust st-monitor WS server (monitor-test-server binary)
 * - Serves the production Preact bundle with a vscode-api-shim that
 *   bridges acquireVsCodeApi() → WebSocket → window.postMessage
 * - All interaction is via the DOM (type, click) — no internal function calls
 *
 * Run:
 *   cargo build -p st-monitor --bin monitor-test-server
 *   cd editors/vscode/test/ui && npx playwright test
 */

const MONITOR_BINARY = path.resolve(
  __dirname, "..", "..", "..", "..", "target", "debug", "monitor-test-server"
);

let monitorServer;
let httpServer;
let httpPort;

// ═══════════════════════════════════════════════════════════════════════
// Server setup
// ═══════════════════════════════════════════════════════════════════════

function startMonitorServer() {
  return new Promise((resolve, reject) => {
    const proc = spawn(MONITOR_BINARY, [], { stdio: ["pipe", "pipe", "pipe"] });
    let stdout = "", done = false;
    proc.stdout.on("data", (d) => {
      stdout += d.toString();
      if (!done) {
        const port = parseInt(stdout.trim(), 10);
        if (port > 0) { done = true; resolve({ port, kill: () => proc.kill("SIGTERM") }); }
      }
    });
    proc.on("error", (e) => { if (!done) reject(e); });
    setTimeout(() => { if (!done) { proc.kill(); reject(new Error("timeout")); } }, 10000);
  });
}

function startFixtureServer(wsPort) {
  const outDir = path.resolve(__dirname, "..", "..", "out", "webview");
  const htmlPath = path.join(outDir, "index.html");
  const cssPath = path.join(outDir, "styles.css");
  const jsPath = path.join(outDir, "monitor.js");
  const shimPath = path.join(__dirname, "vscode-api-shim.js");

  if (!fs.existsSync(htmlPath)) {
    throw new Error(`${htmlPath} not found. Run 'npm run build:webview' first.`);
  }

  let html = fs.readFileSync(htmlPath, "utf8");
  const css = fs.readFileSync(cssPath, "utf8");
  const shimJs = fs.readFileSync(shimPath, "utf8").replace(/__MONITOR_PORT__/g, String(wsPort));
  const bundleJs = fs.readFileSync(jsPath, "utf8");

  // Relax CSP for test
  html = html.replace(
    /content="[^"]*"/,
    `content="default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline' 'unsafe-eval' http: ; connect-src ws: ;"`
  );

  // Inline CSS
  html = html.replace(
    /<link rel="stylesheet" href="{{stylesUri}}"[^>]*>/,
    `<style>${css}</style>`
  );

  // Remove nonce
  html = html.replace(/nonce="{{nonce}}"/g, "");
  html = html.replace(/{{cspSource}}/g, "'unsafe-inline'");

  // Inject initial state
  html = html.replace(
    "{{initialState}}",
    JSON.stringify({ catalog: [], watchList: [], expandedNodes: [], version: "test" })
  );

  // Replace script src with paths served by our HTTP server
  html = html.replace(
    /<script[^>]*src="{{scriptUri}}"[^>]*><\/script>/,
    `<script src="/shim.js"></script>\n<script src="/monitor.js"></script>`
  );

  return new Promise((resolve) => {
    const srv = http.createServer((req, res) => {
      if (req.url === "/shim.js") {
        res.writeHead(200, { "Content-Type": "application/javascript; charset=utf-8" });
        res.end(shimJs);
      } else if (req.url === "/monitor.js") {
        res.writeHead(200, { "Content-Type": "application/javascript; charset=utf-8" });
        res.end(bundleJs);
      } else {
        res.writeHead(200, { "Content-Type": "text/html; charset=utf-8" });
        res.end(html);
      }
    });
    srv.listen(0, () => {
      resolve({ port: srv.address().port, server: srv });
    });
  });
}

test.beforeAll(async () => {
  monitorServer = await startMonitorServer();
  const http = await startFixtureServer(monitorServer.port);
  httpServer = http.server;
  httpPort = http.port;
  console.log(`Monitor WS: ${monitorServer.port}, HTTP: ${httpPort}`);
});

test.afterAll(async () => {
  if (httpServer) httpServer.close();
  if (monitorServer) monitorServer.kill();
});

// ═══════════════════════════════════════════════════════════════════════
// Helpers — all interaction through the DOM
// ═══════════════════════════════════════════════════════════════════════

async function loadPage(page) {
  await page.goto(`http://localhost:${httpPort}/`);
  // Wait for WS connection (shim sets __testWsConnected)
  await page.waitForFunction(() => __testWsConnected, null, { timeout: 5000 });
}

async function addWatch(page, name) {
  const input = page.locator(".add-row input");
  await input.fill(name);
  await page.locator(".add-row button:has-text('Add')").click();
  // Wait for subscribe + data push round-trip
  await page.waitForTimeout(500);
}

async function waitForToggle(page, dataVar) {
  await expect(
    page.locator(`tr[data-var="${dataVar}"] .tree-toggle`)
  ).toBeVisible({ timeout: 5000 });
}

async function waitForValue(page, dataVar) {
  await page.waitForFunction((dv) => {
    const row = document.querySelector('tr[data-var="' + dv + '"]');
    if (!row) return false;
    const v = row.querySelector(".value");
    return v && v.textContent && v.textContent.trim() !== "" && v.textContent !== "\u2026";
  }, dataVar, { timeout: 5000 });
}

async function clickToggle(page, fullPath) {
  await page.locator(`tr[data-var="${fullPath.toLowerCase()}"] .tree-toggle`).click();
  // Small wait for Preact to re-render
  await page.waitForTimeout(100);
}

async function clickRemove(page, fullPath) {
  // Find the remove button in the row for this path
  const row = page.locator(`tr[data-var="${fullPath.toLowerCase()}"]`);
  await row.locator("button:has-text('Remove')").click();
  await page.waitForTimeout(100);
}

async function clickClearAll(page) {
  await page.locator("button:has-text('Clear all')").click();
  await page.waitForTimeout(100);
}

async function restartSession(page) {
  // Read the current watch variables from captured addWatch messages
  const watchVars = await page.evaluate(() => {
    // Get all variables that were added via the shim
    return __capturedMessages
      .filter(function(m) { return m.command === "addWatch"; })
      .map(function(m) { return m.variable; });
  });

  // Close existing WS
  await page.evaluate(() => {
    if (__testWs) { __testWs.close(); __testWs = null; }
    __testWsConnected = false;
    window.postMessage({ command: "resetSession" }, "*");
  });
  await page.waitForTimeout(300);

  // Reconnect to a fresh monitor server
  await page.evaluate((port) => {
    __connectTestWs(port);
  }, monitorServer.port);
  await page.waitForFunction(() => __testWsConnected, null, { timeout: 5000 });

  // Re-subscribe the saved watch variables
  if (watchVars.length > 0) {
    await page.evaluate((vars) => {
      if (__testWs && __testWs.readyState === 1) {
        __testWs.send(JSON.stringify({
          method: "subscribe",
          params: { variables: vars, interval_ms: 0 }
        }));
      }
    }, watchVars);
  }
  await page.waitForTimeout(500);
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════

test.describe("PLC Monitor — Real Server E2E", () => {

  test.beforeEach(async ({ page }) => {
    await loadPage(page);
  });

  // ─── Add variables ─────────────────────────────────────────────

  test("add scalar, verify value from real server", async ({ page }) => {
    await addWatch(page, "Main.cycle");
    await waitForValue(page, "main.cycle");
    await expect(page.locator('tr[data-var="main.cycle"] .type')).toContainText("INT");
    // Scalar has no tree toggle
    await expect(page.locator('tr[data-var="main.cycle"] .tree-toggle')).toHaveCount(0);
  });

  test("add FB, verify tree toggle", async ({ page }) => {
    await addWatch(page, "Main.filler");
    await waitForToggle(page, "main.filler");
  });

  test("add array, verify tree toggle", async ({ page }) => {
    await addWatch(page, "Main.test_array");
    await waitForToggle(page, "main.test_array");
    await expect(page.locator('tr[data-var="main.test_array"] .type')).toContainText("ARRAY");
  });

  // ─── Open tree ─────────────────────────────────────────────────

  test("expand FB, verify children with values", async ({ page }) => {
    await addWatch(page, "Main.filler");
    await waitForToggle(page, "main.filler");
    await clickToggle(page, "Main.filler");

    await expect(page.locator('tr[data-var="main.filler.start"]')).toBeVisible();
    await expect(page.locator('tr[data-var="main.filler.fill_count"]')).toBeVisible();
    await expect(page.locator('tr[data-var="main.filler.counter"]')).toBeVisible();
    await expect(page.locator('tr[data-var="main.filler.edge"]')).toBeVisible();
    await waitForValue(page, "main.filler.fill_count");
  });

  // ─── Open subtree ──────────────────────────────────────────────

  test("expand nested FB (counter inside filler)", async ({ page }) => {
    await addWatch(page, "Main.filler");
    await waitForToggle(page, "main.filler");
    await clickToggle(page, "Main.filler");
    await clickToggle(page, "Main.filler.counter");

    await expect(page.locator('tr[data-var="main.filler.counter.cu"]')).toBeVisible();
    await expect(page.locator('tr[data-var="main.filler.counter.q"]')).toBeVisible();
    await expect(page.locator('tr[data-var="main.filler.counter.cv"]')).toBeVisible();
    await expect(page.locator('tr[data-var="main.filler.counter.pv"]')).toBeVisible();
    await waitForValue(page, "main.filler.counter.cv");
  });

  test("expand array, verify indexed elements", async ({ page }) => {
    await addWatch(page, "Main.test_array");
    await waitForToggle(page, "main.test_array");
    await clickToggle(page, "Main.test_array");

    for (let i = 0; i <= 9; i++) {
      await expect(page.locator(`tr[data-var="main.test_array[${i}]"]`)).toBeVisible();
    }
    await waitForValue(page, "main.test_array[0]");
    await expect(page.locator('tr[data-var="main.test_array[0]"] .type')).toContainText("INT");
  });

  // ─── REGRESSION: expanding one nested node does NOT expand siblings

  test("REGRESSION: expanding one nested node does NOT expand siblings", async ({ page }) => {
    await addWatch(page, "Main.filler");
    await waitForToggle(page, "main.filler");

    await clickToggle(page, "Main.filler");
    await expect(page.locator('tr[data-var="main.filler.counter"]')).toBeVisible();
    await expect(page.locator('tr[data-var="main.filler.edge"]')).toBeVisible();

    // Both nested FBs should be COLLAPSED
    await expect(page.locator('tr[data-var="main.filler.counter.cu"]')).toHaveCount(0);
    await expect(page.locator('tr[data-var="main.filler.edge.clk"]')).toHaveCount(0);

    // Expand ONLY counter
    await clickToggle(page, "Main.filler.counter");

    // Counter's children visible
    await expect(page.locator('tr[data-var="main.filler.counter.cu"]')).toBeVisible();
    await expect(page.locator('tr[data-var="main.filler.counter.cv"]')).toBeVisible();

    // Edge's children STILL hidden
    await expect(page.locator('tr[data-var="main.filler.edge.clk"]')).toHaveCount(0);
    await expect(page.locator('tr[data-var="main.filler.edge.q"]')).toHaveCount(0);
  });

  // ─── Collapse / re-expand ──────────────────────────────────────

  test("collapse and re-expand FB", async ({ page }) => {
    await addWatch(page, "Main.filler");
    await waitForToggle(page, "main.filler");
    await clickToggle(page, "Main.filler");
    await expect(page.locator('tr[data-var="main.filler.start"]')).toBeVisible();

    await clickToggle(page, "Main.filler"); // collapse
    await expect(page.locator('tr[data-var="main.filler.start"]')).toHaveCount(0);

    await clickToggle(page, "Main.filler"); // re-expand
    await expect(page.locator('tr[data-var="main.filler.start"]')).toBeVisible();
  });

  // ─── Delete variable ──────────────────────────────────────────

  test("remove FB from watch list", async ({ page }) => {
    await addWatch(page, "Main.filler");
    await addWatch(page, "Main.cycle");
    await waitForToggle(page, "main.filler");
    await clickRemove(page, "Main.filler");
    await expect(page.locator('tr[data-var="main.filler"]')).toHaveCount(0);
    await expect(page.locator('tr[data-var="main.cycle"]')).toBeVisible();
  });

  test("remove scalar from watch list", async ({ page }) => {
    await addWatch(page, "Main.cycle");
    await waitForValue(page, "main.cycle");
    await clickRemove(page, "Main.cycle");
    await expect(page.locator("#app")).toContainText("Watch list is empty");
  });

  test("remove array from watch list", async ({ page }) => {
    await addWatch(page, "Main.test_array");
    await waitForToggle(page, "main.test_array");
    await clickRemove(page, "Main.test_array");
    await expect(page.locator('tr[data-var="main.test_array"]')).toHaveCount(0);
  });

  // ─── Force / Unforce ──────────────────────────────────────────

  test.skip("force a scalar value", async ({ page }) => {
    // Skip: test server doesn't implement force — requires real engine loop.
  });

  test.skip("unforce a variable", async ({ page }) => {
    // Skip: test server doesn't implement force — requires real engine loop.
  });

  // ─── Value updates over multiple cycles ────────────────────────

  test("scalar values update over cycles", async ({ page }) => {
    await addWatch(page, "Main.cycle");
    await waitForValue(page, "main.cycle");
    const first = await page.locator('tr[data-var="main.cycle"] .value').textContent();
    await page.waitForFunction(
      (prev) => document.querySelector('tr[data-var="main.cycle"] .value')?.textContent !== prev,
      first, { timeout: 5000 }
    );
  });

  test("expanded tree values update over cycles", async ({ page }) => {
    await addWatch(page, "Main.filler");
    await waitForToggle(page, "main.filler");
    await clickToggle(page, "Main.filler");
    await clickToggle(page, "Main.filler.counter");
    await waitForValue(page, "main.filler.counter.cv");
    const first = await page.locator('tr[data-var="main.filler.counter.cv"] .value').textContent();
    await page.waitForFunction(
      (prev) => document.querySelector('tr[data-var="main.filler.counter.cv"] .value')?.textContent !== prev,
      first, { timeout: 5000 }
    );
  });

  test("array values update over cycles", async ({ page }) => {
    await addWatch(page, "Main.test_array");
    await waitForToggle(page, "main.test_array");
    await clickToggle(page, "Main.test_array");
    await waitForValue(page, "main.test_array[0]");
    const first = await page.locator('tr[data-var="main.test_array[0]"] .value').textContent();
    await page.waitForFunction(
      (prev) => document.querySelector('tr[data-var="main.test_array[0]"] .value')?.textContent !== prev,
      first, { timeout: 5000 }
    );
  });

  // ─── Session restart ──────────────────────────────────────────

  test("session restart: watch list persists and repopulates", async ({ page }) => {
    await addWatch(page, "Main.filler");
    await addWatch(page, "Main.cycle");
    await waitForToggle(page, "main.filler");
    await waitForValue(page, "main.cycle");

    await restartSession(page);

    await waitForToggle(page, "main.filler");
    await waitForValue(page, "main.cycle");
  });

  test("session restart: expanded trees re-expand", async ({ page }) => {
    await addWatch(page, "Main.filler");
    await waitForToggle(page, "main.filler");
    await clickToggle(page, "Main.filler");
    await clickToggle(page, "Main.filler.counter");
    await waitForValue(page, "main.filler.counter.cv");

    await restartSession(page);

    await expect(page.locator('tr[data-var="main.filler.start"]')).toBeVisible({ timeout: 5000 });
    await expect(page.locator('tr[data-var="main.filler.counter.cv"]')).toBeVisible({ timeout: 5000 });
  });

  // ─── Mixed workflow ───────────────────────────────────────────

  test("full workflow: add, expand, remove, restart", async ({ page }) => {
    await addWatch(page, "Main.filler");
    await addWatch(page, "Main.test_array");
    await addWatch(page, "Main.cycle");
    await waitForToggle(page, "main.filler");
    await waitForToggle(page, "main.test_array");
    await waitForValue(page, "main.cycle");

    // Expand FB + array
    await clickToggle(page, "Main.filler");
    await expect(page.locator('tr[data-var="main.filler.start"]')).toBeVisible();
    await clickToggle(page, "Main.test_array");
    await expect(page.locator('tr[data-var="main.test_array[0]"]')).toBeVisible();

    // Remove array
    await clickRemove(page, "Main.test_array");
    await expect(page.locator('tr[data-var="main.test_array"]')).toHaveCount(0);
    await expect(page.locator('tr[data-var="main.filler"]')).toBeVisible();

    // Session restart
    await restartSession(page);
    await waitForToggle(page, "main.filler");
    await waitForValue(page, "main.cycle");
  });

  // ─── Clear all ────────────────────────────────────────────────

  test("clear all removes everything", async ({ page }) => {
    await addWatch(page, "Main.filler");
    await addWatch(page, "Main.cycle");
    await waitForToggle(page, "main.filler");
    await clickClearAll(page);
    await expect(page.locator("#app")).toContainText("Watch list is empty");
  });

  // ─── Button clicks survive value updates ──────────────────────

  test("Force button opens dialog reliably during live updates", async ({ page }) => {
    await addWatch(page, "Main.cycle");
    await waitForValue(page, "main.cycle");

    // Click Force — dialog should open every time
    const forceBtn = page.locator('tr[data-var="main.cycle"] button:has-text("Force")');
    await forceBtn.click();

    // Dialog should be visible
    await expect(page.locator(".force-dialog")).toBeVisible({ timeout: 2000 });
    await expect(page.locator(".force-dialog-var")).toContainText("Main.cycle");

    // Close dialog
    await page.locator(".force-dialog button:has-text('Cancel')").click();
    await expect(page.locator(".force-dialog-overlay.visible")).toHaveCount(0);
  });
});
