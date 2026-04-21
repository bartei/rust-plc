/**
 * Unit tests for the PLC Monitor panel's WatchNode rendering logic.
 *
 * Run with: node editors/vscode/test/monitor-tree.test.js
 * (no dependencies required — uses Node's built-in assert)
 *
 * Tests the WatchNode tree structure that the backend sends
 * and the renderWatchNode function that renders it.
 */

const assert = require("assert");

// ============================================================================
// WatchNode structure helpers (mirror the Rust WatchNode struct)
// ============================================================================

function makeScalar(name, fullPath, type, value, forced) {
  return { name, fullPath, kind: "scalar", type, value, forced: !!forced, children: [] };
}

function makeFb(name, fullPath, type, children) {
  return { name, fullPath, kind: "fb", type, value: "", forced: false, children };
}

function makeArray(name, fullPath, type, children) {
  return { name, fullPath, kind: "array", type, value: "", forced: false, children };
}

// ============================================================================
// Simplified renderWatchNode for testing (same logic as production)
// ============================================================================

function encAttr(s) { return s.replace(/&/g,'&amp;').replace(/"/g,'&quot;').replace(/</g,'&lt;'); }

function renderWatchNode(node, depth, isRoot, expandedNodes) {
  let html = "";
  const fullLc = node.fullPath.toLowerCase();
  const hasChildren = node.children && node.children.length > 0;
  const isExpanded = expandedNodes.has(fullLc);

  if (hasChildren) {
    const toggle = isExpanded ? "\u25BE" : "\u25B8";
    const ea = encAttr(node.fullPath);
    html += `<tr data-var="${fullLc}"><td class="name"><span class="tree-toggle" data-action="toggle" data-path="${ea}">${toggle}</span> ${node.name}</td>` +
      `<td class="value">${node.value || ""}</td>` +
      `<td class="type">${node.type || ""}</td><td></td></tr>`;
    if (isExpanded) {
      for (const child of node.children) {
        html += renderWatchNode(child, depth + 1, false, expandedNodes);
      }
    }
  } else {
    html += `<tr data-var="${fullLc}"><td class="name">${node.name}</td>` +
      `<td class="value">${node.value || ""}</td>` +
      `<td class="type">${node.type || ""}</td><td></td></tr>`;
  }
  return html;
}

// ============================================================================
// Mock trees
// ============================================================================

function mockFillerTree() {
  return makeFb("Main.filler", "Main.filler", "FillController", [
    makeScalar("start", "Main.filler.start", "BOOL", "FALSE"),
    makeScalar("fill_count", "Main.filler.fill_count", "INT", "2"),
    makeFb("counter", "Main.filler.counter", "CTU", [
      makeScalar("CU", "Main.filler.counter.CU", "BOOL", "TRUE"),
      makeScalar("Q", "Main.filler.counter.Q", "BOOL", "FALSE"),
      makeScalar("CV", "Main.filler.counter.CV", "INT", "2"),
      makeScalar("PV", "Main.filler.counter.PV", "INT", "5"),
    ]),
    makeFb("edge", "Main.filler.edge", "R_TRIG", [
      makeScalar("CLK", "Main.filler.edge.CLK", "BOOL", "FALSE"),
      makeScalar("Q", "Main.filler.edge.Q", "BOOL", "FALSE"),
    ]),
  ]);
}

function mockArrayTree() {
  const elements = [];
  for (let i = 0; i < 10; i++) {
    elements.push(makeScalar(`[${i}]`, `Main.test_array[${i}]`, "INT", String(i * 10)));
  }
  return makeArray("Main.test_array", "Main.test_array", "ARRAY[0..9] OF INT", elements);
}

// ============================================================================
// Tests
// ============================================================================

let passed = 0;
let failed = 0;

function test(name, fn) {
  try {
    fn();
    passed++;
    console.log(`  \x1b[32m\u2713\x1b[0m ${name}`);
  } catch (e) {
    failed++;
    console.log(`  \x1b[31m\u2717\x1b[0m ${name}`);
    console.log(`    ${e.message}`);
  }
}

console.log("\nPLC Monitor WatchNode tree tests\n");

test("scalar node has no children", () => {
  const node = makeScalar("Main.cycle", "Main.cycle", "INT", "42");
  assert.strictEqual(node.children.length, 0);
});

test("FB node has children with fullPath", () => {
  const node = mockFillerTree();
  assert.ok(node.children.length > 0);
  const start = node.children.find(c => c.name === "start");
  assert.ok(start);
  assert.strictEqual(start.fullPath, "Main.filler.start");
});

test("nested FB has correct fullPaths", () => {
  const node = mockFillerTree();
  const counter = node.children.find(c => c.name === "counter");
  assert.strictEqual(counter.fullPath, "Main.filler.counter");
  const cv = counter.children.find(c => c.name === "CV");
  assert.strictEqual(cv.fullPath, "Main.filler.counter.CV");
});

test("array has indexed children with bracket fullPaths", () => {
  const node = mockArrayTree();
  assert.strictEqual(node.children.length, 10);
  assert.strictEqual(node.children[0].fullPath, "Main.test_array[0]");
  assert.strictEqual(node.children[9].fullPath, "Main.test_array[9]");
});

test("collapsed FB renders only parent row", () => {
  const html = renderWatchNode(mockFillerTree(), 0, true, new Set());
  assert.strictEqual((html.match(/<tr /g) || []).length, 1);
});

test("expanded FB renders parent + children (not nested)", () => {
  const html = renderWatchNode(mockFillerTree(), 0, true, new Set(["main.filler"]));
  assert.ok((html.match(/<tr /g) || []).length > 3);
  assert.ok(!html.includes("main.filler.counter.cv")); // counter not expanded
});

test("expanding counter shows 3rd level", () => {
  const html = renderWatchNode(mockFillerTree(), 0, true, new Set(["main.filler", "main.filler.counter"]));
  assert.ok(html.includes("main.filler.counter.cv"));
});

test("expanding counter does NOT expand edge siblings", () => {
  const html = renderWatchNode(mockFillerTree(), 0, true, new Set(["main.filler", "main.filler.counter"]));
  assert.ok(html.includes("main.filler.counter.cv")); // counter expanded
  assert.ok(!html.includes("main.filler.edge.clk")); // edge NOT expanded
});

test("expanded array renders all elements", () => {
  const html = renderWatchNode(mockArrayTree(), 0, true, new Set(["main.test_array"]));
  assert.strictEqual((html.match(/<tr /g) || []).length, 11); // 1 parent + 10 elements
});

test("data-var uses lowercased fullPath", () => {
  const html = renderWatchNode(mockArrayTree(), 0, true, new Set(["main.test_array"]));
  assert.ok(html.includes('data-var="main.test_array[0]"'));
  assert.ok(html.includes('data-var="main.test_array[9]"'));
});

// ============================================================================
// Production bundle validation
// ============================================================================

console.log("\nProduction bundle validation\n");

test("esbuild output exists", () => {
  const fs = require("fs");
  const path = require("path");
  const jsPath = path.resolve(__dirname, "..", "out", "webview", "monitor.js");
  assert.ok(fs.existsSync(jsPath), `${jsPath} should exist — run 'npm run build:webview'`);
});

test("production HTML has no template interpolation leftovers", () => {
  const fs = require("fs");
  const path = require("path");
  const htmlPath = path.resolve(__dirname, "..", "out", "webview", "index.html");
  if (!fs.existsSync(htmlPath)) return; // skip if not built
  const html = fs.readFileSync(htmlPath, "utf8");
  // The HTML should have {{placeholders}} that the extension host replaces
  assert.ok(html.includes("{{scriptUri}}"), "Should have scriptUri placeholder");
  assert.ok(html.includes("{{stylesUri}}"), "Should have stylesUri placeholder");
  assert.ok(html.includes("{{initialState}}"), "Should have initialState placeholder");
  // Should NOT have any JavaScript in the HTML (it's all in monitor.js)
  assert.ok(!html.includes("function renderWatchNode"), "No inline JS");
});

// Summary
console.log(`\n${passed} passed, ${failed} failed\n`);
process.exit(failed > 0 ? 1 : 0);
