// @ts-check
const { test, expect } = require("@playwright/test");

/**
 * PLC Monitor Panel — Playwright UI tests.
 *
 * These tests load the visual test fixture HTML (which simulates the exact
 * same JS logic as the real VS Code webview) and validate that the tree
 * rendering, expand/collapse, value updates, and edge cases all work
 * correctly in a real browser DOM.
 *
 * Run:
 *   cd editors/vscode/test/ui
 *   npm install
 *   npm run install-browsers
 *   npm test
 */

test.describe("PLC Monitor Watch List", () => {
  test.beforeEach(async ({ page }) => {
    await page.goto("/");
    // Verify the fixture loaded
    await expect(page.locator("h2").first()).toHaveText(
      "PLC Monitor Panel — Visual Test Fixture"
    );
  });

  // =========================================================================
  // Basic watch operations
  // =========================================================================

  test("empty state shows placeholder message", async ({ page }) => {
    await expect(page.locator("#var-body")).toContainText(
      "Watch list is empty"
    );
  });

  test("adding a scalar variable shows flat row with value", async ({
    page,
  }) => {
    await page.click('[data-testid="btn-watch-cycle"]');

    const row = page.locator('tr[data-var="main.cycle"]');
    await expect(row).toBeVisible();
    await expect(row.locator(".name")).toContainText("Main.cycle");
    await expect(row.locator(".value")).not.toHaveText("…");

    // Scalar should NOT have a tree toggle
    await expect(row.locator(".tree-toggle")).toHaveCount(0);
  });

  test("removing a watch clears it from the table", async ({ page }) => {
    await page.click('[data-testid="btn-watch-cycle"]');
    await expect(page.locator('tr[data-var="main.cycle"]')).toBeVisible();

    // Click the Remove button
    await page.locator('tr[data-var="main.cycle"] button').filter({ hasText: "Remove" }).click();
    await expect(page.locator('tr[data-var="main.cycle"]')).toHaveCount(0);
    await expect(page.locator("#var-body")).toContainText(
      "Watch list is empty"
    );
  });

  test("clear all empties the table", async ({ page }) => {
    await page.click('[data-testid="btn-watch-cycle"]');
    await page.click('[data-testid="btn-watch-counter"]');
    await page.click('[data-testid="btn-clear"]');
    await expect(page.locator("#var-body")).toContainText(
      "Watch list is empty"
    );
  });

  // =========================================================================
  // Tree rendering for FB instances
  // =========================================================================

  test("watching Main.filler shows a tree toggle", async ({ page }) => {
    await page.click('[data-testid="btn-watch-filler"]');

    const row = page.locator('tr[data-var="main.filler"]');
    await expect(row).toBeVisible();
    await expect(row.locator(".tree-toggle")).toHaveCount(1);
    // Should be collapsed initially (▸)
    await expect(row.locator(".tree-toggle")).toContainText("▸");
  });

  test("expanding Main.filler shows direct fields and nested FB groups", async ({
    page,
  }) => {
    await page.click('[data-testid="btn-watch-filler"]');

    // Click the toggle to expand
    await page.locator('tr[data-var="main.filler"] .tree-toggle').click();

    // Should now show child rows
    const tbody = page.locator("#var-body");

    // Direct scalar fields should be visible
    await expect(tbody.locator('tr[data-var="main.filler.start"]')).toBeVisible();
    await expect(tbody.locator('tr[data-var="main.filler.fill_count"]')).toBeVisible();
    await expect(tbody.locator('tr[data-var="main.filler.filling"]')).toBeVisible();

    // Nested FBs should appear as intermediate nodes with their own toggles
    const counterRow = tbody.locator('tr[data-var="main.filler.counter"]');
    await expect(counterRow).toBeVisible();
    await expect(counterRow.locator(".tree-toggle")).toHaveCount(1);

    const edgeRow = tbody.locator('tr[data-var="main.filler.edge"]');
    await expect(edgeRow).toBeVisible();
    await expect(edgeRow.locator(".tree-toggle")).toHaveCount(1);

    // Counter's children should NOT be visible yet (counter is collapsed)
    await expect(
      tbody.locator('tr[data-var="main.filler.counter.q"]')
    ).toHaveCount(0);
  });

  test("expanding counter inside filler shows CTU fields", async ({
    page,
  }) => {
    await page.click('[data-testid="btn-watch-filler"]');

    // Expand filler
    await page.locator('tr[data-var="main.filler"] .tree-toggle').click();

    // Expand counter
    await page.locator('tr[data-var="main.filler.counter"] .tree-toggle').click();

    const tbody = page.locator("#var-body");

    // CTU fields should now be visible
    await expect(
      tbody.locator('tr[data-var="main.filler.counter.cu"]')
    ).toBeVisible();
    await expect(
      tbody.locator('tr[data-var="main.filler.counter.q"]')
    ).toBeVisible();
    await expect(
      tbody.locator('tr[data-var="main.filler.counter.cv"]')
    ).toBeVisible();
    await expect(
      tbody.locator('tr[data-var="main.filler.counter.pv"]')
    ).toBeVisible();
    await expect(
      tbody.locator('tr[data-var="main.filler.counter.prev_cu"]')
    ).toBeVisible();
    await expect(
      tbody.locator('tr[data-var="main.filler.counter.reset"]')
    ).toBeVisible();

    // Toggle indicator should be ▾ (expanded)
    await expect(
      page.locator('tr[data-var="main.filler.counter"] .tree-toggle')
    ).toContainText("▾");
  });

  test("collapsing counter hides its children", async ({ page }) => {
    await page.click('[data-testid="btn-watch-filler"]');
    await page.locator('tr[data-var="main.filler"] .tree-toggle').click();
    await page.locator('tr[data-var="main.filler.counter"] .tree-toggle').click();

    // Children visible
    await expect(
      page.locator('tr[data-var="main.filler.counter.q"]')
    ).toBeVisible();

    // Collapse counter
    await page.locator('tr[data-var="main.filler.counter"] .tree-toggle').click();

    // Children hidden
    await expect(
      page.locator('tr[data-var="main.filler.counter.q"]')
    ).toHaveCount(0);

    // Toggle back to ▸
    await expect(
      page.locator('tr[data-var="main.filler.counter"] .tree-toggle')
    ).toContainText("▸");
  });

  // =========================================================================
  // Watching a nested FB directly
  // =========================================================================

  test("watching Main.filler.counter directly shows its fields", async ({
    page,
  }) => {
    await page.click('[data-testid="btn-watch-counter"]');

    const row = page.locator('tr[data-var="main.filler.counter"]');
    await expect(row).toBeVisible();
    await expect(row.locator(".tree-toggle")).toHaveCount(1);

    // Expand
    await row.locator(".tree-toggle").click();

    // Should show CTU fields
    await expect(
      page.locator('tr[data-var="main.filler.counter.q"]')
    ).toBeVisible();
    await expect(
      page.locator('tr[data-var="main.filler.counter.cv"]')
    ).toBeVisible();

    // Should NOT show filler-level fields (start, filling, etc.)
    await expect(
      page.locator('tr[data-var="main.filler.start"]')
    ).toHaveCount(0);
  });

  // =========================================================================
  // Value updates
  // =========================================================================

  test("telemetry tick updates values without rebuilding structure", async ({
    page,
  }) => {
    await page.click('[data-testid="btn-watch-cycle"]');

    const valueCell = page.locator(
      'tr[data-var="main.cycle"] .value'
    );
    const firstValue = await valueCell.textContent();

    // Tick to update values
    await page.click('[data-testid="btn-tick"]');
    const secondValue = await valueCell.textContent();

    expect(firstValue).not.toEqual(secondValue);
  });

  test("values update inside expanded tree", async ({ page }) => {
    await page.click('[data-testid="btn-watch-filler"]');
    await page.locator('tr[data-var="main.filler"] .tree-toggle').click();
    await page.locator('tr[data-var="main.filler.counter"] .tree-toggle').click();

    const cvCell = page.locator(
      'tr[data-var="main.filler.counter.cv"] .value'
    );
    const firstCV = await cvCell.textContent();

    // Multiple ticks to ensure the counter advances
    await page.click('[data-testid="btn-tick"]');
    await page.click('[data-testid="btn-tick"]');
    const laterCV = await cvCell.textContent();

    // CV should have changed (the mock increments it)
    expect(firstCV).not.toEqual(laterCV);
  });

  // =========================================================================
  // Edge cases
  // =========================================================================

  test("no duplicate rows for overlapping watches", async ({ page }) => {
    // Watch both the parent and a child
    await page.click('[data-testid="btn-watch-filler"]');
    await page.click('[data-testid="btn-watch-cv"]');

    // Main.filler should be one row (tree)
    // Main.filler.counter.CV should be a separate flat row
    const fillerRows = page.locator('tr[data-var="main.filler"]');
    await expect(fillerRows).toHaveCount(1);
  });

  test("multiple watches render independently", async ({ page }) => {
    await page.click('[data-testid="btn-watch-cycle"]');
    await page.click('[data-testid="btn-watch-counter"]');

    // Both should be visible
    await expect(
      page.locator('tr[data-var="main.cycle"]')
    ).toBeVisible();
    await expect(
      page.locator('tr[data-var="main.filler.counter"]')
    ).toBeVisible();

    // cycle is flat (no toggle), counter has a toggle
    await expect(
      page.locator('tr[data-var="main.cycle"] .tree-toggle')
    ).toHaveCount(0);
    await expect(
      page.locator('tr[data-var="main.filler.counter"] .tree-toggle')
    ).toHaveCount(1);
  });

  // =========================================================================
  // Counter names: no ambiguity between counter.Q and edge.Q
  // =========================================================================

  test("counter.Q and edge.Q are distinct in the tree", async ({ page }) => {
    await page.click('[data-testid="btn-watch-filler"]');
    await page.locator('tr[data-var="main.filler"] .tree-toggle').click();
    await page.locator('tr[data-var="main.filler.counter"] .tree-toggle').click();
    await page.locator('tr[data-var="main.filler.edge"] .tree-toggle').click();

    // Both Q entries should exist but at different paths
    const counterQ = page.locator(
      'tr[data-var="main.filler.counter.q"]'
    );
    const edgeQ = page.locator('tr[data-var="main.filler.edge.q"]');

    await expect(counterQ).toBeVisible();
    await expect(edgeQ).toBeVisible();

    // They should be separate rows (not the same element)
    const counterQName = await counterQ.locator(".name").textContent();
    const edgeQName = await edgeQ.locator(".name").textContent();
    expect(counterQName.trim()).toBe("Q");
    expect(edgeQName.trim()).toBe("Q");
  });

  // =========================================================================
  // Tree data model with children from telemetry (Item 1 + 2)
  // =========================================================================

  test("tree built from telemetry children array shows same structure", async ({
    page,
  }) => {
    // Use the "Watch with children" button which injects pre-built children
    // from the DAP (tree-structured telemetry) instead of flat dotted paths.
    await page.click('[data-testid="btn-watch-with-children"]');

    const row = page.locator('tr[data-var="main.filler"]');
    await expect(row).toBeVisible();
    await expect(row.locator(".tree-toggle")).toHaveCount(1);

    // Expand filler
    await row.locator(".tree-toggle").click();

    const tbody = page.locator("#var-body");
    // Direct scalar fields from children
    await expect(tbody.locator('tr[data-var="main.filler.start"]')).toBeVisible();
    await expect(tbody.locator('tr[data-var="main.filler.fill_count"]')).toBeVisible();

    // Nested FB groups from children
    const counterRow = tbody.locator('tr[data-var="main.filler.counter"]');
    await expect(counterRow).toBeVisible();
    await expect(counterRow.locator(".tree-toggle")).toHaveCount(1);

    const edgeRow = tbody.locator('tr[data-var="main.filler.edge"]');
    await expect(edgeRow).toBeVisible();
    await expect(edgeRow.locator(".tree-toggle")).toHaveCount(1);
  });

  test("children-based tree expands nested FBs correctly", async ({
    page,
  }) => {
    await page.click('[data-testid="btn-watch-with-children"]');
    // Expand filler
    await page.locator('tr[data-var="main.filler"] .tree-toggle').click();
    // Expand counter (nested FB from children tree)
    await page.locator('tr[data-var="main.filler.counter"] .tree-toggle').click();

    const tbody = page.locator("#var-body");
    // CTU fields from the children array
    await expect(
      tbody.locator('tr[data-var="main.filler.counter.cu"]')
    ).toBeVisible();
    await expect(
      tbody.locator('tr[data-var="main.filler.counter.q"]')
    ).toBeVisible();
    await expect(
      tbody.locator('tr[data-var="main.filler.counter.cv"]')
    ).toBeVisible();
    await expect(
      tbody.locator('tr[data-var="main.filler.counter.pv"]')
    ).toBeVisible();
  });

  // =========================================================================
  // Expand/collapse state persistence (Item 3)
  // =========================================================================

  test("expand/collapse state persists across simulated panel reload", async ({
    page,
  }) => {
    // 1. Add a watch and expand some nodes
    await page.click('[data-testid="btn-watch-filler"]');
    await page.locator('tr[data-var="main.filler"] .tree-toggle').click();
    await page.locator('tr[data-var="main.filler.counter"] .tree-toggle').click();

    // Verify counter children are visible
    await expect(
      page.locator('tr[data-var="main.filler.counter.q"]')
    ).toBeVisible();

    // 2. Verify persistedExpanded was saved (check via JS evaluation)
    const persisted = await page.evaluate(() => persistedExpanded);
    expect(persisted).toContain("main.filler");
    expect(persisted).toContain("main.filler.counter");

    // 3. Simulate panel reload: clear expandedNodes, then restore from persistence
    await page.evaluate(() => {
      expandedNodes.clear();
      renderWatchTable();
    });

    // Counter children should be hidden after clearing
    await expect(
      page.locator('tr[data-var="main.filler.counter.q"]')
    ).toHaveCount(0);
    // Filler children should also be hidden
    await expect(
      page.locator('tr[data-var="main.filler.counter"]')
    ).toHaveCount(0);

    // 4. Restore from persisted state
    await page.click('[data-testid="btn-restore-expanded"]');

    // Filler and counter should be expanded again
    await expect(
      page.locator('tr[data-var="main.filler.counter"]')
    ).toBeVisible();
    await expect(
      page.locator('tr[data-var="main.filler.counter.q"]')
    ).toBeVisible();

    // Filler toggle should show expanded indicator (▾)
    await expect(
      page.locator('tr[data-var="main.filler"] .tree-toggle')
    ).toContainText("▾");
  });

  test("collapsed nodes are removed from persisted state", async ({
    page,
  }) => {
    await page.click('[data-testid="btn-watch-filler"]');
    // Expand filler and counter
    await page.locator('tr[data-var="main.filler"] .tree-toggle').click();
    await page.locator('tr[data-var="main.filler.counter"] .tree-toggle').click();

    // Verify both are persisted
    let persisted = await page.evaluate(() => persistedExpanded);
    expect(persisted).toContain("main.filler");
    expect(persisted).toContain("main.filler.counter");

    // Collapse counter
    await page.locator('tr[data-var="main.filler.counter"] .tree-toggle').click();

    // Counter should be removed from persisted state
    persisted = await page.evaluate(() => persistedExpanded);
    expect(persisted).toContain("main.filler");
    expect(persisted).not.toContain("main.filler.counter");
  });

  test("clear all resets persisted expanded state", async ({ page }) => {
    await page.click('[data-testid="btn-watch-filler"]');
    await page.locator('tr[data-var="main.filler"] .tree-toggle').click();

    let persisted = await page.evaluate(() => persistedExpanded);
    expect(persisted.length).toBeGreaterThan(0);

    await page.click('[data-testid="btn-clear"]');

    persisted = await page.evaluate(() => persistedExpanded);
    expect(persisted.length).toBe(0);
  });

  // =========================================================================
  // Session reset: values update after stop + restart (regression test)
  // =========================================================================

  test("watch list values update after session reset (stop + restart)", async ({
    page,
  }) => {
    // Session 1: add a scalar watch and verify it has a value
    await page.click('[data-testid="btn-watch-cycle"]');
    const valueCell = page.locator('tr[data-var="main.cycle"] .value');
    const session1Value = await valueCell.textContent();
    expect(session1Value).not.toBe("…");

    // Tick a few times so the value advances
    await page.click('[data-testid="btn-tick"]');
    await page.click('[data-testid="btn-tick"]');

    // Session 2: simulate stop + restart (reset clears stale valueMap)
    await page.click('[data-testid="btn-reset-session"]');

    // After reset, the row should show pending (no stale data)
    const afterReset = await valueCell.textContent();
    expect(afterReset).toBe("…");

    // First tick of session 2 might be empty (watch list not synced yet)
    await page.click('[data-testid="btn-empty-tick"]');
    // Values should still be pending
    expect(await valueCell.textContent()).toBe("…");

    // After resync, new telemetry arrives with data
    await page.click('[data-testid="btn-tick"]');
    const session2Value = await valueCell.textContent();
    expect(session2Value).not.toBe("…");
    expect(session2Value).not.toBe("");

    // Tick again — value should change (proving updates work)
    await page.click('[data-testid="btn-tick"]');
    const session2Later = await valueCell.textContent();
    expect(session2Later).not.toBe(session2Value);
  });

  test("FB tree rebuilds after session reset", async ({ page }) => {
    // Session 1: watch an FB, expand it, verify children
    await page.click('[data-testid="btn-watch-filler"]');
    await page.locator('tr[data-var="main.filler"] .tree-toggle').click();
    await expect(
      page.locator('tr[data-var="main.filler.start"]')
    ).toBeVisible();

    // Expand counter too
    await page.locator('tr[data-var="main.filler.counter"] .tree-toggle').click();
    await expect(
      page.locator('tr[data-var="main.filler.counter.q"]')
    ).toBeVisible();

    // Session 2: reset
    await page.click('[data-testid="btn-reset-session"]');

    // After reset, children should be gone (no stale data)
    await expect(
      page.locator('tr[data-var="main.filler.start"]')
    ).toHaveCount(0);
    await expect(
      page.locator('tr[data-var="main.filler.counter.q"]')
    ).toHaveCount(0);

    // New telemetry arrives — tree should rebuild
    await page.click('[data-testid="btn-tick"]');

    // Filler should be back as a tree node
    const fillerRow = page.locator('tr[data-var="main.filler"]');
    await expect(fillerRow).toBeVisible();
    await expect(fillerRow.locator(".tree-toggle")).toHaveCount(1);

    // Expand state persisted from session 1 — filler AND counter
    // should already be expanded without clicking toggles again.
    await expect(
      page.locator('tr[data-var="main.filler.start"]')
    ).toBeVisible();
    await expect(
      page.locator('tr[data-var="main.filler.counter"]')
    ).toBeVisible();
    await expect(
      page.locator('tr[data-var="main.filler.counter.q"]')
    ).toBeVisible();

    // Values should be populated (not empty)
    const cvValue = await page
      .locator('tr[data-var="main.filler.counter.cv"] .value')
      .textContent();
    expect(cvValue).toBeTruthy();
    expect(cvValue).not.toBe("…");
  });
});
