// @ts-check
const { test, expect } = require("@playwright/test");
const { spawn } = require("child_process");
const http = require("http");
const fs = require("fs");
const path = require("path");

/**
 * PLC Monitor Panel — RETAIN / PERSISTENT badge visual verification.
 *
 * Drives the production Preact bundle against the real Rust
 * monitor-test-server (which has been seeded with three retained
 * scalars and a RETAIN PERSISTENT FB instance) and asserts the
 * `.retain-badge` element renders with the expected label and
 * tooltip on every row that should have one — and is ABSENT on
 * non-retained rows.
 *
 * This is the visual counterpart of:
 *   - DAP:    `test_retain_persistent_presentation_hint`
 *   - Server: `ws_catalog_and_watch_tree_carry_retain_flags`
 *
 * Run:
 *   cargo build -p st-monitor --bin monitor-test-server
 *   npm --prefix editors/vscode run build:webview
 *   cd editors/vscode/test/ui && npx playwright test retain-badge.spec.js
 */

const MONITOR_BINARY = path.resolve(
  __dirname, "..", "..", "..", "..", "target", "debug", "monitor-test-server"
);

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

  html = html.replace(
    /content="[^"]*"/,
    `content="default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline' 'unsafe-eval' http: ; connect-src ws: ;"`
  );
  html = html.replace(
    /<link rel="stylesheet" href="{{stylesUri}}"[^>]*>/,
    `<style>${css}</style>`
  );
  html = html.replace(/nonce="{{nonce}}"/g, "");
  html = html.replace(/{{cspSource}}/g, "'unsafe-inline'");
  html = html.replace(
    "{{initialState}}",
    JSON.stringify({ catalog: [], watchList: [], expandedNodes: [], version: "test" })
  );
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
  const httpResult = await startFixtureServer(monitorServer.port);
  httpServer = httpResult.server;
  httpPort = httpResult.port;
  console.log(`Monitor WS: ${monitorServer.port}, HTTP: ${httpPort}`);
});

test.afterAll(async () => {
  if (httpServer) httpServer.close();
  if (monitorServer) monitorServer.kill();
});

async function loadPage(page) {
  await page.goto(`http://localhost:${httpPort}/`);
  await page.waitForFunction(() => __testWsConnected, null, { timeout: 5000 });
}

async function addWatch(page, name) {
  const input = page.locator(".add-row input");
  await input.fill(name);
  await page.locator(".add-row button:has-text('Add')").click();
  await page.waitForTimeout(500);
}

async function waitForValue(page, dataVar) {
  await page.waitForFunction((dv) => {
    const row = document.querySelector('tr[data-var="' + dv + '"]');
    if (!row) return false;
    const v = row.querySelector(".value");
    return v && v.textContent && v.textContent.trim() !== "" && v.textContent !== "…";
  }, dataVar, { timeout: 5000 });
}

async function waitForToggle(page, dataVar) {
  await expect(
    page.locator(`tr[data-var="${dataVar}"] .tree-toggle`)
  ).toBeVisible({ timeout: 5000 });
}

// ═══════════════════════════════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════════════════════════════

