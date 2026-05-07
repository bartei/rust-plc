import * as assert from "assert";
import * as vscode from "vscode";
import * as path from "path";
import * as fs from "fs";
import * as os from "os";

/**
 * YAML schema acceptance test (headless VS Code).
 *
 * Verifies the `yamlValidation` contribution in the iec61131-st extension's
 * package.json wires the bundled schemas to the right files:
 *
 *   - plc-project.yaml         → schemas/plc-project.schema.json
 *   - **\/profiles/*.yaml      → schemas/device-profile.schema.json
 *
 * The test:
 *   1. opens a plc-project.yaml with a deliberately invalid `version`
 *      (must match the regex in the schema) and asserts the YAML
 *      language server emits a diagnostic.
 *   2. opens a device profile with an unknown protocol enum value and
 *      asserts the schema's `enum` constraint fires.
 *   3. invokes `vscode.executeCompletionItemProvider` at the top level
 *      of an empty plc-project.yaml and asserts schema-defined
 *      properties (e.g. `name`, `version`, `engine`) appear in the
 *      completion list.
 *
 * No mocks. The test exercises the actual `redhat.vscode-yaml` extension
 * against the actual bundled schemas — same code path a real user
 * triggers when editing the file.
 */

const SCHEMA_DIAGNOSTIC_TIMEOUT_MS = 20000;
const COMPLETION_TIMEOUT_MS = 10000;

let tmpDir: string;
const tempFiles: string[] = [];

