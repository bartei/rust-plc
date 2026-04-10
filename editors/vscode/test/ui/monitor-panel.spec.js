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
});
