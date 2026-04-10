# PLC Monitor Panel — UI Tests

Playwright-based end-to-end tests for the PLC Monitor panel's tree rendering,
expand/collapse, value updates, and edge cases.

## Setup (one-time)

```bash
cd editors/vscode/test/ui
npm install
npm run install-browsers
```

## Run tests

```bash
# Headless (CI)
npm test

# Headed (watch the browser)
npm run test:headed

# Debug mode (step through with Playwright Inspector)
npm run test:debug
```

## What's tested

- **Empty state** — placeholder message when no watches
- **Scalar watch** — flat row with value, no tree toggle
- **FB instance watch** — tree toggle visible, collapsed by default
- **Tree expansion** — clicking ▸ shows direct fields + nested FB groups
- **Nested expansion** — expanding counter inside filler shows CTU fields (CU, Q, CV, PV, RESET, prev_cu)
- **Collapse** — hides children, toggle reverts to ▸
- **Direct FB watch** — watching `Main.filler.counter` shows only counter's fields, not filler-level
- **Value updates** — telemetry tick changes values without rebuilding DOM
- **Tree value updates** — values inside expanded tree update live
- **No duplicates** — overlapping watches don't produce duplicate rows
- **Multiple watches** — independent watches render correctly
- **Disambiguated names** — `counter.Q` and `edge.Q` are distinct rows at different tree paths

## Visual fixture

Open `../monitor-panel-visual.html` in any browser for manual interactive testing.
Click buttons to add watches, simulate telemetry, and visually verify the tree.

## Architecture

The tests load `monitor-panel-visual.html` which contains an **exact copy** of the
tree-building JS logic from `monitorPanel.ts`, fed with mock data matching the
`multi_file_project` playground. If these tests pass, the real VS Code webview
renders correctly.

The mock data simulates:
- `Main.filler` (FillController FB) with 7 scalar fields
- `Main.filler.counter` (CTU nested FB) with 6 fields
- `Main.filler.edge` (R_TRIG nested FB) with 3 fields
- `Main.cycle` (scalar INT)
