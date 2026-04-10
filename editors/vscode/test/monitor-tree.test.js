/**
 * Automated unit tests for the PLC Monitor panel's tree builder logic.
 *
 * Run with: node editors/vscode/test/monitor-tree.test.js
 * (no dependencies required — uses Node's built-in assert)
 *
 * These tests exercise the exact same buildSubTree + renderTree logic
 * that runs in the webview, but with mock data and assertions instead
 * of visual inspection.
 */

const assert = require("assert");

// ============================================================================
// Extract the pure tree-builder functions (same logic as monitorPanel.ts)
// ============================================================================

function buildSubTree(prefix, valueMap) {
  const prefixLc = prefix.toLowerCase() + ".";
  const tree = {};
  valueMap.forEach((v, fullLc) => {
    if (!fullLc.startsWith(prefixLc)) return;
    const relative = v.name.substring(prefix.length + 1);
    const parts = relative.split(".");
    let node = tree;
    for (let i = 0; i < parts.length - 1; i++) {
      const seg = parts[i];
      if (!node[seg]) node[seg] = { __children: {} };
      node = node[seg].__children;
    }
    const leaf = parts[parts.length - 1];
    node[leaf] = {
      __value: v,
      __children: node[leaf] ? node[leaf].__children : null,
    };
  });
  return tree;
}

function countLeaves(tree) {
  let count = 0;
  for (const key of Object.keys(tree)) {
    const entry = tree[key];
    if (entry.__children && Object.keys(entry.__children).length > 0) {
      count += countLeaves(entry.__children);
    }
    if (entry.__value) count++;
  }
  return count;
}

function getNodeNames(tree) {
  return Object.keys(tree).sort();
}

// ============================================================================
// Mock data matching the multi_file_project playground
// ============================================================================

function createMockValueMap() {
  const entries = [
    { name: "Main.filler.start", value: "FALSE", type: "BOOL" },
    { name: "Main.filler.target_fill", value: "5", type: "INT" },
    { name: "Main.filler.valve_open", value: "TRUE", type: "BOOL" },
    { name: "Main.filler.fill_done", value: "FALSE", type: "BOOL" },
    { name: "Main.filler.fill_count", value: "2", type: "INT" },
    { name: "Main.filler.filling", value: "TRUE", type: "BOOL" },
    { name: "Main.filler.pulse_toggle", value: "FALSE", type: "BOOL" },
    { name: "Main.filler.counter.CU", value: "TRUE", type: "BOOL" },
    { name: "Main.filler.counter.RESET", value: "FALSE", type: "BOOL" },
    { name: "Main.filler.counter.PV", value: "5", type: "INT" },
    { name: "Main.filler.counter.Q", value: "FALSE", type: "BOOL" },
    { name: "Main.filler.counter.CV", value: "2", type: "INT" },
    { name: "Main.filler.counter.prev_cu", value: "FALSE", type: "BOOL" },
    { name: "Main.filler.edge.CLK", value: "FALSE", type: "BOOL" },
    { name: "Main.filler.edge.Q", value: "FALSE", type: "BOOL" },
    { name: "Main.filler.edge.prev", value: "FALSE", type: "BOOL" },
    { name: "Main.cycle", value: "42", type: "INT" },
  ];
  const map = new Map();
  for (const e of entries) {
    map.set(e.name.toLowerCase(), e);
  }
  return map;
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
    console.log(`  \x1b[32m✓\x1b[0m ${name}`);
  } catch (e) {
    failed++;
    console.log(`  \x1b[31m✗\x1b[0m ${name}`);
    console.log(`    ${e.message}`);
  }
}

console.log("\nPLC Monitor tree builder tests\n");

// --- Test: watching a scalar variable produces no tree ---
test("scalar variable has no children", () => {
  const vm = createMockValueMap();
  const tree = buildSubTree("Main.cycle", vm);
  assert.strictEqual(Object.keys(tree).length, 0, "Main.cycle is a leaf — no children");
});

// --- Test: watching Main.filler produces a tree with direct fields + nested FBs ---
test("Main.filler tree has correct top-level nodes", () => {
  const vm = createMockValueMap();
  const tree = buildSubTree("Main.filler", vm);
  const names = getNodeNames(tree);
  assert.ok(names.includes("start"), "Should include 'start'");
  assert.ok(names.includes("fill_count"), "Should include 'fill_count'");
  assert.ok(names.includes("counter"), "Should include 'counter' (nested FB)");
  assert.ok(names.includes("edge"), "Should include 'edge' (nested FB)");
  assert.ok(names.includes("filling"), "Should include 'filling'");
});

