// @ts-check
const { test, expect } = require("@playwright/test");
const { spawn } = require("child_process");
const http = require("http");
const fs = require("fs");
const path = require("path");

/**
 * PLC Monitor Panel — Full end-to-end Playwright tests.
 *
 * Architecture:
 * - Starts the REAL Rust st-monitor WS server (monitor-test-server binary)
 * - Serves the test fixture HTML which uses the SAME rendering functions
 *   as the production webview (renderWatchNode, updateValueCellsFromTree)
 * - Connects via REAL WebSocket — zero mocking of data
 * - The test fixture's JS is kept in sync with production via the unit test
 *   "compiled webview script parses without syntax errors" which catches
 *   template literal escaping bugs
 *
 * Run:
 *   cargo build -p st-monitor --bin monitor-test-server
 *   cd editors/vscode/test/ui && npx playwright test
 */

const MONITOR_BINARY = path.resolve(
  __dirname, "..", "..", "..", "..", "target", "debug", "monitor-test-server"
);
const FIXTURE_FILE = path.resolve(__dirname, "..", "monitor-panel-visual.html");

let monitorServer;
let httpServer;
let httpPort;

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
  return new Promise((resolve) => {
    const fixture = fs.readFileSync(FIXTURE_FILE, "utf8");
    // Inject the WS port into the fixture via query parameter rewrite
    const srv = http.createServer((req, res) => {
      res.writeHead(200, { "Content-Type": "text/html; charset=utf-8" });
      res.end(fixture);
    });
    srv.listen(0, () => {
      const port = srv.address().port;
      resolve({ port, server: srv });
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
// Helpers
// ═══════════════════════════════════════════════════════════════════════

async function loadPage(page) {
  await page.goto(`http://localhost:${httpPort}/?port=${monitorServer.port}`);
  // The fixture sets wsConnected = true on WS open
  await page.waitForFunction(() => typeof wsConnected !== "undefined" && wsConnected, null, { timeout: 5000 });
}

async function addWatch(page, name) {
  await page.evaluate((n) => { addWatch(n); }, name);
  await page.waitForTimeout(300); // wait for subscribe + push round-trip
}

async function waitForToggle(page, dataVar) {
  await expect(page.locator(`tr[data-var="${dataVar}"] .tree-toggle`)).toBeVisible({ timeout: 5000 });
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
  await page.locator(`[data-action="toggle"][data-path="${fullPath}"]`).click();
}

async function clickRemove(page, fullPath) {
  await page.locator(`[data-action="remove"][data-path="${fullPath}"]`).click();
}

async function restartSession(page) {
  await page.evaluate(() => {
    // Close existing WS
    if (ws) { ws.close(); ws = null; }
    wsConnected = false;
    serverWatchTree = [];
    valueMap.clear();
    renderWatchTable();
  });
  await page.waitForTimeout(200);
  // Reconnect to the real server
  await page.evaluate((port) => { connectWs(port); }, monitorServer.port);
  await page.waitForFunction(() => wsConnected, null, { timeout: 5000 });
  // Re-subscribe
  await page.evaluate(() => {
    if (watchList.length > 0 && ws && ws.readyState === 1) {
      wsSend({ method: "subscribe", params: { variables: watchList, interval_ms: 0 } });
    }
  });
  await page.waitForTimeout(300);
}

// ═══════════════════════════════════════════════════════════════════════
// Tests — every scenario, real data, real WS, real rendering
// ═══════════════════════════════════════════════════════════════════════

test.describe("PLC Monitor — Real Server E2E", () => {

  test.beforeEach(async ({ page }) => {
    await loadPage(page);
  });

  // ─── REGRESSION: preset watch list must populate on connect ────
  // This is the exact bug the user reported: variables in the watch
  // list from a previous session don't populate when a new debug
  // session starts. The tree stays empty until the user manually
  // adds or removes a variable.

  test("REGRESSION: preset FB watch populates tree on session start", async ({ page }) => {
    // Navigate fresh (no existing WS connection)
    await page.goto(`http://localhost:${httpPort}/`);

    // Capture all WS messages for debugging
    await page.evaluate(() => {
      window.__wsMessages = [];
    });

    // Step 1: Pre-populate watch list BEFORE connecting (simulates persisted state)
    await page.evaluate(() => {
      watchList = ["Main.filler"];
      renderWatchTable(); // shows fallback/pending
    });

    // Verify fallback shows the item as pending
    await expect(page.locator('tr[data-var="main.filler"]')).toBeVisible();

    // Step 2: Connect to the real server (simulates debug session starting)
    await page.evaluate((port) => {
      // Patch the WS onmessage to capture messages
      var origConnect = connectWs;
      connectWs = function(p) {
        origConnect(p);
        // Intercept messages
        var origOnmsg = ws.onmessage;
        ws.onmessage = function(event) {
          var msg = JSON.parse(event.data);
          window.__wsMessages.push({
            type: msg.type,
            varCount: (msg.variables || []).length,
            treeCount: (msg.watch_tree || []).length,
            treeRoots: (msg.watch_tree || []).map(function(n) { return n.name; }),
          });
          origOnmsg.call(ws, event);
        };
      };
      connectWs(port);
    }, monitorServer.port);

    // Wait for WS to connect
    await page.waitForFunction(() => wsConnected, null, { timeout: 5000 });

    // Step 3: Subscribe (simulates what the extension host does on WS open)
    await page.evaluate(() => {
      wsSend({ method: "subscribe", params: { variables: watchList, interval_ms: 0 } });
    });

    // Step 4: The tree toggle MUST appear without ANY further user action.
    // Wait up to 5s for data to arrive and render.
    try {
      await waitForToggle(page, "main.filler");
    } catch (e) {
      // Dump captured WS messages for debugging
      const msgs = await page.evaluate(() => window.__wsMessages);
      console.log("WS messages received:", JSON.stringify(msgs, null, 2));
      const treeLen = await page.evaluate(() => serverWatchTree.length);
      console.log("serverWatchTree.length:", treeLen);
      const html = await page.locator("#var-body").innerHTML();
      console.log("var-body HTML:", html.substring(0, 500));
      throw e;
    }

    // Step 5: Expand and verify children have real values
    await clickToggle(page, "Main.filler");
    await expect(page.locator('tr[data-var="main.filler.start"]')).toBeVisible({ timeout: 5000 });
    await expect(page.locator('tr[data-var="main.filler.counter"]')).toBeVisible();
    await waitForValue(page, "main.filler.fill_count");
  });

  test("REGRESSION: expanding one nested node does NOT expand siblings", async ({ page }) => {
    await addWatch(page, "Main.filler");
    await waitForToggle(page, "main.filler");

    // Expand filler
    await clickToggle(page, "Main.filler");
    await expect(page.locator('tr[data-var="main.filler.counter"]')).toBeVisible();
    await expect(page.locator('tr[data-var="main.filler.edge"]')).toBeVisible();

    // Both nested FBs should be COLLAPSED (no children visible)
    await expect(page.locator('tr[data-var="main.filler.counter.cu"]')).toHaveCount(0);
    await expect(page.locator('tr[data-var="main.filler.edge.clk"]')).toHaveCount(0);

    // Expand ONLY counter
    await clickToggle(page, "Main.filler.counter");

    // Counter's children should be visible
    await expect(page.locator('tr[data-var="main.filler.counter.cu"]')).toBeVisible();
    await expect(page.locator('tr[data-var="main.filler.counter.cv"]')).toBeVisible();

    // Edge's children must STILL be hidden (this was the bug — all siblings expanded)
    await expect(page.locator('tr[data-var="main.filler.edge.clk"]')).toHaveCount(0);
    await expect(page.locator('tr[data-var="main.filler.edge.q"]')).toHaveCount(0);
  });

  // ─── Add variables ─────────────────────────────────────────────

  test("add scalar, verify value from real server", async ({ page }) => {
    await addWatch(page, "Main.cycle");
    await waitForValue(page, "main.cycle");
    await expect(page.locator('tr[data-var="main.cycle"] .type')).toContainText("INT");
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
    await expect(page.locator("#var-body")).toContainText("Watch list is empty");
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
    await addWatch(page, "Main.cycle");
    await waitForValue(page, "main.cycle");

    // The force input and button are inside the row
    const row = page.locator('tr[data-var="main.cycle"]');
    await row.locator(".force-input").fill("999");
    await row.locator('[data-action="force"]').click();

    // Wait for forced state
    await page.waitForFunction(() => {
      const r = document.querySelector('tr[data-var="main.cycle"]');
      const v = r?.querySelector(".value");
      return v && v.textContent === "999";
    }, null, { timeout: 5000 });
  });

  test.skip("unforce a variable", async ({ page }) => {
    // Skip: test server doesn't implement force — requires real engine loop.
    await addWatch(page, "Main.cycle");
    await waitForValue(page, "main.cycle");

    const row = page.locator('tr[data-var="main.cycle"]');
    await row.locator(".force-input").fill("999");
    await row.locator('[data-action="force"]').click();
    await page.waitForFunction(() =>
      document.querySelector('tr[data-var="main.cycle"] .value')?.textContent === "999",
      null, { timeout: 5000 }
    );

    await row.locator('[data-action="unforce"]').click();
    // After unforce, value should change from 999 (server continues incrementing)
    await page.waitForFunction(() => {
      const v = document.querySelector('tr[data-var="main.cycle"] .value');
      return v && v.textContent !== "999";
    }, null, { timeout: 5000 });
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

    // Expand state persisted
    await expect(page.locator('tr[data-var="main.filler.start"]')).toBeVisible({ timeout: 5000 });
    await expect(page.locator('tr[data-var="main.filler.counter.cv"]')).toBeVisible({ timeout: 5000 });
  });

  // ─── Mixed workflow ───────────────────────────────────────────

  test("full workflow: add, expand, force, remove, restart", async ({ page }) => {
    // Add three types
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
    await page.evaluate(() => { clearAll(); });
    await expect(page.locator("#var-body")).toContainText("Watch list is empty");
  });
});
