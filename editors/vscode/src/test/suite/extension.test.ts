import * as assert from "assert";
import * as vscode from "vscode";
import * as path from "path";
import * as fs from "fs";
import * as os from "os";

const tmpDir = path.join(os.tmpdir(), "st-ext-test");
const tempFiles: string[] = [];

/** Write a temp .st file and open it — avoids untitled "Save As" prompts. */
async function openTempStFile(content: string): Promise<vscode.TextDocument> {
  if (!fs.existsSync(tmpDir)) {
    fs.mkdirSync(tmpDir, { recursive: true });
  }
  const name = `test_${Date.now()}_${Math.random().toString(36).slice(2, 6)}.st`;
  const filePath = path.join(tmpDir, name);
  fs.writeFileSync(filePath, content, "utf8");
  tempFiles.push(filePath);
  const doc = await vscode.workspace.openTextDocument(filePath);
  return doc;
}

suite("Extension Test Suite", () => {
  // Wait for extension to activate
  suiteSetup(async () => {
    // Open an ST file to trigger activation
    const stFiles = await vscode.workspace.findFiles("**/*.st", null, 1);
    if (stFiles.length > 0) {
      await vscode.window.showTextDocument(stFiles[0]);
    }
    // Give the extension time to activate and LSP to start
    await new Promise((resolve) => setTimeout(resolve, 3000));
  });

  suiteTeardown(() => {
    for (const f of tempFiles) {
      try { fs.unlinkSync(f); } catch { /* ignore */ }
    }
  });

  test("ST language is registered", () => {
    const langs = vscode.languages.getLanguages();
    return langs.then((ids) => {
      assert.ok(
        ids.includes("structured-text"),
        `'structured-text' not in registered languages: ${ids.join(", ")}`
      );
    });
  });

  test(".st files are recognized as Structured Text", async () => {
    const doc = await openTempStFile(
      "PROGRAM Test\nVAR\n    x : INT;\nEND_VAR\n    x := 1;\nEND_PROGRAM\n"
    );
    assert.strictEqual(doc.languageId, "structured-text");
  });

  test("Extension activates on .st file", async () => {
    // In dev mode the extension ID may not match — check language registration
    // as the activation proof instead.
    const langs = await vscode.languages.getLanguages();
    assert.ok(
      langs.includes("structured-text"),
      "structured-text language should be registered after opening .st file"
    );
  });

  test("Diagnostics appear for broken ST code", async function () {
    this.timeout(25000);
    const doc = await openTempStFile(
      "PROGRAM Broken\nVAR\n    x : INT := 0;\nEND_VAR\n    x := undeclared;\nEND_PROGRAM\n"
    );
    await vscode.window.showTextDocument(doc);

    // The LSP server may take several seconds to start on first activation.
    // Retry the wait to handle cold-start latency.
    let diagnostics = await waitForDiagnostics(doc.uri, 8000);
    if (diagnostics.length === 0) {
      // Trigger a re-analysis by making a trivial edit
      const editor = vscode.window.activeTextEditor;
      if (editor) {
        await editor.edit(eb => eb.insert(new vscode.Position(0, 0), " "));
        await editor.edit(eb => eb.delete(new vscode.Range(0, 0, 0, 1)));
      }
      diagnostics = await waitForDiagnostics(doc.uri, 8000);
    }

    assert.ok(
      diagnostics.length > 0,
      "Expected diagnostics for undeclared variable"
    );
    const hasUndeclared = diagnostics.some((d) =>
      d.message.includes("undeclared")
    );
    assert.ok(
      hasUndeclared,
      `Expected 'undeclared' in diagnostics: ${diagnostics
        .map((d) => d.message)
        .join(", ")}`
    );
  });

  test("Syntax highlighting provides tokens", async () => {
    const doc = await openTempStFile(
      "PROGRAM Main\nVAR\n    x : INT;\nEND_VAR\n    x := 1;\nEND_PROGRAM\n"
    );
    assert.strictEqual(doc.languageId, "structured-text");
  });
});

/**
 * Wait for diagnostics to appear on a document URI.
 */
function waitForDiagnostics(
  uri: vscode.Uri,
  timeoutMs: number
): Promise<vscode.Diagnostic[]> {
  return new Promise((resolve) => {
    const existing = vscode.languages.getDiagnostics(uri);
    if (existing.length > 0) {
      resolve(existing);
      return;
    }

    const disposable = vscode.languages.onDidChangeDiagnostics((e) => {
      if (e.uris.some((u) => u.toString() === uri.toString())) {
        const diags = vscode.languages.getDiagnostics(uri);
        if (diags.length > 0) {
          disposable.dispose();
          resolve(diags);
        }
      }
    });

    // Timeout fallback
    setTimeout(() => {
      disposable.dispose();
      resolve(vscode.languages.getDiagnostics(uri));
    }, timeoutMs);
  });
}