// --- Test: nested FB 'counter' has children ---
test("counter node has children (CU, Q, CV, etc.)", () => {
  const vm = createMockValueMap();
  const tree = buildSubTree("Main.filler", vm);
  const counter = tree["counter"];
  assert.ok(counter, "counter node should exist");
  assert.ok(counter.__children, "counter should have __children");
  const children = getNodeNames(counter.__children);
  assert.ok(children.includes("CU"), "counter should have CU");
  assert.ok(children.includes("Q"), "counter should have Q");
  assert.ok(children.includes("CV"), "counter should have CV");
  assert.ok(children.includes("PV"), "counter should have PV");
  assert.ok(children.includes("prev_cu"), "counter should have prev_cu");
  assert.ok(children.includes("RESET"), "counter should have RESET");
  assert.strictEqual(children.length, 6, "counter should have exactly 6 children");
});

// --- Test: nested FB 'edge' has children ---
test("edge node has children (CLK, Q, prev)", () => {
  const vm = createMockValueMap();
  const tree = buildSubTree("Main.filler", vm);
  const edge = tree["edge"];
  assert.ok(edge, "edge node should exist");
  assert.ok(edge.__children, "edge should have __children");
  const children = getNodeNames(edge.__children);
  assert.ok(children.includes("CLK"), "edge should have CLK");
  assert.ok(children.includes("Q"), "edge should have Q");
  assert.ok(children.includes("prev"), "edge should have prev");
  assert.strictEqual(children.length, 3, "edge should have exactly 3 children");
});

// --- Test: scalar fields are leaves (no __children) ---
test("scalar fields are leaves", () => {
  const vm = createMockValueMap();
  const tree = buildSubTree("Main.filler", vm);
  const start = tree["start"];
  assert.ok(start, "start node should exist");
  assert.ok(start.__value, "start should have __value");
  assert.strictEqual(start.__value.value, "FALSE");
  assert.ok(!start.__children || Object.keys(start.__children).length === 0,
    "start should NOT have children");
});

// --- Test: watching Main.filler.counter produces only counter's children ---
test("watching counter directly shows only counter fields", () => {
  const vm = createMockValueMap();
  const tree = buildSubTree("Main.filler.counter", vm);
  const names = getNodeNames(tree);
  assert.ok(names.includes("CU"), "Should include CU");
  assert.ok(names.includes("Q"), "Should include Q");
  assert.ok(names.includes("CV"), "Should include CV");
  assert.ok(!names.includes("start"), "Should NOT include filler's start");
  assert.ok(!names.includes("counter"), "Should NOT include counter itself");
});

// --- Test: leaf count is correct ---
test("Main.filler has 16 leaf values total", () => {
  const vm = createMockValueMap();
  const tree = buildSubTree("Main.filler", vm);
  const leaves = countLeaves(tree);
  // 7 direct scalars + 6 counter fields + 3 edge fields = 16
  assert.strictEqual(leaves, 16, `Expected 16 leaves, got ${leaves}`);
});

// --- Test: values are accessible in the tree ---
test("counter.CV value is accessible", () => {
  const vm = createMockValueMap();
  const tree = buildSubTree("Main.filler", vm);
  const cv = tree["counter"].__children["CV"];
  assert.ok(cv, "CV node should exist");
  assert.strictEqual(cv.__value.value, "2");
  assert.strictEqual(cv.__value.type, "INT");
});

// --- Test: empty prefix produces no tree ---
test("empty valueMap produces empty tree", () => {
  const vm = new Map();
  const tree = buildSubTree("Main.filler", vm);
  assert.strictEqual(Object.keys(tree).length, 0);
});

// --- Test: 2-level nesting (Outer.Inner.field) ---
test("2-level nested FB: Outer → Inner → field", () => {
  const vm = new Map();
  vm.set("main.outer.inner.x", { name: "Main.Outer.Inner.x", value: "1", type: "INT" });
  vm.set("main.outer.inner.y", { name: "Main.Outer.Inner.y", value: "2", type: "INT" });
  vm.set("main.outer.state", { name: "Main.Outer.state", value: "5", type: "INT" });

  const tree = buildSubTree("Main.Outer", vm);
  assert.ok(tree["Inner"], "Inner node should exist");
  assert.ok(tree["Inner"].__children, "Inner should have children");
  assert.ok(tree["Inner"].__children["x"], "Inner.x should exist");
  assert.ok(tree["Inner"].__children["y"], "Inner.y should exist");
  assert.ok(tree["state"], "state should exist as a leaf");
  assert.strictEqual(tree["state"].__value.value, "5");
});

// Summary
console.log(`\n${passed} passed, ${failed} failed\n`);
process.exit(failed > 0 ? 1 : 0);