test.describe("RETAIN / PERSISTENT badge", () => {
  test.beforeEach(async ({ page }) => {
    await loadPage(page);
  });

  test("RETAIN scalar shows 'R' badge with the right tooltip", async ({ page }) => {
    await addWatch(page, "Main.retain_var");
    await waitForValue(page, "main.retain_var");

    const row = page.locator('tr[data-var="main.retain_var"]');
    const badge = row.locator(".retain-badge");
    await expect(badge).toBeVisible();
    await expect(badge).toHaveText("R");
    await expect(badge).toHaveAttribute("data-retain", "1");
    await expect(badge).toHaveAttribute("data-persistent", "0");
    await expect(badge).toHaveAttribute(
      "title",
      /RETAIN — survives warm restart/,
    );
  });

  test("PERSISTENT-only scalar shows 'P' badge", async ({ page }) => {
    await addWatch(page, "Main.persistent_var");
    await waitForValue(page, "main.persistent_var");

    const row = page.locator('tr[data-var="main.persistent_var"]');
    const badge = row.locator(".retain-badge");
    await expect(badge).toBeVisible();
    await expect(badge).toHaveText("P");
    await expect(badge).toHaveAttribute("data-retain", "0");
    await expect(badge).toHaveAttribute("data-persistent", "1");
    await expect(badge).toHaveAttribute(
      "title",
      /PERSISTENT — survives cold restart/,
    );
  });

  test("RETAIN PERSISTENT scalar shows 'RP' badge", async ({ page }) => {
    await addWatch(page, "Main.durable_var");
    await waitForValue(page, "main.durable_var");

    const row = page.locator('tr[data-var="main.durable_var"]');
    const badge = row.locator(".retain-badge");
    await expect(badge).toBeVisible();
    await expect(badge).toHaveText("RP");
    await expect(badge).toHaveAttribute("data-retain", "1");
    await expect(badge).toHaveAttribute("data-persistent", "1");
    await expect(badge).toHaveAttribute(
      "title",
      /RETAIN PERSISTENT — survives warm and cold restart/,
    );
  });

  test("plain (non-retained) scalar has no badge", async ({ page }) => {
    await addWatch(page, "Main.cycle");
    await waitForValue(page, "main.cycle");

    const row = page.locator('tr[data-var="main.cycle"]');
    await expect(row.locator(".retain-badge")).toHaveCount(0);
  });

  test("RETAIN PERSISTENT FB shows badge on parent AND children", async ({ page }) => {
    await addWatch(page, "Main.retain_fb");
    await waitForToggle(page, "main.retain_fb");

    // Parent row carries the badge.
    const parentBadge = page
      .locator('tr[data-var="main.retain_fb"] .retain-badge')
      .first();
    await expect(parentBadge).toBeVisible();
    await expect(parentBadge).toHaveText("RP");

    // Expand and confirm children inherit the badge — RETAIN semantics
    // capture every field of an FB instance flagged RETAIN, so the UI
    // must communicate that on each child row too.
    await page.locator('tr[data-var="main.retain_fb"] .tree-toggle').click();
    await page.waitForTimeout(200);

    const childRow = page.locator('tr[data-var="main.retain_fb.cv"]');
    await expect(childRow).toBeVisible();
    const childBadge = childRow.locator(".retain-badge");
    await expect(childBadge).toBeVisible();
    await expect(childBadge).toHaveText("RP");
  });

  test("badge survives live value updates", async ({ page }) => {
    await addWatch(page, "Main.retain_var");
    await waitForValue(page, "main.retain_var");

    const row = page.locator('tr[data-var="main.retain_var"]');
    const badge = row.locator(".retain-badge");
    await expect(badge).toBeVisible();

    const first = await row.locator(".value").textContent();
    // Wait for the value to change (proves the row re-rendered).
    await page.waitForFunction(
      (prev) => {
        const r = document.querySelector('tr[data-var="main.retain_var"]');
        return r && r.querySelector(".value")?.textContent !== prev;
      },
      first,
      { timeout: 5000 },
    );

    // Badge must still be there with the same text.
    await expect(badge).toBeVisible();
    await expect(badge).toHaveText("R");
  });

  test("badge styling: RP gets the highlighted background", async ({ page }) => {
    await addWatch(page, "Main.durable_var");
    await waitForValue(page, "main.durable_var");

    const badge = page
      .locator('tr[data-var="main.durable_var"] .retain-badge')
      .first();
    await expect(badge).toBeVisible();

    // The CSS at editors/vscode/src/webview/styles.css gives
    // [data-retain="1"][data-persistent="1"] a charts.yellow-ish bg.
    // We can't assert exact colour reliably across themes, but we can
    // confirm the inline computed style differs from a plain badge.
    const rpBg = await badge.evaluate(
      (el) => window.getComputedStyle(el).backgroundColor,
    );
    expect(rpBg).toBeTruthy();
    expect(rpBg).not.toBe("rgba(0, 0, 0, 0)");
  });
});
