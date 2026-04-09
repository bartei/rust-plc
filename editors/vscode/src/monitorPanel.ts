import * as vscode from "vscode";

/**
 * WebView panel for PLC online monitoring.
 *
 * The panel shows scan-cycle stats and a user-managed watch list of variables.
 * Variables in the watch list are streamed from the DAP every ~500ms while the
 * program runs. The user adds variables via an autocomplete dropdown populated
 * from the `plc/varCatalog` event the DAP pushes on launch.
 *
 * Watch lists are persisted to workspace state so they survive panel close
 * and reload. Each project (keyed by workspace folder) has its own watch list.
 *
 * Data flows in via postMessage so the DOM updates incrementally — no
 * flicker even at high refresh rates.
 */
export class MonitorPanel {
  public static currentPanel: MonitorPanel | undefined;
  private static workspaceState: vscode.Memento | undefined;
  private readonly panel: vscode.WebviewPanel;
  private disposables: vscode.Disposable[] = [];
  private catalog: Array<{ name: string; type: string }> = [];
  private watchList: string[] = [];

  private constructor(panel: vscode.WebviewPanel) {
    this.panel = panel;
    this.panel.onDidDispose(() => this.dispose(), null, this.disposables);
    this.panel.webview.onDidReceiveMessage(
      (msg) => this.handleWebviewMessage(msg),
      null,
      this.disposables
    );
    // Load persisted watch list for this workspace.
    this.watchList = this.loadWatchList();
    this.setInitialHtml();
    // Push the persisted list to the DAP so the next telemetry tick
    // includes its values.
    if (this.watchList.length > 0) {
      this.sendWatchListToDap();
    }
  }

  public static createOrShow(extensionUri: vscode.Uri) {
    const column = vscode.ViewColumn.Beside;

    if (MonitorPanel.currentPanel) {
      MonitorPanel.currentPanel.panel.reveal(column);
      return;
    }

    const panel = vscode.window.createWebviewPanel(
      "stMonitor",
      "PLC Monitor",
      column,
      {
        enableScripts: true,
        retainContextWhenHidden: true,
      }
    );

    MonitorPanel.currentPanel = new MonitorPanel(panel);
  }

  /// Wire the workspace state once at extension activation. The MonitorPanel
  /// uses this to persist the watch list across panel close / reload.
  public static setWorkspaceState(state: vscode.Memento) {
    MonitorPanel.workspaceState = state;
  }

  public updateVariables(
    vars: Array<{ name: string; value: string; type: string; forced?: boolean }>
  ) {
    this.panel.webview.postMessage({ command: "updateVariables", variables: vars });
  }

  public updateCycleInfo(info: {
    cycle_count: number;
    last_cycle_us: number;
    min_cycle_us: number;
    max_cycle_us: number;
    avg_cycle_us: number;
    target_us: number | null;
    jitter_max_us: number;
    last_period_us: number;
  }) {
    this.panel.webview.postMessage({ command: "updateCycleInfo", info });
  }

  public updateCatalog(catalog: Array<{ name: string; type: string }>) {
    this.catalog = catalog;
    this.panel.webview.postMessage({ command: "updateCatalog", catalog });
    // Send the persisted watch list back to the new session — the DAP doesn't
    // know about it until we re-issue the command.
    if (this.watchList.length > 0) {
      this.sendWatchListToDap();
      this.panel.webview.postMessage({
        command: "updateWatchList",
        watchList: this.watchList,
      });
    }
  }

  private handleWebviewMessage(msg: any) {
    switch (msg.command) {
      case "addWatch":
        if (typeof msg.variable === "string" && msg.variable.trim()) {
          const name = msg.variable.trim();
          if (!this.watchList.some((v) => v.toLowerCase() === name.toLowerCase())) {
            this.watchList.push(name);
            this.saveWatchList();
            this.evaluate(`addWatch ${name}`);
          }
        }
        break;
      case "removeWatch":
        if (typeof msg.variable === "string") {
          const before = this.watchList.length;
          this.watchList = this.watchList.filter(
            (v) => v.toLowerCase() !== msg.variable.toLowerCase()
          );
          if (this.watchList.length !== before) {
            this.saveWatchList();
            this.evaluate(`removeWatch ${msg.variable}`);
          }
        }
        break;
      case "clearWatch":
        this.watchList = [];
        this.saveWatchList();
        this.evaluate("clearWatch");
        break;
      case "force":
        if (typeof msg.variable === "string" && typeof msg.value === "string") {
          this.evaluate(`force ${msg.variable} = ${msg.value}`);
        }
        break;
      case "unforce":
        if (typeof msg.variable === "string") {
          this.evaluate(`unforce ${msg.variable}`);
        }
        break;
    }
  }

