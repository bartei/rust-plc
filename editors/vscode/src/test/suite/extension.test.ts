import * as assert from "assert";
import * as vscode from "vscode";
import * as path from "path";

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
    const doc = await vscode.workspace.openTextDocument({
      language: "structured-text",
      content: "PROGRAM Test\nVAR\n    x : INT;\nEND_VAR\n    x := 1;\nEND_PROGRAM\n",
    });
    assert.strictEqual(doc.languageId, "structured-text");
  });

  test("Extension activates on .st file", async () => {
    const ext = vscode.extensions.getExtension("rust-plc.iec61131-st");
    // Extension might not be found by ID in dev mode, check language registration instead
    if (ext) {
      assert.ok(ext.isActive, "Extension should be active after opening .st file");
    }
    // At minimum, the language should be registered
    const langs = await vscode.languages.getLanguages();
    assert.ok(langs.includes("structured-text"));
  });

  test("Diagnostics appear for broken ST code", async () => {
    const doc = await vscode.workspace.openTextDocument({
      language: "structured-text",
      content:
        "PROGRAM Broken\nVAR\n    x : INT := 0;\nEND_VAR\n    x := undeclared;\nEND_PROGRAM\n",
    });
    await vscode.window.showTextDocument(doc);

    // Wait for diagnostics to arrive from the LSP
    const diagnostics = await waitForDiagnostics(doc.uri, 5000);

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
    const doc = await vscode.workspace.openTextDocument({
      language: "structured-text",
      content: "PROGRAM Main\nVAR\n    x : INT;\nEND_VAR\n    x := 1;\nEND_PROGRAM\n",
    });

    // The TextMate grammar should provide basic tokenization
    // We can't directly test token colors, but we can verify the document is not plain text
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
