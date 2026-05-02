import * as assert from "assert";
import * as vscode from "vscode";
import * as path from "path";
import * as fs from "fs";
import * as os from "os";

/**
 * Headless-VS-Code acceptance tests for plan items 262–263:
 *
 *  - Hover shows type information on variables
 *  - Go-to-definition navigates to symbol
 *
 * These drive `vscode.executeHoverProvider` / `vscode.executeDefinitionProvider`,
 * which is the *exact* code path triggered when a user hovers a token or
 * presses F12 in the editor — the requests flow through VS Code's LSP
 * client to the `st-cli serve` subprocess that the extension started on
 * activation. They sit on top of `lsp_integration.rs` (subprocess JSON-RPC)
 * by also covering the editor-integration layer that subprocess tests can't.
 */

let tmpDir: string;
const tempFiles: string[] = [];

const SINGLE_FILE = `PROGRAM Main
VAR
    counter : INT := 0;
    flag : BOOL := FALSE;
    temperature : REAL := 21.5;
END_VAR
    counter := counter + 1;
    IF counter > 10 THEN
        flag := TRUE;
    END_IF;
END_PROGRAM
`;

const HELPER = `FUNCTION Add : INT
VAR_INPUT
    a : INT;
    b : INT;
END_VAR
    Add := a + b;
END_FUNCTION
`;

const MAIN_USES_HELPER = `PROGRAM Main
VAR
    sum : INT := 0;
END_VAR
    sum := Add(2, 3);
END_PROGRAM
`;