  /// Send a synthetic evaluate request to the active DAP session — used
  /// for force / unforce / addWatch / removeWatch / clearWatch.
  private evaluate(expression: string) {
    const session = vscode.debug.activeDebugSession;
    if (!session || session.type !== "st") {
      vscode.window.showWarningMessage(
        "PLC Monitor: no active debug session"
      );
      return;
    }
    session.customRequest("evaluate", { expression, context: "repl" });
  }

  private sendWatchListToDap() {
    const session = vscode.debug.activeDebugSession;
    if (!session || session.type !== "st") return;
    session.customRequest("evaluate", {
      expression: `watchVariables ${this.watchList.join(",")}`,
      context: "repl",
    });
  }

  // ── persistence ────────────────────────────────────────────────────

  private workspaceKey(): string {
    const folder = vscode.workspace.workspaceFolders?.[0];
    const root = folder ? folder.uri.fsPath : "<no-workspace>";
    return `plcMonitor.watchList:${root}`;
  }

  private loadWatchList(): string[] {
    const state = MonitorPanel.workspaceState;
    if (!state) return [];
    const v = state.get<string[]>(this.workspaceKey());
    return Array.isArray(v) ? v : [];
  }

  private saveWatchList() {
    const state = MonitorPanel.workspaceState;
    if (!state) return;
    void state.update(this.workspaceKey(), this.watchList);
  }

  // ── HTML ───────────────────────────────────────────────────────────

