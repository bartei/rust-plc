import * as vscode from "vscode";

/**
 * WebView panel for PLC online monitoring.
 *
 * Connects to a WebSocket monitor server (embedded in the DAP for local
 * debug, or on the remote target agent) and displays real-time scan-cycle
 * stats + a user-managed watch list of variables with force/unforce.
 *
 * Watch lists are persisted to workspace state so they survive panel close
 * and reload. Each project (keyed by workspace folder) has its own watch list.
 */
export class MonitorPanel {
  public static currentPanel: MonitorPanel | undefined;
  private static workspaceState: vscode.Memento | undefined;
  private readonly panel: vscode.WebviewPanel;
  private disposables: vscode.Disposable[] = [];
  private catalog: Array<{ name: string; type: string }> = [];
  private watchList: string[] = [];
  private statusPollTimer: ReturnType<typeof setInterval> | undefined;

  /** Active WebSocket connection to the target agent. */
  private monitorWs: import("ws").WebSocket | undefined;
  /** Whether WebSocket monitoring is connected and streaming. */
  private wsMonitorActive = false;

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
  }

  private handleWebviewMessage(msg: any) {
    switch (msg.command) {
      case "addWatch":
        if (typeof msg.variable === "string" && msg.variable.trim()) {
          const name = msg.variable.trim();
          if (!this.watchList.some((v) => v.toLowerCase() === name.toLowerCase())) {
            // Validate against catalog
            const valid = this.catalog.some(
              (c) => c.name.toLowerCase() === name.toLowerCase()
            );
            if (!valid && this.catalog.length > 0) {
              vscode.window.showWarningMessage(
                `Variable "${name}" not found in program catalog`
              );
              break;
            }
            this.watchList.push(name);
            this.saveWatchList();
            this.wsSend({
              method: "subscribe",
              params: { variables: [name], interval_ms: 0 },
            });
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
            this.wsSend({
              method: "unsubscribe",
              params: { variables: [msg.variable] },
            });
          }
        }
        break;
      case "clearWatch":
        if (this.watchList.length > 0) {
          this.wsSend({
            method: "unsubscribe",
            params: { variables: [...this.watchList] },
          });
        }
        this.watchList = [];
        this.saveWatchList();
        break;
      case "force":
        if (typeof msg.variable === "string" && typeof msg.value === "string") {
          this.targetForce(msg.variable, msg.value);
        }
        break;
      case "unforce":
        if (typeof msg.variable === "string") {
          this.targetUnforce(msg.variable);
        }
        break;
      case "resetStats":
        MonitorPanel.log("Sending resetStats");
        this.wsSend({ method: "resetStats" });
        break;
      case "expandedNodesChanged":
        if (Array.isArray(msg.nodes)) {
          this.saveExpandedNodes(msg.nodes);
        }
        break;

      // ── Deployment toolbar commands ──────────────────────────
      case "tb:install":
        vscode.commands.executeCommand("structured-text.targetInstall");
        break;
      case "tb:upload":
        vscode.commands.executeCommand("structured-text.targetUpload");
        break;
      case "tb:onlineUpdate":
        vscode.commands.executeCommand("structured-text.targetOnlineUpdate");
        break;
      case "tb:run":
        vscode.commands.executeCommand("structured-text.targetRun");
        break;
      case "tb:stop":
        vscode.commands.executeCommand("structured-text.targetStop");
        break;
      case "tb:selectTarget":
        if (msg.host) {
          this.selectedTargetHost = msg.host;
          this.selectedTargetPort = msg.agentPort || 4840;
          this.stopWsMonitoring();
          this.startStatusPolling();
        } else {
          this.selectedTargetHost = undefined;
          this.stopWsMonitoring();
          this.stopStatusPolling();
          this.updateTargetStatus("offline");
        }
        break;
      case "tb:refreshTargets":
        vscode.commands.executeCommand("structured-text.refreshMonitorTargets");
        break;
    }
  }

  /** Update the target status indicator in the toolbar. */
  public updateTargetStatus(status: string, targetName?: string) {
    this.panel.webview.postMessage({
      command: "updateTargetStatus",
      status,
      targetName: targetName || "",
    });
  }

  /** Poll the selected target's /api/v1/status endpoint and update toolbar.
   *  When the target is running, opens a WebSocket for real-time monitoring. */
  public async pollTargetStatus() {
    if (!this.selectedTargetHost) {
      this.updateTargetStatus("offline");
      this.stopWsMonitoring();
      return;
    }
    const host = this.selectedTargetHost;
    const port = this.selectedTargetPort;
    try {
      const resp = await fetch(
        `http://${host}:${port}/api/v1/status`,
        { signal: AbortSignal.timeout(3000) }
      );
      if (resp.ok) {
        const body = await resp.json() as any;
        const status = (body.status || body.runtime_status || "idle").toLowerCase();
        if (status === "running" || status === "debugpaused") {
          this.updateTargetStatus("running", host);
          MonitorPanel.log(
            `Target ${host}:${port} is running (cycle=${body.cycle_stats?.cycle_count ?? 0}, wsActive=${this.wsMonitorActive})`
          );
          // Push cycle stats from the HTTP status response as a baseline
          // (WS will push fresher data once connected)
          if (body.cycle_stats) {
            const cs = body.cycle_stats;
            this.updateCycleInfo({
              cycle_count: cs.cycle_count || 0,
              last_cycle_us: cs.last_cycle_time_us || 0,
              min_cycle_us: cs.min_cycle_time_us || 0,
              max_cycle_us: cs.max_cycle_time_us || 0,
              avg_cycle_us: cs.avg_cycle_time_us || 0,
              target_us: null,
              jitter_max_us: 0,
              last_period_us: 0,
            });
          }
          // Open WebSocket if not already connected (and not in local debug mode)
          if (!this.wsMonitorActive && !this.monitorWs && !this.isLocalMonitor) {
            this.connectToMonitor(host, port, host);
          }
        } else {
          this.updateTargetStatus("idle", host);
          this.stopWsMonitoring();
        }
      } else {
        this.updateTargetStatus("error", host);
        this.stopWsMonitoring();
      }
    } catch {
      this.updateTargetStatus("offline", host);
      this.stopWsMonitoring();
    }
  }

  // ── WebSocket Variable Monitoring ─────────────────────────────────

  private static log(msg: string) {
    console.log(`[PLC-Monitor] ${msg}`);
  }

  /** Open a WebSocket to the target agent for real-time variable monitoring. */
  public startWsMonitoring() {
    if (this.monitorWs) return; // already attempting/connected
    if (!this.selectedTargetHost) return;

    const host = this.selectedTargetHost;
    const port = this.selectedTargetPort;
    const url = `ws://${host}:${port}/api/v1/monitor/ws`;
    MonitorPanel.log(`Connecting WebSocket to ${url}`);

    try {
      const WebSocket = require("ws") as typeof import("ws");
      const ws = new WebSocket(url);
      this.monitorWs = ws;

      ws.on("open", () => {
        MonitorPanel.log("WebSocket connected — requesting catalog");
        this.wsMonitorActive = true;
        // 1. Get catalog for autocomplete
        this.wsSend({ method: "getCatalog" });
        // 2. Subscribe to the persisted watch list
        if (this.watchList.length > 0) {
          MonitorPanel.log(
            `Subscribing to ${this.watchList.length} variables: ${this.watchList.join(", ")}`
          );
          this.wsSend({
            method: "subscribe",
            params: { variables: this.watchList, interval_ms: 0 },
          });
        }
        // 3. Get initial cycle info
        this.wsSend({ method: "getCycleInfo" });
      });

      ws.on("message", (data: import("ws").RawData) => {
        try {
          const msg = JSON.parse(data.toString());
          this.handleWsMessage(msg);
        } catch (e: any) {
          MonitorPanel.log(`Failed to parse WS message: ${e.message}`);
        }
      });

      ws.on("close", (code: number, reason: Buffer) => {
        MonitorPanel.log(
          `WebSocket closed (code=${code}, reason=${reason.toString() || "none"})`
        );
        this.wsMonitorActive = false;
        this.monitorWs = undefined;
      });

      ws.on("error", (err: Error) => {
        MonitorPanel.log(`WebSocket error: ${err.message}`);
        this.wsMonitorActive = false;
        this.monitorWs = undefined;
      });
    } catch (e: any) {
      MonitorPanel.log(`WebSocket connect failed: ${e.message || e}`);
    }
  }

  /** Close the WebSocket connection. */
  public stopWsMonitoring() {
    if (this.monitorWs) {
      MonitorPanel.log("Closing WebSocket");
      try { this.monitorWs.close(); } catch { /* ignore */ }
      this.monitorWs = undefined;
    }
    this.wsMonitorActive = false;
  }

  /** Send a JSON message over the WebSocket. */
  private wsSend(msg: any) {
    if (this.monitorWs && this.monitorWs.readyState === 1 /* OPEN */) {
      this.monitorWs.send(JSON.stringify(msg));
    }
  }

  /** Handle an incoming message from the WebSocket. */
  private handleWsMessage(msg: any) {
    switch (msg.type) {
      case "variableUpdate":
        if (Array.isArray(msg.variables)) {
          const vars = msg.variables.map((v: any) => ({
            name: v.name,
            value: v.value,
            type: v.type,
            forced: !!v.forced,
          }));
          this.panel.webview.postMessage({
            command: "updateVariables",
            variables: vars,
          });
          // Cycle stats are included in every variableUpdate
          this.updateCycleInfo({
            cycle_count: msg.cycle || 0,
            last_cycle_us: msg.last_cycle_us || 0,
            min_cycle_us: msg.min_cycle_us || 0,
            max_cycle_us: msg.max_cycle_us || 0,
            avg_cycle_us: msg.avg_cycle_us || 0,
            target_us: msg.target_cycle_us || null,
            jitter_max_us: msg.jitter_max_us || 0,
            last_period_us: msg.last_period_us || 0,
          });
        }
        break;
      case "catalog":
        if (Array.isArray(msg.variables)) {
          this.catalog = msg.variables.map((v: any) => ({
            name: v.name,
            type: v.type,
          }));
          MonitorPanel.log(`Catalog received: ${this.catalog.length} variables`);
          this.panel.webview.postMessage({
            command: "updateCatalog",
            catalog: this.catalog,
          });
        }
        break;
      case "cycleInfo":
        this.updateCycleInfo({
          cycle_count: msg.cycle_count || 0,
          last_cycle_us: msg.last_cycle_us || 0,
          min_cycle_us: msg.min_cycle_us || 0,
          max_cycle_us: msg.max_cycle_us || 0,
          avg_cycle_us: msg.avg_cycle_us || 0,
          target_us: null,
          jitter_max_us: 0,
          last_period_us: 0,
        });
        break;
      case "response":
        MonitorPanel.log(
          `WS response: success=${msg.success}${msg.data ? " data=" + JSON.stringify(msg.data) : ""}`
        );
        break;
      case "error":
        MonitorPanel.log(`WS error: ${msg.message}`);
        vscode.window.showWarningMessage(`Monitor: ${msg.message}`);
        break;
      default:
        MonitorPanel.log(`Unknown WS message type: ${msg.type}`);
    }
  }

  /** Force a variable via WebSocket. */
  private targetForce(variable: string, value: string) {
    if (!this.wsMonitorActive) {
      vscode.window.showWarningMessage("Monitor: not connected to target");
      return;
    }
    let jsonValue: any = value;
    if (value === "true" || value === "false") {
      jsonValue = value === "true";
    } else if (/^-?\d+$/.test(value)) {
      jsonValue = parseInt(value, 10);
    } else if (/^-?\d+(\.\d+)?([eE][+-]?\d+)?$/.test(value)) {
      jsonValue = parseFloat(value);
    }
    MonitorPanel.log(`Force ${variable} = ${JSON.stringify(jsonValue)}`);
    this.wsSend({ method: "force", params: { variable, value: jsonValue } });
  }

  /** Unforce a variable via WebSocket. */
  private targetUnforce(variable: string) {
    if (!this.wsMonitorActive) {
      vscode.window.showWarningMessage("Monitor: not connected to target");
      return;
    }
    MonitorPanel.log(`Unforce ${variable}`);
    this.wsSend({ method: "unforce", params: { variable } });
  }

  /** Start periodic status polling (every 5s). */
  private startStatusPolling() {
    this.stopStatusPolling();
    MonitorPanel.log(
      `Status polling started for ${this.selectedTargetHost}:${this.selectedTargetPort}`
    );
    this.pollTargetStatus(); // immediate first poll
    this.statusPollTimer = setInterval(() => this.pollTargetStatus(), 5000);
  }

  /** Stop periodic status polling. */
  private stopStatusPolling() {
    if (this.statusPollTimer) {
      clearInterval(this.statusPollTimer);
      this.statusPollTimer = undefined;
    }
  }

  /** Populate the target dropdown from plc-project.yaml targets. */
  public setTargets(targets: Array<{ name: string; host: string; agentPort: number }>) {
    this.panel.webview.postMessage({
      command: "setTargets",
      targets,
    });
  }

  /** The currently selected target host (set by the webview dropdown). */
  public selectedTargetHost: string | undefined;
  public selectedTargetPort: number = 4840;

  // ── Unified Monitor Connection ───────────────────────────────────

  /** Whether this is a local debug monitor (auto-disconnects on session end). */
  private isLocalMonitor = false;

  /**
   * Connect the Monitor panel to a WebSocket monitor server.
   * Called by the extension when a DAP session starts (local debug) or
   * when the user selects a remote target and it's running.
   */
  public connectToMonitor(host: string, port: number, label: string) {
    // Disconnect any existing connection first
    this.stopWsMonitoring();
    this.isLocalMonitor = host === "127.0.0.1" || host === "localhost";
    MonitorPanel.log(`connectToMonitor: ${label} → ws://${host}:${port}`);

    // Reset stale data from previous session
    this.panel.webview.postMessage({ command: "resetSession" });
    this.panel.webview.postMessage({
      command: "updateTargetStatus",
      status: "running",
      targetName: label,
    });

    // Stash host/port so startWsMonitoring can use them
    this.selectedTargetHost = host;
    this.selectedTargetPort = port;
    this.startWsMonitoring();
  }

  /**
   * Disconnect the local debug monitor (called when the debug session ends).
   * Does NOT disconnect a remote target connection.
   */
  public disconnectLocalMonitor() {
    if (!this.isLocalMonitor) return;
    MonitorPanel.log("Local debug session ended — disconnecting monitor");
    this.stopWsMonitoring();
    this.isLocalMonitor = false;
    this.panel.webview.postMessage({
      command: "updateTargetStatus",
      status: "offline",
      targetName: "",
    });
  }

  // ── persistence ────────────────────────────────────────────────────

  private workspaceKey(): string {
    const folder = vscode.workspace.workspaceFolders?.[0];
    const root = folder ? folder.uri.fsPath : "<no-workspace>";
    return `plcMonitor.watchList:${root}`;
  }

  private expandedKey(): string {
    const folder = vscode.workspace.workspaceFolders?.[0];
    const root = folder ? folder.uri.fsPath : "<no-workspace>";
    return `plcMonitor.expandedNodes:${root}`;
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

  private loadExpandedNodes(): string[] {
    const state = MonitorPanel.workspaceState;
    if (!state) return [];
    const v = state.get<string[]>(this.expandedKey());
    return Array.isArray(v) ? v : [];
  }

  private saveExpandedNodes(nodes: string[]) {
    const state = MonitorPanel.workspaceState;
    if (!state) return;
    void state.update(this.expandedKey(), nodes);
  }

  // ── HTML ───────────────────────────────────────────────────────────

  private setInitialHtml() {
    const initialCatalog = JSON.stringify(this.catalog);
    const initialWatchList = JSON.stringify(this.watchList);
    const initialExpanded = JSON.stringify(this.loadExpandedNodes());
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
      position: relative;
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
    .tree-toggle {
      cursor: pointer;
      font-size: 11px;
      user-select: none;
      display: inline-block;
      width: 12px;
    }
    .tree-child td.name { padding-left: 28px; }
    .autocomplete-dropdown {
      display: none;
      position: absolute;
      top: 100%;
      left: 0;
      right: 48px;
      max-height: 200px;
      overflow-y: auto;
      background: var(--vscode-editorSuggestWidget-background, var(--vscode-editor-background));
      border: 1px solid var(--vscode-editorSuggestWidget-border, var(--vscode-panel-border));
      z-index: 1000;
      font-size: 13px;
    }
    .autocomplete-dropdown.visible { display: block; }
    .autocomplete-item {
      padding: 4px 8px;
      cursor: pointer;
      display: flex;
      justify-content: space-between;
    }
    .autocomplete-item:hover, .autocomplete-item.selected {
      background: var(--vscode-list-hoverBackground);
    }
    .autocomplete-item .item-type {
      color: var(--vscode-descriptionForeground);
      font-style: italic;
      margin-left: 12px;
    }
    .forced td.value { color: var(--vscode-charts-orange, #d18616); font-weight: bold; }
    .forced td.name::before {
      content: "🔒 ";
      color: var(--vscode-charts-orange, #d18616);
    }
    .force-error {
      border-color: var(--vscode-inputValidation-errorBorder, #be1100) !important;
      background: var(--vscode-inputValidation-errorBackground) !important;
    }
    /* ── Deployment Toolbar ─────────────────────────────────── */
    .deploy-toolbar {
      display: flex;
      align-items: center;
      gap: 4px;
      padding: 6px 0 10px 0;
      border-bottom: 1px solid var(--vscode-panel-border);
      margin-bottom: 10px;
      flex-wrap: wrap;
    }
    .deploy-toolbar .tb-group {
      display: flex;
      gap: 3px;
      align-items: center;
    }
    .deploy-toolbar .tb-sep {
      width: 1px;
      height: 20px;
      background: var(--vscode-panel-border);
      margin: 0 6px;
    }
    .deploy-toolbar button {
      padding: 3px 8px;
      font-size: 11px;
      display: flex;
      align-items: center;
      gap: 4px;
      white-space: nowrap;
    }
    .deploy-toolbar button .tb-icon {
      font-size: 13px;
      line-height: 1;
    }
    .deploy-toolbar button:disabled {
      opacity: 0.5;
      cursor: default;
    }
    .deploy-toolbar .tb-status {
      font-size: 11px;
      margin-left: auto;
      display: flex;
      align-items: center;
      gap: 6px;
      color: var(--vscode-descriptionForeground);
    }
    #tb-target-select {
      font-size: 11px;
      font-family: var(--vscode-font-family);
      background: var(--vscode-dropdown-background);
      color: var(--vscode-dropdown-foreground);
      border: 1px solid var(--vscode-dropdown-border);
      border-radius: 2px;
      padding: 2px 4px;
      cursor: pointer;
      max-width: 180px;
    }
    .tb-status .tb-dot {
      display: inline-block;
      width: 8px;
      height: 8px;
      border-radius: 50%;
      background: var(--vscode-descriptionForeground);
    }
    .tb-dot.running { background: var(--vscode-charts-green, #4caf50); }
    .tb-dot.stopped { background: var(--vscode-descriptionForeground); }
    .tb-dot.error   { background: var(--vscode-charts-red, #f44336); }
    .tb-dot.offline { background: var(--vscode-descriptionForeground); opacity: 0.4; }
  </style>
</head>
<body>
  <div class="deploy-toolbar" id="deploy-toolbar">
    <div class="tb-group">
      <button onclick="tbInstall()" title="Install or upgrade the PLC runtime on the target">
        <span class="tb-icon">&#x2B07;</span> Install
      </button>
    </div>
    <div class="tb-sep"></div>
    <div class="tb-group">
      <button onclick="tbUpload()" title="Upload PLC program to target (offline update — stops the program)">
        <span class="tb-icon">&#x2191;</span> Upload
      </button>
      <button onclick="tbOnlineUpdate()" title="Online update — hot-reload without stopping (when possible)">
        <span class="tb-icon">&#x21BB;</span> Online
      </button>
    </div>
    <div class="tb-sep"></div>
    <div class="tb-group">
      <button onclick="tbRun()" id="tb-run" title="Start or restart the PLC program on the target">
        <span class="tb-icon">&#x25B6;</span> Run
      </button>
      <button onclick="tbStop()" id="tb-stop" title="Stop the PLC program on the target">
        <span class="tb-icon">&#x25A0;</span> Stop
      </button>
    </div>
    <div class="tb-status" id="tb-status">
      <span class="tb-dot offline" id="tb-dot"></span>
      <select id="tb-target-select" onchange="onTargetChange(this)" title="Select deployment target">
        <option value="">-- No target --</option>
      </select>
      <button onclick="tbRefreshTargets()" title="Reload targets from plc-project.yaml" style="background:none;border:none;color:var(--vscode-descriptionForeground);cursor:pointer;font-size:12px;padding:0 2px;">&#x21BB;</button>
      <span id="tb-status-text"></span>
    </div>
  </div>

  <h2>
    Scan Cycle
    <span class="h-actions">
      <button class="secondary" onclick="resetStats()">Reset Stats</button>
    </span>
  </h2>
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
      placeholder="Add variable to watch (start typing for suggestions)..."
      autocomplete="off"
    />
    <div id="autocomplete-dropdown" class="autocomplete-dropdown"></div>
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
    /** Latest values keyed by lowercased name. */
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

    let selectedIdx = -1;
    function refreshCatalogDatalist() {
      typeMap = new Map(catalog.map(c => [c.name.toLowerCase(), c.type]));
    }

    function showDropdown() {
      const input = document.getElementById("add-input");
      const dd = document.getElementById("autocomplete-dropdown");
      const query = input.value.trim().toLowerCase();
      if (!query || catalog.length === 0) {
        dd.classList.remove("visible");
        return;
      }
      const matches = catalog.filter(c => c.name.toLowerCase().includes(query));
      if (matches.length === 0) {
        dd.classList.remove("visible");
        return;
      }
      selectedIdx = -1;
      dd.innerHTML = matches.slice(0, 50).map((c, i) =>
        '<div class="autocomplete-item" data-name="' + c.name + '" data-idx="' + i + '">' +
          '<span>' + c.name + '</span>' +
          '<span class="item-type">' + c.type + '</span>' +
        '</div>'
      ).join("");
      dd.classList.add("visible");
      dd.querySelectorAll(".autocomplete-item").forEach(item => {
        item.addEventListener("mousedown", function(e) {
          e.preventDefault();
          input.value = this.getAttribute("data-name");
          dd.classList.remove("visible");
          addFromInput();
        });
      });
    }

    document.getElementById("add-input").addEventListener("input", showDropdown);
    document.getElementById("add-input").addEventListener("focus", showDropdown);
    document.getElementById("add-input").addEventListener("blur", function() {
      setTimeout(function() {
        document.getElementById("autocomplete-dropdown").classList.remove("visible");
      }, 150);
    });

    /**
     * Build the table STRUCTURE from the current watchList. Called only
     * when the watch list itself changes (add/remove/clear). Each row gets
     * a stable data-var attribute so updateValueCells() can find it later
     * and mutate just the value/type cells without touching the input
     * elements — preserving focus and any half-typed force value.
     */
    /** Set of expanded tree nodes (lowercased full paths). Initialized
     *  from persisted state so the tree survives panel close / reload. */
    let expandedNodes = new Set(${initialExpanded});

    /**
     * Build a tree from flat dotted-path prefix matching in valueMap.
     */
    function buildSubTree(prefix) {
      const lc = prefix.toLowerCase();
      const prefixLc = lc + ".";
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
        node[leaf] = { __value: v, __children: node[leaf] ? node[leaf].__children : null };
      });
      return tree;
    }


    function renderTree(tree, parentPath, depth) {
      let html = "";
      const indent = "\\u00a0\\u00a0\\u00a0\\u00a0".repeat(depth);
      const sortedKeys = Object.keys(tree).sort();
      for (const key of sortedKeys) {
        const entry = tree[key];
        const fullPath = parentPath + "." + key;
        const fullLc = fullPath.toLowerCase();
        const hasChildren = entry.__children && Object.keys(entry.__children).length > 0;
        const isExpanded = expandedNodes.has(fullLc);
        const v = entry.__value;

        if (hasChildren) {
          // Intermediate node (FB instance) — expandable
          const toggle = '<span class="tree-toggle" onclick="toggleNode(\\'' +
            fullPath.replace(/'/g, "\\\\'") + '\\')">' +
            (isExpanded ? '\\u25BE' : '\\u25B8') + '</span> ';
          const value = v ? v.value : '';
          const type = v ? v.type : '';
          html += '<tr data-var="' + fullLc + '">' +
            '<td class="name">' + indent + toggle + key + '</td>' +
            '<td class="value">' + value + '</td>' +
            '<td class="type"><i>' + type + '</i></td>' +
            '<td></td></tr>';
          if (isExpanded) {
            html += renderTree(entry.__children, fullPath, depth + 1);
          }
        } else if (v) {
          // Leaf node (scalar field) — with force controls
          const isForced = !!v.forced;
          const leafSafe = fullPath.replace(/'/g, "\\\\'");
          const leafPlaceholder = v.type ? placeholderForType(v.type) : "value";
          html += '<tr data-var="' + fullLc + '"' + (isForced ? ' class="forced"' : '') + '>' +
            '<td class="name">' + indent + '\\u00a0\\u00a0 ' + key + '</td>' +
            '<td class="value">' + v.value + '</td>' +
            '<td class="type"><i>' + v.type + '</i></td>' +
            '<td>' +
              '<input class="force-input" placeholder="' + leafPlaceholder + '" />' +
              ' <button onclick="forceVar(\\'' + leafSafe + '\\')">Force</button>' +
              ' <button class="secondary" onclick="unforceVar(\\'' + leafSafe + '\\')">Unforce</button>' +
            '</td></tr>';
        }
      }
      return html;
    }

    function renderWatchTable() {
      const tbody = document.getElementById("var-body");
      if (watchList.length === 0) {
        tbody.innerHTML = '<tr><td colspan="4" class="empty-msg">' +
          'Watch list is empty. Add a variable above.</td></tr>';
        return;
      }
      let html = "";
      for (const name of watchList) {
        const lc = name.toLowerCase();
        const v = valueMap.get(lc);
        const safeName = name.replace(/'/g, "\\\\'");

        // Check if there are descendant values under this prefix
        const tree = buildSubTree(name);
        const hasChildren = Object.keys(tree).length > 0;
        const isExpanded = expandedNodes.has(lc);

        if (hasChildren) {
          // FB instance or prefix with children — show tree toggle
          const toggle = '<span class="tree-toggle" onclick="toggleNode(\\'' +
            safeName + '\\')">' + (isExpanded ? '\\u25BE' : '\\u25B8') + '</span> ';
          const value = v ? v.value : '';
          const type = v ? v.type : '';
          html += '<tr data-var="' + lc + '">' +
            '<td class="name">' + toggle + name + '</td>' +
            '<td class="value">' + value + '</td>' +
            '<td class="type"><i>' + type + '</i></td>' +
            '<td>' +
              '<button class="secondary" onclick="removeWatch(\\'' + safeName + '\\')">Remove</button>' +
            '</td></tr>';
          if (isExpanded) {
            html += renderTree(tree, name, 1);
          }
        } else {
          // Scalar variable — flat row with force controls
          const value = v ? v.value : '<span class="pending">\\u2026</span>';
          const type = (v && v.type) || typeMap.get(lc) || '';
          const isForced = !!(v && v.forced);
          const placeholder = type ? placeholderForType(type) : "value";
          html += '<tr data-var="' + lc + '"' + (isForced ? ' class="forced"' : '') + '>' +
            '<td class="name">' + name + '</td>' +
            '<td class="value">' + value + '</td>' +
            '<td class="type"><i>' + type + '</i></td>' +
            '<td>' +
              '<input class="force-input" placeholder="' + placeholder + '" />' +
              ' <button onclick="forceVar(\\'' + safeName + '\\')">Force</button>' +
              ' <button class="secondary" onclick="unforceVar(\\'' + safeName + '\\')">Unforce</button>' +
              ' <button class="secondary" onclick="removeWatch(\\'' + safeName + '\\')">Remove</button>' +
            '</td></tr>';
        }
      }
      tbody.innerHTML = html;
    }

    function toggleNode(name) {
      const lc = name.toLowerCase();
      if (expandedNodes.has(lc)) {
        expandedNodes.delete(lc);
      } else {
        expandedNodes.add(lc);
      }
      renderWatchTable();
      // Persist expanded state to the extension host.
      vscode.postMessage({
        command: "expandedNodesChanged",
        nodes: Array.from(expandedNodes)
      });
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
      // Update both watch-list rows AND any expanded child rows
      const tbody = document.getElementById("var-body");
      const rows = tbody.children;
      for (let i = 0; i < rows.length; i++) {
        const row = rows[i];
        const lc = row.getAttribute && row.getAttribute("data-var");
        if (!lc) continue;
        const v = valueMap.get(lc);
        if (!v) continue;
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
      const dd = document.getElementById("autocomplete-dropdown");
      const items = dd.querySelectorAll(".autocomplete-item");
      if (e.key === "ArrowDown" && dd.classList.contains("visible")) {
        e.preventDefault();
        selectedIdx = Math.min(selectedIdx + 1, items.length - 1);
        items.forEach((el, i) => el.classList.toggle("selected", i === selectedIdx));
        if (items[selectedIdx]) items[selectedIdx].scrollIntoView({ block: "nearest" });
      } else if (e.key === "ArrowUp" && dd.classList.contains("visible")) {
        e.preventDefault();
        selectedIdx = Math.max(selectedIdx - 1, 0);
        items.forEach((el, i) => el.classList.toggle("selected", i === selectedIdx));
        if (items[selectedIdx]) items[selectedIdx].scrollIntoView({ block: "nearest" });
      } else if (e.key === "Enter") {
        e.preventDefault();
        if (selectedIdx >= 0 && selectedIdx < items.length) {
          this.value = items[selectedIdx].getAttribute("data-name");
          dd.classList.remove("visible");
        }
        addFromInput();
      } else if (e.key === "Escape") {
        dd.classList.remove("visible");
      }
    });

    function removeWatch(name) {
      watchList = watchList.filter(v => v.toLowerCase() !== name.toLowerCase());
      valueMap.delete(name.toLowerCase());
      renderWatchTable();
      vscode.postMessage({ command: "removeWatch", variable: name });
    }

    // ── Deployment Toolbar Handlers ─────────────────────────────
    function tbInstall() {
      vscode.postMessage({ command: "tb:install" });
    }
    function tbUpload() {
      vscode.postMessage({ command: "tb:upload" });
    }
    function tbOnlineUpdate() {
      vscode.postMessage({ command: "tb:onlineUpdate" });
    }
    function tbRun() {
      vscode.postMessage({ command: "tb:run" });
    }
    function tbStop() {
      vscode.postMessage({ command: "tb:stop" });
    }

    function tbRefreshTargets() {
      vscode.postMessage({ command: "tb:refreshTargets" });
    }

    /** Handle target dropdown change. */
    function onTargetChange(select) {
      const opt = select.options[select.selectedIndex];
      const host = opt.dataset.host || "";
      const port = parseInt(opt.dataset.port || "4840", 10);
      vscode.postMessage({ command: "tb:selectTarget", host, agentPort: port });
      // Reset status when switching targets
      updateToolbarStatus("offline", host ? opt.textContent : "");
    }

    /** Populate the target dropdown from extension data. */
    function populateTargets(targets) {
      const select = document.getElementById("tb-target-select");
      if (!select) return;
      // Preserve current selection if possible
      const current = select.value;
      select.innerHTML = '<option value="">-- No target --</option>';
      for (const t of targets) {
        const opt = document.createElement("option");
        opt.value = t.host;
        opt.textContent = t.name + " (" + t.host + ":" + t.agentPort + ")";
        opt.dataset.host = t.host;
        opt.dataset.port = String(t.agentPort);
        select.appendChild(opt);
      }
      // Restore previous selection, or auto-select if only one target
      if (current && [...select.options].some(o => o.value === current)) {
        select.value = current;
      } else if (targets.length === 1) {
        select.value = targets[0].host;
      }
      // Notify extension of the selection so it polls status
      if (select.value) {
        onTargetChange(select);
      }
    }

    /** Update the status indicator in the toolbar. */
    function updateToolbarStatus(status, targetName) {
      const dot = document.getElementById("tb-dot");
      const text = document.getElementById("tb-status-text");
      if (!dot || !text) return;

      dot.className = "tb-dot";
      if (status === "running") {
        dot.classList.add("running");
        text.textContent = targetName ? targetName + " — Running" : "Running";
      } else if (status === "idle" || status === "stopped") {
        dot.classList.add("stopped");
        text.textContent = targetName ? targetName + " — Stopped" : "Stopped";
      } else if (status === "error") {
        dot.classList.add("error");
        text.textContent = targetName ? targetName + " — Error" : "Error";
      } else {
        dot.classList.add("offline");
        text.textContent = targetName || "No target";
      }

      // Enable/disable Run/Stop buttons based on state
      const runBtn = document.getElementById("tb-run");
      const stopBtn = document.getElementById("tb-stop");
      if (runBtn) runBtn.disabled = (status === "running");
      if (stopBtn) stopBtn.disabled = (status !== "running");
    }

    function resetStats() {
      vscode.postMessage({ command: "resetStats" });
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
        const prevSize = valueMap.size;
        for (const v of msg.variables) {
          valueMap.set(v.name.toLowerCase(), v);
        }
        // If new variable keys appeared (e.g., first telemetry after adding
        // a FB watch), rebuild the table structure so the tree can form.
        // Otherwise just update values in-place to avoid focus loss.
        if (valueMap.size !== prevSize) {
          renderWatchTable();
        } else {
          updateValueCells();
        }
      } else if (msg.command === "resetSession") {
        // New debug session — clear stale values from the previous session
        // so the panel rebuilds when the first telemetry tick arrives.
        valueMap.clear();
  
        renderWatchTable();
      } else if (msg.command === "updateCatalog") {
        catalog = msg.catalog;
        refreshCatalogDatalist();
      } else if (msg.command === "updateWatchList") {
        watchList = msg.watchList;
        renderWatchTable();
      } else if (msg.command === "updateTargetStatus") {
        updateToolbarStatus(msg.status, msg.targetName);
      } else if (msg.command === "setTargets") {
        populateTargets(msg.targets || []);
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
    this.stopStatusPolling();
    this.stopWsMonitoring();
    MonitorPanel.currentPanel = undefined;
    this.panel.dispose();
    while (this.disposables.length) {
      const d = this.disposables.pop();
      if (d) d.dispose();
    }
  }
}