suite("YAML schema validation (headless VS Code)", () => {
  suiteSetup(async function () {
    this.timeout(60000);
    tmpDir = path.join(os.tmpdir(), "st-schema-acceptance-" + Date.now());
    fs.mkdirSync(path.join(tmpDir, "profiles"), { recursive: true });

    // Wait for the YAML extension to activate. Without this, the schema
    // mappings registered by package.json haven't been picked up yet
    // and getDiagnostics returns an empty array.
    const yamlExt = vscode.extensions.getExtension("redhat.vscode-yaml");
    assert.ok(
      yamlExt,
      "redhat.vscode-yaml is not installed in the test VS Code profile — " +
        "the runSchemaTest.ts runner must `--install-extension` it before booting.",
    );
    if (!yamlExt.isActive) {
      await yamlExt.activate();
    }
  });

  suiteTeardown(() => {
    for (const f of tempFiles) {
      try { fs.unlinkSync(f); } catch { /* ignore */ }
    }
    if (tmpDir && fs.existsSync(tmpDir)) {
      try { fs.rmSync(tmpDir, { recursive: true, force: true }); } catch { /* ignore */ }
    }
  });

  test("invalid version in plc-project.yaml produces a schema diagnostic", async function () {
    this.timeout(SCHEMA_DIAGNOSTIC_TIMEOUT_MS + 5000);

    const filePath = path.join(tmpDir, "plc-project.yaml");
    // `version` must match `^[0-9]+\\.[0-9]+\\.[0-9]+.*$` per the schema.
    // "abc" violates that regex.
    fs.writeFileSync(
      filePath,
      "name: BadProject\nversion: abc\nentryPoint: Main\n",
      "utf8",
    );
    tempFiles.push(filePath);

    const doc = await vscode.workspace.openTextDocument(filePath);
    await vscode.window.showTextDocument(doc);

    const diags = await waitForDiagnostics(
      doc.uri,
      SCHEMA_DIAGNOSTIC_TIMEOUT_MS,
      (ds) => ds.some((d) =>
        d.message.toLowerCase().includes("pattern") ||
        d.message.toLowerCase().includes("version") ||
        d.message.includes("[0-9]"),
      ),
    );
    assert.ok(
      diags.length > 0,
      `expected at least one schema diagnostic on plc-project.yaml, got none`,
    );
    const versionDiag = diags.find((d) =>
      d.range.start.line === 1 || /version|pattern|\[0-9\]/i.test(d.message),
    );
    assert.ok(
      versionDiag,
      `expected diagnostic about version regex, got: ${diags.map((d) => d.message).join(" | ")}`,
    );
  });

  test("clean plc-project.yaml has no schema diagnostics", async function () {
    this.timeout(SCHEMA_DIAGNOSTIC_TIMEOUT_MS + 5000);

    // Use a different filename inside a subdir so the schema still applies
    // (fileMatch is "plc-project.yaml" — exact filename).
    const subdir = path.join(tmpDir, "clean-prj");
    fs.mkdirSync(subdir, { recursive: true });
    const filePath = path.join(subdir, "plc-project.yaml");
    fs.writeFileSync(
      filePath,
      "name: CleanProject\nversion: 1.0.0\nentryPoint: Main\n",
      "utf8",
    );
    tempFiles.push(filePath);

    const doc = await vscode.workspace.openTextDocument(filePath);
    await vscode.window.showTextDocument(doc);

    // Settle: give the YAML server time to validate.
    await sleep(2000);
    const diags = vscode.languages.getDiagnostics(doc.uri);
    // Filter out non-schema diagnostics (e.g. parse errors). Schema
    // errors carry source 'YAML' from the redhat extension.
    const schemaDiags = diags.filter((d) => /yaml/i.test(d.source ?? ""));
    assert.strictEqual(
      schemaDiags.length, 0,
      `clean plc-project.yaml must have no schema diagnostics, got: ${
        schemaDiags.map((d) => d.message).join(" | ")
      }`,
    );
  });

  test("invalid protocol enum in profiles/*.yaml produces a schema diagnostic", async function () {
    this.timeout(SCHEMA_DIAGNOSTIC_TIMEOUT_MS + 5000);

    const filePath = path.join(tmpDir, "profiles", "bad_profile.yaml");
    // `protocol` enum is restricted to modbus-tcp/-rtu/-ascii/generic.
    // "dnp3" is not in the enum — must be reported.
    fs.writeFileSync(
      filePath,
      [
        "name: BadDevice",
        "vendor: Test",
        "protocol: dnp3",
        "fields:",
        "  - name: x",
        "    type: int16",
        "    address: 0",
        "    kind: input",
        "",
      ].join("\n"),
      "utf8",
    );
    tempFiles.push(filePath);

    const doc = await vscode.workspace.openTextDocument(filePath);
    await vscode.window.showTextDocument(doc);

    const diags = await waitForDiagnostics(
      doc.uri,
      SCHEMA_DIAGNOSTIC_TIMEOUT_MS,
      (ds) => ds.some((d) =>
        d.message.toLowerCase().includes("modbus") ||
        d.message.toLowerCase().includes("dnp3") ||
        d.message.toLowerCase().includes("enum"),
      ),
    );
    assert.ok(
      diags.length > 0,
      "expected schema diagnostic on profiles/bad_profile.yaml, got none",
    );
  });

  test("targets section is accepted (regression for missing 'targets' property)", async function () {
    this.timeout(SCHEMA_DIAGNOSTIC_TIMEOUT_MS + 5000);

    // Earlier the schema rejected `targets:` with "Property targets is
    // not allowed" because the deployment-targets section was never
    // wired. This pins the fix.
    const subdir = path.join(tmpDir, "with-targets");
    fs.mkdirSync(subdir, { recursive: true });
    const filePath = path.join(subdir, "plc-project.yaml");
    fs.writeFileSync(
      filePath,
      [
        "name: ProjectWithTargets",
        "version: 1.0.0",
        "entryPoint: Main",
        "targets:",
        "  - name: line1-plc",
        "    host: 10.1.2.193",
        "    user: plc",
        "    auth: key",
        "    os: linux",
        "    arch: aarch64",
        "    agent_port: 4840",
        "default_target: line1-plc",
        "",
      ].join("\n"),
      "utf8",
    );
    tempFiles.push(filePath);

    const doc = await vscode.workspace.openTextDocument(filePath);
    await vscode.window.showTextDocument(doc);
    await sleep(2000);

    const schemaDiags = vscode.languages.getDiagnostics(doc.uri)
      .filter((d) => /yaml/i.test(d.source ?? ""));
    assert.strictEqual(
      schemaDiags.length, 0,
      `targets section must be accepted, got: ${
        schemaDiags.map((d) => `[L${d.range.start.line}] ${d.message}`).join(" | ")
      }`,
    );
  });

  test("simulated link + simulated protocol device is accepted (regression for missing enum values)", async function () {
    this.timeout(SCHEMA_DIAGNOSTIC_TIMEOUT_MS + 5000);

    // The link.type and device.protocol enums originally omitted
    // 'simulated', breaking every playground project that uses an
    // in-memory link for development.
    const subdir = path.join(tmpDir, "simulated-io");
    fs.mkdirSync(subdir, { recursive: true });
    const filePath = path.join(subdir, "plc-project.yaml");
    fs.writeFileSync(
      filePath,
      [
        "name: SimulatedIO",
        "version: 1.0.0",
        "entryPoint: Main",
        "links:",
        "  - name: sim_link",
        "    type: simulated",
        "devices:",
        "  - name: io_rack",
        "    link: sim_link",
        "    protocol: simulated",
        "    mode: cyclic",
        "    device_profile: sim_8di_4ai_4do_2ao",
        "",
      ].join("\n"),
      "utf8",
    );
    tempFiles.push(filePath);

    const doc = await vscode.workspace.openTextDocument(filePath);
    await vscode.window.showTextDocument(doc);
    await sleep(2000);

    const schemaDiags = vscode.languages.getDiagnostics(doc.uri)
      .filter((d) => /yaml/i.test(d.source ?? ""));
    assert.strictEqual(
      schemaDiags.length, 0,
      `simulated link/protocol must validate, got: ${
        schemaDiags.map((d) => `[L${d.range.start.line}] ${d.message}`).join(" | ")
      }`,
    );
  });

  test("real-world template_project/plc-project.yaml has zero schema diagnostics", async function () {
    this.timeout(SCHEMA_DIAGNOSTIC_TIMEOUT_MS + 5000);

    // The shipped template uses every documented top-level field. If
    // the schema diverges from the actual struct definitions, this
    // test catches it. We open the file in-place from the playground
    // directory (which is the test workspace) so its inline
    // `# yaml-language-server: $schema=../../schemas/...` directive
    // still resolves correctly. Strip the directive's lines from
    // diagnostics in case the inline schema URL points at a path the
    // language server can't load (we want to validate against the
    // CONTRIBUTED schema, not the inline override).
    const repoRoot = path.resolve(__dirname, "../../../../..");
    const templatePath = path.join(repoRoot, "playground", "template_project", "plc-project.yaml");
    if (!fs.existsSync(templatePath)) {
      console.log(`skipping: template not found at ${templatePath}`);
      return;
    }

    const doc = await vscode.workspace.openTextDocument(templatePath);
    await vscode.window.showTextDocument(doc);
    await sleep(3000); // template is larger; give the server time to validate

    const schemaDiags = vscode.languages.getDiagnostics(doc.uri)
      // Filter out diagnostics from the inline `# yaml-language-server: $schema=`
      // directive itself (those are about the schema URL not loading,
      // not about the file content).
      .filter((d) => /yaml/i.test(d.source ?? ""))
      .filter((d) =>
        !/Unable to load schema|cannot load schema|No content/i.test(d.message),
      );
    assert.strictEqual(
      schemaDiags.length, 0,
      `template plc-project.yaml must validate cleanly, got: ${
        schemaDiags.map((d) => `[L${d.range.start.line + 1}] ${d.message}`).join(" | ")
      }`,
    );
  });

  test("completion in plc-project.yaml surfaces schema-defined properties", async function () {
    this.timeout(COMPLETION_TIMEOUT_MS + 5000);

    const filePath = path.join(tmpDir, "completion-test", "plc-project.yaml");
    fs.mkdirSync(path.dirname(filePath), { recursive: true });
    // Empty document — completion at line 0 col 0 should offer the
    // top-level schema properties.
    fs.writeFileSync(filePath, "", "utf8");
    tempFiles.push(filePath);

    const doc = await vscode.workspace.openTextDocument(filePath);
    await vscode.window.showTextDocument(doc);

    // Give the YAML server a moment to attach the schema.
    await sleep(2000);

    const list = await vscode.commands.executeCommand<vscode.CompletionList>(
      "vscode.executeCompletionItemProvider",
      doc.uri,
      new vscode.Position(0, 0),
    );
    const labels = (list?.items ?? []).map((i) =>
      typeof i.label === "string" ? i.label : i.label.label,
    );
    // The schema declares `name`, `version`, `entryPoint`, `engine`,
    // `targets` etc. at least one must be in the completion result for
    // the schema mapping to be working.
    const expected = ["name", "version", "entryPoint", "engine"];
    const hit = expected.find((p) => labels.includes(p));
    assert.ok(
      hit,
      `expected at least one of ${JSON.stringify(expected)} in completion ` +
        `labels for empty plc-project.yaml, got: ${JSON.stringify(labels.slice(0, 20))}`,
    );
    // `targets` was the field that was originally missing from the
    // schema. Pin it explicitly so a future regression is caught here.
    assert.ok(
      labels.includes("targets"),
      `'targets' must appear in completion at top level of plc-project.yaml, ` +
        `got: ${JSON.stringify(labels.slice(0, 20))}`,
    );
  });

  // ── UPP / pyrometer schema additions ────────────────────────────

  test("device profile with protocol: upp and per-field upp: bindings validates clean", async function () {
    this.timeout(SCHEMA_DIAGNOSTIC_TIMEOUT_MS + 5000);

    // Pyrometer profiles use `upp:` blocks instead of `register:`.
    // The schema's `oneOf` on the field must accept this shape.
    const filePath = path.join(tmpDir, "profiles", "pyro_clean.yaml");
    fs.writeFileSync(
      filePath,
      [
        "name: ImpacIgar6Smart",
        "vendor: Advanced Energy",
        "protocol: upp",
        "fields:",
        "  - name: temperature",
        "    type: REAL",
        "    direction: input",
        "    upp:",
        "      command: ms",
        "      decoder: temp_5d_tenth",
        "  - name: emissivity",
        "    type: REAL",
        "    direction: inout",
        "    upp:",
        "      command: em",
        "      decoder: u16_dec_milli",
        "",
      ].join("\n"),
      "utf8",
    );
    tempFiles.push(filePath);

    const doc = await vscode.workspace.openTextDocument(filePath);
    await vscode.window.showTextDocument(doc);
    await sleep(2000);
    const diags = vscode.languages.getDiagnostics(doc.uri)
      .filter((d) => /yaml/i.test(d.source ?? ""));
    assert.strictEqual(
      diags.length, 0,
      `clean UPP profile must validate, got: ${diags.map((d) => d.message).join(" | ")}`,
    );
  });

  test("device profile with unknown UPP decoder is flagged", async function () {
    this.timeout(SCHEMA_DIAGNOSTIC_TIMEOUT_MS + 5000);

    // `decoder` must come from the schema's enum. An unknown value
    // is the most common typo and must be rejected by the language
    // server, not deferred to runtime.
    const filePath = path.join(tmpDir, "profiles", "pyro_bad_decoder.yaml");
    fs.writeFileSync(
      filePath,
      [
        "name: BadProfile",
        "protocol: upp",
        "fields:",
        "  - name: temperature",
        "    type: REAL",
        "    direction: input",
        "    upp:",
        "      command: ms",
        "      decoder: definitely_not_a_decoder",
        "",
      ].join("\n"),
      "utf8",
    );
    tempFiles.push(filePath);

    const doc = await vscode.workspace.openTextDocument(filePath);
    await vscode.window.showTextDocument(doc);
    const diags = await waitForDiagnostics(
      doc.uri,
      SCHEMA_DIAGNOSTIC_TIMEOUT_MS,
      (ds) => ds.some((d) => /enum|allowed values|decoder/i.test(d.message)),
    );
    assert.ok(
      diags.length > 0,
      "expected enum-rejection diagnostic for unknown decoder",
    );
  });

  test("device profile field with neither register nor upp is flagged", async function () {
    this.timeout(SCHEMA_DIAGNOSTIC_TIMEOUT_MS + 5000);

    // The schema's oneOf requires exactly one of `register` or `upp`.
    // A field with NEITHER must be reported.
    const filePath = path.join(tmpDir, "profiles", "pyro_no_binding.yaml");
    fs.writeFileSync(
      filePath,
      [
        "name: NoBinding",
        "fields:",
        "  - name: temperature",
        "    type: REAL",
        "    direction: input",
        "",
      ].join("\n"),
      "utf8",
    );
    tempFiles.push(filePath);

    const doc = await vscode.workspace.openTextDocument(filePath);
    await vscode.window.showTextDocument(doc);
    const diags = await waitForDiagnostics(
      doc.uri,
      SCHEMA_DIAGNOSTIC_TIMEOUT_MS,
      (ds) => ds.some((d) => /oneOf|register|upp|missing/i.test(d.message)),
    );
    assert.ok(
      diags.length > 0,
      "expected oneOf / missing-binding diagnostic when field has neither register: nor upp:",
    );
  });

  test("plc-project.yaml accepts protocol: upp on a device", async function () {
    this.timeout(SCHEMA_DIAGNOSTIC_TIMEOUT_MS + 5000);

    // Regression: 'upp' was added to the device.protocol enum in
    // plc-project.schema.json alongside the new pyrometer crate.
    const subdir = path.join(tmpDir, "with-upp-device");
    fs.mkdirSync(subdir, { recursive: true });
    const filePath = path.join(subdir, "plc-project.yaml");
    fs.writeFileSync(
      filePath,
      [
        "name: WithUppDevice",
        "version: 1.0.0",
        "entryPoint: Main",
        "links:",
        "  - name: rs485_bus",
        "    type: serial",
        "    port: /dev/ttyUSB0",
        "    baud: 19200",
        "    parity: even",
        "devices:",
        "  - name: pyro1",
        "    link: rs485_bus",
        "    protocol: upp",
        "    mode: cyclic",
        "    device_profile: impac_igar_6_smart",
        "",
      ].join("\n"),
      "utf8",
    );
    tempFiles.push(filePath);

    const doc = await vscode.workspace.openTextDocument(filePath);
    await vscode.window.showTextDocument(doc);
    await sleep(2000);
    const diags = vscode.languages.getDiagnostics(doc.uri)
      .filter((d) => /yaml/i.test(d.source ?? ""));
    assert.strictEqual(
      diags.length, 0,
      `protocol: upp must validate, got: ${diags.map((d) => d.message).join(" | ")}`,
    );
  });

  test("shipped profiles/impac_igar_6_smart.yaml has zero schema diagnostics", async function () {
    this.timeout(SCHEMA_DIAGNOSTIC_TIMEOUT_MS + 5000);

    // Read the shipped YAML straight from the repo. Catches the
    // case where someone adds a new field whose decoder name isn't
    // in the schema enum.
    const repoRoot = path.resolve(__dirname, "../../../../..");
    const profilePath = path.join(repoRoot, "profiles", "impac_igar_6_smart.yaml");
    if (!fs.existsSync(profilePath)) {
      console.log(`skipping: profile not found at ${profilePath}`);
      return;
    }
    const doc = await vscode.workspace.openTextDocument(profilePath);
    await vscode.window.showTextDocument(doc);
    await sleep(3000);
    const diags = vscode.languages.getDiagnostics(doc.uri)
      .filter((d) => /yaml/i.test(d.source ?? ""))
      .filter((d) =>
        !/Unable to load schema|cannot load schema|No content/i.test(d.message),
      );
    assert.strictEqual(
      diags.length, 0,
      `shipped IGAR profile must validate cleanly, got: ${
        diags.map((d) => `[L${d.range.start.line + 1}] ${d.message}`).join(" | ")
      }`,
    );
  });
});

// ── helpers ───────────────────────────────────────────────────────────

function sleep(ms: number): Promise<void> {
  return new Promise((res) => setTimeout(res, ms));
}

/**
 * Poll `vscode.languages.getDiagnostics(uri)` until `predicate` is true
 * or the deadline expires. Returns the diagnostics at the time the
 * predicate first matched (or the last snapshot before timeout). The
 * YAML language server validates asynchronously, so we cannot read
 * diagnostics synchronously after openTextDocument.
 */
async function waitForDiagnostics(
  uri: vscode.Uri,
  timeoutMs: number,
  predicate: (ds: vscode.Diagnostic[]) => boolean,
): Promise<vscode.Diagnostic[]> {
  const start = Date.now();
  let last: vscode.Diagnostic[] = [];
  while (Date.now() - start < timeoutMs) {
    last = vscode.languages.getDiagnostics(uri);
    if (predicate(last)) {
      return last;
    }
    await sleep(200);
  }
  return last;
}