suite("LSP Acceptance (headless VS Code)", () => {
  suiteSetup(async function () {
    this.timeout(30000);
    tmpDir = path.join(os.tmpdir(), "st-lsp-acceptance-" + Date.now());
    fs.mkdirSync(tmpDir, { recursive: true });

    // Force activation: opening any .st file triggers `onLanguage:structured-text`,
    // which starts the LanguageClient that the subsequent tests drive.
    const seedPath = path.join(tmpDir, "seed.st");
    fs.writeFileSync(seedPath, SINGLE_FILE, "utf8");
    tempFiles.push(seedPath);
    const seedDoc = await vscode.workspace.openTextDocument(seedPath);
    await vscode.window.showTextDocument(seedDoc);

    // Wait for the LSP client to actually start serving. The extension's
    // `activate()` calls `client.start()` asynchronously; until that
    // resolves, executeHoverProvider returns an empty array.
    await waitForLspReady(seedDoc.uri, 20000);
  });

  suiteTeardown(() => {
    for (const f of tempFiles) {
      try { fs.unlinkSync(f); } catch { /* ignore */ }
    }
    if (tmpDir && fs.existsSync(tmpDir)) {
      try { fs.rmSync(tmpDir, { recursive: true, force: true }); } catch { /* ignore */ }
    }
  });

  // ── Hover ───────────────────────────────────────────────────────────

  test("Hover on an INT variable returns type info", async function () {
    this.timeout(30000);

    const filePath = path.join(tmpDir, "hover_int.st");
    fs.writeFileSync(filePath, SINGLE_FILE, "utf8");
    tempFiles.push(filePath);

    const doc = await vscode.workspace.openTextDocument(filePath);
    await vscode.window.showTextDocument(doc);
    await waitForLspReady(doc.uri, 15000);

    // `counter := counter + 1;` is at line 6 (0-indexed); column 4 lands on
    // the first `counter` identifier.
    const hovers = await getHovers(doc.uri, new vscode.Position(6, 4));
    assert.ok(hovers.length > 0, "expected at least one hover provider response");

    const text = renderHovers(hovers);
    assert.ok(
      /int/i.test(text),
      `hover for INT variable 'counter' should mention INT, got:\n${text}`,
    );
    assert.ok(
      text.toLowerCase().includes("counter"),
      `hover should reference the variable name 'counter', got:\n${text}`,
    );
  });

  test("Hover on a REAL variable returns REAL type", async function () {
    this.timeout(30000);

    const filePath = path.join(tmpDir, "hover_real.st");
    fs.writeFileSync(filePath, SINGLE_FILE, "utf8");
    tempFiles.push(filePath);

    const doc = await vscode.workspace.openTextDocument(filePath);
    await vscode.window.showTextDocument(doc);
    await waitForLspReady(doc.uri, 15000);

    // `temperature : REAL := 21.5;` is at line 4; column 6 lands inside
    // the identifier.
    const hovers = await getHovers(doc.uri, new vscode.Position(4, 6));
    assert.ok(hovers.length > 0, "expected at least one hover for `temperature`");

    const text = renderHovers(hovers);
    assert.ok(
      /real/i.test(text),
      `hover for REAL variable 'temperature' should mention REAL, got:\n${text}`,
    );
  });

  test("Hover on whitespace returns no result (handler null path)", async function () {
    this.timeout(15000);
    const filePath = path.join(tmpDir, "hover_blank.st");
    fs.writeFileSync(filePath, SINGLE_FILE, "utf8");
    tempFiles.push(filePath);

    const doc = await vscode.workspace.openTextDocument(filePath);
    await vscode.window.showTextDocument(doc);
    await waitForLspReady(doc.uri, 15000);

    // Line 1 (`VAR`) column 50 is well past the line length — the LSP
    // server's hover handler should short-circuit to None.
    const hovers = await getHovers(doc.uri, new vscode.Position(1, 50));
    const text = renderHovers(hovers);
    assert.ok(
      hovers.length === 0 || text.trim().length === 0,
      `expected no hover content past EOL, got: ${text}`,
    );
  });

  // ── Go-to-definition ────────────────────────────────────────────────

  test("Go-to-definition on a local variable jumps to its declaration", async function () {
    this.timeout(30000);
    const filePath = path.join(tmpDir, "goto_local.st");
    fs.writeFileSync(filePath, SINGLE_FILE, "utf8");
    tempFiles.push(filePath);

    const doc = await vscode.workspace.openTextDocument(filePath);
    await vscode.window.showTextDocument(doc);
    await waitForLspReady(doc.uri, 15000);

    // Cursor on the second `counter` in `counter := counter + 1;` (line 6).
    // Column layout: 4 spaces, "counter" (4-10), " := " (11-14), "counter"
    // (15-21). Column 18 lands inside the second occurrence — F12 should
    // resolve to the declaration on line 2.
    const locations = await getDefinitions(doc.uri, new vscode.Position(6, 18));
    assert.ok(locations.length > 0, "expected at least one definition location");

    const target = locations[0];
    const targetLine = "range" in target ? target.range.start.line : target.targetRange.start.line;
    assert.strictEqual(
      targetLine,
      2,
      `definition of 'counter' should land on line 2 (declaration), got line ${targetLine}`,
    );
  });

  test("Go-to-definition across files lands in the helper file", async function () {
    this.timeout(60000);
    const projDir = path.join(tmpDir, "multi_file_project");
    fs.mkdirSync(projDir, { recursive: true });
    const helperPath = path.join(projDir, "helper.st");
    const mainPath = path.join(projDir, "main.st");
    fs.writeFileSync(helperPath, HELPER, "utf8");
    fs.writeFileSync(mainPath, MAIN_USES_HELPER, "utf8");
    fs.writeFileSync(
      path.join(projDir, "plc-project.yaml"),
      "name: GotoCross\nversion: '1.0.0'\nentryPoint: Main\n",
    );
    tempFiles.push(helperPath, mainPath);

    // The LSP discovers project siblings by walking up from the open file
    // looking for plc-project.yaml. So just opening main.st should be
    // enough — but giving the LSP a moment to load the project + analyze
    // both files significantly cuts flakes here.
    const mainDoc = await vscode.workspace.openTextDocument(mainPath);
    await vscode.window.showTextDocument(mainDoc);
    await waitForLspReady(mainDoc.uri, 30000);
    // Wait for the analysis to settle once siblings are loaded — diagnostics
    // re-fire after the project sources arrive, so a quick second wait
    // ensures `Add` is resolvable.
    await sleep(1500);

    // main.st layout (0-indexed lines):
    //   0: PROGRAM Main
    //   1: VAR
    //   2:     sum : INT := 0;
    //   3: END_VAR
    //   4:     sum := Add(2, 3);
    //   5: END_PROGRAM
    // The `Add` call is on line 4: 4 spaces, "sum" (4-6), " := " (7-10),
    // "Add" (11-13). Try a couple of in-token columns.
    let locations: (vscode.Location | vscode.LocationLink)[] = [];
    for (const col of [12, 11, 13]) {
      locations = await getDefinitions(mainDoc.uri, new vscode.Position(4, col));
      if (locations.length > 0) break;
    }
    assert.ok(locations.length > 0, "expected at least one cross-file definition");

    const target = locations[0];
    const targetUri: vscode.Uri = "uri" in target ? target.uri : target.targetUri;
    const targetRange = "range" in target ? target.range : target.targetRange;
    const same = (a: string, b: string) => path.normalize(a) === path.normalize(b);

    if (!same(targetUri.fsPath, helperPath)) {
      // If we landed in main.st instead of helper.st the LSP either failed
      // to load helper.st as a project sibling, or got the offset
      // arithmetic wrong (the bug we just fixed in document.rs). Print
      // diagnostic detail so future regressions surface immediately.
      const helperContent = fs.readFileSync(helperPath, "utf8");
      const mainContent = fs.readFileSync(mainPath, "utf8");
      assert.fail(
        `cross-file goto should land in helper.st\n` +
        `  got URI:    ${targetUri.fsPath}\n` +
        `  got range:  ${JSON.stringify(targetRange)}\n` +
        `  helper.st:  ${helperContent.length} bytes\n` +
        `  main.st:    ${mainContent.length} bytes`,
      );
    }
  });

  test("Go-to-definition on whitespace returns nothing", async function () {
    this.timeout(15000);
    const filePath = path.join(tmpDir, "goto_blank.st");
    fs.writeFileSync(filePath, SINGLE_FILE, "utf8");
    tempFiles.push(filePath);

    const doc = await vscode.workspace.openTextDocument(filePath);
    await vscode.window.showTextDocument(doc);
    await waitForLspReady(doc.uri, 15000);

    const locations = await getDefinitions(doc.uri, new vscode.Position(0, 0));
    assert.strictEqual(
      locations.length,
      0,
      `whitespace position should yield no definition, got: ${JSON.stringify(locations)}`,
    );
  });
});