  private setInitialHtml() {
    const initialCatalog = JSON.stringify(this.catalog);
    const initialWatchList = JSON.stringify(this.watchList);
    this.panel.webview.html = `<!DOCTYPE html>
<html>
<head>
  <style>
    body {
      font-family: var(--vscode-font-family);
      color: var(--vscode-foreground);
      background: var(--vscode-editor-background);
      padding: 10px;
      font-size: 13px;
    }
    h2 {
      margin-top: 0;
      border-bottom: 1px solid var(--vscode-panel-border);
      padding-bottom: 4px;
      display: flex;
      align-items: center;
      justify-content: space-between;
    }
    h2 .h-actions { font-size: 11px; font-weight: normal; }
    table {
      width: 100%;
      border-collapse: collapse;
      margin-bottom: 20px;
    }
    th, td {
      text-align: left;
      padding: 4px 8px;
      border-bottom: 1px solid var(--vscode-panel-border);
    }
    th {
      background: var(--vscode-editor-selectionBackground);
      font-weight: bold;
    }
    .value {
      font-family: var(--vscode-editor-font-family, monospace);
      font-weight: bold;
    }
    .type {
      color: var(--vscode-descriptionForeground);
      font-style: italic;
    }
    .stats {
      display: grid;
      grid-template-columns: auto 1fr;
      gap: 2px 12px;
      margin-bottom: 16px;
    }
    .stat-label { color: var(--vscode-descriptionForeground); }
    .stat-value {
      font-family: var(--vscode-editor-font-family, monospace);
      font-weight: bold;
    }
    .add-row {
      display: flex;
      gap: 6px;
      margin-bottom: 8px;
    }
    .add-row input {
      flex: 1;
      box-sizing: border-box;
      padding: 4px 8px;
      background: var(--vscode-input-background);
      color: var(--vscode-input-foreground);
      border: 1px solid var(--vscode-input-border);
      font-family: var(--vscode-font-family);
      font-size: 13px;
    }
    .add-row input:focus { outline: 1px solid var(--vscode-focusBorder); }
    .force-input {
      width: 80px;
      padding: 2px 4px;
      background: var(--vscode-input-background);
      color: var(--vscode-input-foreground);
      border: 1px solid var(--vscode-input-border);
      font-family: var(--vscode-editor-font-family, monospace);
      font-size: 12px;
    }
    button {
      padding: 2px 8px;
      cursor: pointer;
      background: var(--vscode-button-background);
      color: var(--vscode-button-foreground);
      border: none;
      border-radius: 2px;
      font-size: 12px;
    }
    button:hover { background: var(--vscode-button-hoverBackground); }
    button.secondary {
      background: var(--vscode-button-secondaryBackground);
      color: var(--vscode-button-secondaryForeground);
    }
    button.secondary:hover { background: var(--vscode-button-secondaryHoverBackground); }
    .empty-msg {
      text-align: center;
      color: var(--vscode-descriptionForeground);
      padding: 12px;
    }
    .pending { color: var(--vscode-descriptionForeground); }
    .forced td.value { color: var(--vscode-charts-orange, #d18616); font-weight: bold; }
    .forced td.name::before {
      content: "🔒 ";
      color: var(--vscode-charts-orange, #d18616);
    }
    .force-error {
      border-color: var(--vscode-inputValidation-errorBorder, #be1100) !important;
      background: var(--vscode-inputValidation-errorBackground) !important;
    }
  </style>
</head>
<body>
  <h2>Scan Cycle</h2>
  <div class="stats">
    <span class="stat-label">Cycles:</span><span class="stat-value" id="s-cycles">0</span>
    <span class="stat-label">Last:</span><span class="stat-value" id="s-last">-</span>
    <span class="stat-label">Min:</span><span class="stat-value" id="s-min">-</span>
    <span class="stat-label">Max:</span><span class="stat-value" id="s-max">-</span>
    <span class="stat-label">Avg:</span><span class="stat-value" id="s-avg">-</span>
    <span class="stat-label" id="s-target-label" style="display:none">Target:</span>
    <span class="stat-value" id="s-target" style="display:none">-</span>
    <span class="stat-label" id="s-period-label" style="display:none">Period:</span>
    <span class="stat-value" id="s-period" style="display:none">-</span>
    <span class="stat-label" id="s-jitter-label" style="display:none">Jitter (max):</span>
    <span class="stat-value" id="s-jitter" style="display:none">-</span>
  </div>

  <h2>
    Watch List
    <span class="h-actions">
      <button class="secondary" onclick="clearAll()">Clear all</button>
    </span>
  </h2>
  <div class="add-row">
    <input
      id="add-input"
      list="catalog"
      placeholder="Add variable to watch (start typing for suggestions)..."
      autocomplete="off"
    />
    <datalist id="catalog"></datalist>
    <button onclick="addFromInput()">Add</button>
  </div>
  <table>
    <thead><tr><th>Name</th><th>Value</th><th>Type</th><th>Actions</th></tr></thead>
    <tbody id="var-body">
      <tr><td colspan="4" class="empty-msg">Watch list is empty. Add a variable above.</td></tr>
    </tbody>
  </table>

  <script>
    const vscode = acquireVsCodeApi();
    let catalog = ${initialCatalog};
    let watchList = ${initialWatchList};
    /** Latest values from the DAP, keyed by lowercased name. */
    let valueMap = new Map();
    /** name → type from the catalog, used as a fallback. */
    let typeMap = new Map();

    function fmtUs(us) {
      if (us >= 1000) {
        return (us / 1000).toFixed(us >= 10000 ? 0 : 1) + " ms";
      }
      return us + " \\u00b5s";
    }

    function show(id, visible) {
      document.getElementById(id).style.display = visible ? "" : "none";
    }

    function refreshCatalogDatalist() {
      const dl = document.getElementById("catalog");
      dl.innerHTML = catalog.map(c =>
        '<option value="' + c.name + '">' + c.type + '</option>'
      ).join("");
      typeMap = new Map(catalog.map(c => [c.name.toLowerCase(), c.type]));
    }

    /**
     * Build the table STRUCTURE from the current watchList. Called only
     * when the watch list itself changes (add/remove/clear). Each row gets
     * a stable data-var attribute so updateValueCells() can find it later
     * and mutate just the value/type cells without touching the input
     * elements — preserving focus and any half-typed force value.
     */
    function renderWatchTable() {
      const tbody = document.getElementById("var-body");
      if (watchList.length === 0) {
        tbody.innerHTML = '<tr><td colspan="4" class="empty-msg">' +
          'Watch list is empty. Add a variable above.</td></tr>';
        return;
      }
      tbody.innerHTML = watchList.map(name => {
        const lc = name.toLowerCase();
        const v = valueMap.get(lc);
        const value = v ? v.value : '<span class="pending">…</span>';
        const type = (v && v.type) || typeMap.get(lc) || '';
        const isForced = !!(v && v.forced);
        const safeName = name.replace(/'/g, "\\\\'");
        const placeholder = type ? placeholderForType(type) : "value";
        return '<tr data-var="' + lc + '"' + (isForced ? ' class="forced"' : '') + '>' +
          '<td class="name">' + name + '</td>' +
          '<td class="value">' + value + '</td>' +
          '<td class="type">' + type + '</td>' +
          '<td>' +
            '<input class="force-input" placeholder="' + placeholder + '" />' +
            ' <button onclick="forceVar(\\'' + safeName + '\\')">Force</button>' +
            ' <button class="secondary" onclick="unforceVar(\\'' + safeName + '\\')">Unforce</button>' +
            ' <button class="secondary" onclick="removeWatch(\\'' + safeName + '\\')">Remove</button>' +
          '</td>' +
        '</tr>';
      }).join("");
    }

    /**
     * In-place update of the value + type cells for every row in the
     * watch list. Does NOT touch the row structure or the force input —
     * the user can keep typing in a force field while the periodic
     * telemetry refresh updates surrounding cells.
     *
     * Also toggles the .forced row class so the lock icon + value
     * highlight appear/disappear immediately when force/unforce takes
     * effect on the runtime.
     */
    function updateValueCells() {
      for (const name of watchList) {
        const lc = name.toLowerCase();
        const v = valueMap.get(lc);
        if (!v) continue;
        const row = findRow(lc);
        if (!row) continue;
        const valueCell = row.querySelector(".value");
        const typeCell = row.querySelector(".type");
        if (valueCell && valueCell.textContent !== v.value) {
          valueCell.textContent = v.value;
        }
        if (typeCell && v.type && typeCell.textContent !== v.type) {
          typeCell.textContent = v.type;
        }
        const wasForced = row.classList.contains("forced");
        const isForced = !!v.forced;
        if (wasForced !== isForced) {
          row.classList.toggle("forced", isForced);
        }
      }
    }

    /**
     * Look up a row by its data-var attribute. We iterate the tbody
     * children directly instead of using querySelector with a CSS
     * attribute selector — that path requires escaping any CSS-special
     * chars in the attribute value, which has tripped us up before
     * (the previous polyfill produced double-backslashes that didn't
     * match). Linear scan is fine for watch lists of <100 entries.
     */
    function findRow(lc) {
      const tbody = document.getElementById("var-body");
      const rows = tbody.children;
      for (let i = 0; i < rows.length; i++) {
        if (rows[i].getAttribute && rows[i].getAttribute("data-var") === lc) {
          return rows[i];
        }
      }
      return null;
    }

    function placeholderForType(type) {
      const t = type.toUpperCase();
      if (t === "BOOL") return "TRUE / FALSE";
      if (t === "STRING" || t === "WSTRING") return "text";
      if (t === "REAL" || t === "LREAL") return "1.5";
      return "0";
    }

    /**
     * Validate a user-entered force value against the variable's
     * declared type. Returns the canonicalized value to send to the
     * DAP, or null if the input is invalid for this type.
     *
     * BOOL accepts true/false/0/1 (case insensitive).
     * Integer types accept signed decimals; we don't enforce range
     * because the DAP/VM clamps to the declared type at load time.
     * Float types accept decimals or integers.
     * STRING accepts any non-empty input.
     */
    function validateForceValue(type, raw) {
      if (!raw) return null;
      const t = (type || "").toUpperCase();
      if (t === "BOOL") {
        const lower = raw.toLowerCase();
        if (lower === "true" || lower === "1") return "true";
        if (lower === "false" || lower === "0") return "false";
        return null;
      }
      const intTypes = ["SINT", "USINT", "BYTE", "INT", "UINT", "WORD",
                        "DINT", "UDINT", "DWORD", "LINT", "ULINT", "LWORD"];
      if (intTypes.indexOf(t) !== -1) {
        if (!/^-?\\d+$/.test(raw)) return null;
        return raw;
      }
      if (t === "REAL" || t === "LREAL") {
        if (!/^-?\\d+(\\.\\d+)?([eE][+-]?\\d+)?$/.test(raw)) return null;
        return raw;
      }
      if (t === "STRING" || t === "WSTRING") {
        return raw;
      }
      // Unknown / complex type — accept as-is and let the DAP reject.
      return raw;
    }

    function addFromInput() {
      const input = document.getElementById("add-input");
      const name = input.value.trim();
      if (!name) return;
      if (!watchList.some(v => v.toLowerCase() === name.toLowerCase())) {
        watchList.push(name);
        renderWatchTable();
        vscode.postMessage({ command: "addWatch", variable: name });
      }
      input.value = "";
    }

    document.getElementById("add-input").addEventListener("keydown", function(e) {
      if (e.key === "Enter") {
        addFromInput();
        e.preventDefault();
      }
    });

    function removeWatch(name) {
      watchList = watchList.filter(v => v.toLowerCase() !== name.toLowerCase());
      valueMap.delete(name.toLowerCase());
      renderWatchTable();
      vscode.postMessage({ command: "removeWatch", variable: name });
    }

    function clearAll() {
      if (watchList.length === 0) return;
      watchList = [];
      valueMap.clear();
      renderWatchTable();
      vscode.postMessage({ command: "clearWatch" });
    }

    function forceVar(name) {
      const lc = name.toLowerCase();
      const row = findRow(lc);
      const input = row ? row.querySelector(".force-input") : null;
      const raw = input ? input.value.trim() : "";
      if (!raw) {
        if (input) input.focus();
        return;
      }
      // Validate against the declared type from the latest snapshot or
      // catalog. Reject incompatible input by flashing the input red and
      // refocusing — the user must correct it before the force is sent.
      const v = valueMap.get(lc);
      const type = (v && v.type) || typeMap.get(lc) || "";
      const canonical = validateForceValue(type, raw);
      if (canonical === null) {
        if (input) {
          input.classList.add("force-error");
          input.title = "Invalid value for type " + type;
          input.focus();
          input.select();
          setTimeout(function() {
            input.classList.remove("force-error");
            input.title = "";
          }, 1500);
        }
        return;
      }
      vscode.postMessage({ command: "force", variable: name, value: canonical });
      if (input) input.value = "";
    }

    function unforceVar(name) {
      vscode.postMessage({ command: "unforce", variable: name });
    }

    window.addEventListener("message", function(event) {
      const msg = event.data;
      if (msg.command === "updateCycleInfo") {
        const ci = msg.info;
        document.getElementById("s-cycles").textContent = ci.cycle_count.toLocaleString();
        document.getElementById("s-last").innerHTML = fmtUs(ci.last_cycle_us);
        document.getElementById("s-min").innerHTML = fmtUs(ci.min_cycle_us);
        document.getElementById("s-max").innerHTML = fmtUs(ci.max_cycle_us);
        document.getElementById("s-avg").innerHTML = fmtUs(ci.avg_cycle_us);
        if (ci.target_us != null) {
          show("s-target-label", true); show("s-target", true);
          document.getElementById("s-target").innerHTML = fmtUs(ci.target_us);
          show("s-period-label", true); show("s-period", true);
          document.getElementById("s-period").innerHTML = fmtUs(ci.last_period_us);
          show("s-jitter-label", true); show("s-jitter", true);
          document.getElementById("s-jitter").innerHTML = fmtUs(ci.jitter_max_us);
        }
      } else if (msg.command === "updateVariables") {
        for (const v of msg.variables) {
          valueMap.set(v.name.toLowerCase(), v);
        }
        // Only update text content on existing cells — DO NOT rebuild the
        // table, that would destroy the force input fields and steal
        // focus from the user mid-typing.
        updateValueCells();
      } else if (msg.command === "updateCatalog") {
        catalog = msg.catalog;
        refreshCatalogDatalist();
      } else if (msg.command === "updateWatchList") {
        watchList = msg.watchList;
        renderWatchTable();
      }
    });

    // Initial render with whatever the extension provided.
    refreshCatalogDatalist();
    renderWatchTable();
  </script>
</body>
</html>`;
  }

  private dispose() {
    MonitorPanel.currentPanel = undefined;
    this.panel.dispose();
    while (this.disposables.length) {
      const d = this.disposables.pop();
      if (d) d.dispose();
    }
  }
}