// ── helpers ────────────────────────────────────────────────────────────

/**
 * Wait until the language server has produced *something* for the
 * document — diagnostics fired, hover responds non-null, or both. The
 * extension activates asynchronously so the first hover/definition call
 * can race against a not-yet-ready LSP connection.
 */
async function waitForLspReady(uri: vscode.Uri, timeoutMs: number): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    // A trivial hover at (0,0) — even when it returns nothing, a successful
    // round-trip means the LSP is alive and answering requests.
    try {
      const hovers = await vscode.commands.executeCommand<vscode.Hover[]>(
        "vscode.executeHoverProvider",
        uri,
        new vscode.Position(0, 0),
      );
      const diags = vscode.languages.getDiagnostics(uri);
      if (Array.isArray(hovers) && (hovers.length > 0 || diags.length > 0)) {
        return;
      }
    } catch {
      // not yet ready
    }
    await sleep(200);
  }
  // If we time out, the test will fail later with a clearer assertion;
  // we don't throw here so the caller can produce a meaningful message.
}

async function getHovers(uri: vscode.Uri, pos: vscode.Position): Promise<vscode.Hover[]> {
  const result = await vscode.commands.executeCommand<vscode.Hover[]>(
    "vscode.executeHoverProvider",
    uri,
    pos,
  );
  return Array.isArray(result) ? result : [];
}

async function getDefinitions(
  uri: vscode.Uri,
  pos: vscode.Position,
): Promise<(vscode.Location | vscode.LocationLink)[]> {
  const result = await vscode.commands.executeCommand<(vscode.Location | vscode.LocationLink)[]>(
    "vscode.executeDefinitionProvider",
    uri,
    pos,
  );
  return Array.isArray(result) ? result : [];
}

function renderHovers(hovers: vscode.Hover[]): string {
  return hovers
    .flatMap((h) => h.contents)
    .map((c) => {
      if (typeof c === "string") return c;
      if ("value" in c) return c.value;
      return "";
    })
    .join("\n");
}

function sleep(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}
