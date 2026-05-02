import * as vscode from "vscode";
import * as fs from "fs";
import * as path from "path";

/**
 * WebView panel for PLC online monitoring.
 *
 * The webview HTML, CSS, and JS are separate files bundled by esbuild
 * (out/webview/). This class manages the WS connection, message
 * forwarding, persistence, and target management.
 */
export class MonitorPanel {
  public static currentPanel: MonitorPanel | undefined;
  private static workspaceState: vscode.Memento | undefined;
  private readonly panel: vscode.WebviewPanel;
  private readonly extensionUri: vscode.Uri;
  private disposables: vscode.Disposable[] = [];
  private catalog: Array<{ name: string; type: string }> = [];
  private watchList: string[] = [];
  private statusPollTimer: ReturnType<typeof setInterval> | undefined;

  /** Active WebSocket connection to the target agent. */
  private monitorWs: import("ws").WebSocket | undefined;
  /** Whether WebSocket monitoring is connected and streaming. */
  private wsMonitorActive = false;

  private constructor(panel: vscode.WebviewPanel, extensionUri: vscode.Uri) {
    this.panel = panel;
    this.extensionUri = extensionUri;
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
        localResourceRoots: [vscode.Uri.joinPath(extensionUri, "out", "webview")],
      }
    );

    MonitorPanel.currentPanel = new MonitorPanel(panel, extensionUri);
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
            this.watchList.push(name);
            this.saveWatchList();
          }
          // Subscribe to the FULL watch list so the server builds a
          // complete tree with all roots (not just the new one).
          this.wsSend({
            method: "subscribe",
            params: { variables: this.watchList, interval_ms: 0 },
          });
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
      case "trigger":
        if (typeof msg.variable === "string" && typeof msg.value === "string") {
          this.targetTrigger(msg.variable, msg.value);
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
        if (this.selectedTargetHost) {
          vscode.commands.executeCommand("structured-text.targetInstall", {
            host: this.selectedTargetHost,
            port: this.selectedTargetPort,
          });
        } else {
          vscode.commands.executeCommand("structured-text.targetInstall");
        }
        break;
      case "tb:upload":
        if (this.selectedTargetHost) {
          vscode.commands.executeCommand("structured-text.targetUpload", {
            host: this.selectedTargetHost,
            port: this.selectedTargetPort,
          });
        } else {
          vscode.commands.executeCommand("structured-text.targetUpload");
        }
        break;
      case "tb:onlineUpdate":
        if (this.selectedTargetHost) {
          vscode.commands.executeCommand("structured-text.targetOnlineUpdate", {
            host: this.selectedTargetHost,
            port: this.selectedTargetPort,
          });
        } else {
          vscode.commands.executeCommand("structured-text.targetOnlineUpdate");
        }
        break;
      case "tb:liveAttach":
        if (this.selectedTargetHost) {
          // The DAP proxy listens on agent_port + 1 by convention.
          vscode.commands.executeCommand("structured-text.targetLiveAttach", {
            host: this.selectedTargetHost,
            port: this.selectedTargetPort + 1,
          });
        } else {
          vscode.commands.executeCommand("structured-text.targetLiveAttach");
        }
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
      case "tb:fetchTargetInfo":
        this.fetchTargetInfo();
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
    // The DAP's embedded monitor server is a raw WS on the port root.
    // The target-agent's monitor is behind /api/v1/monitor/ws (axum route).
    const url = this.isLocalMonitor
      ? `ws://${host}:${port}`
      : `ws://${host}:${port}/api/v1/monitor/ws`;
    MonitorPanel.log(`Connecting WebSocket to ${url}`);

    try {
      const WebSocket = require("ws") as typeof import("ws");
      const ws = new WebSocket(url);
      this.monitorWs = ws;

      ws.on("open", () => {
        MonitorPanel.log(`WebSocket CONNECTED to ${url}`);
        this.wsMonitorActive = true;
        // 1. Get catalog for autocomplete
        MonitorPanel.log("Requesting catalog...");
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
        } else {
          MonitorPanel.log("Watch list is empty — no subscriptions to send");
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
          `WebSocket CLOSED (code=${code}, reason=${reason.toString() || "none"}) — url was ${url}`
        );
        this.wsMonitorActive = false;
        this.monitorWs = undefined;
      });

      ws.on("error", (err: Error) => {
        MonitorPanel.log(`WebSocket ERROR: ${err.message} — url was ${url}`);
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
          const tree = msg.watch_tree || [];
          MonitorPanel.log(
            `variableUpdate: ${vars.length} flat vars, ${tree.length} tree roots, cycle=${msg.cycle || 0}`
          );
          if (tree.length > 0) {
            for (const node of tree) {
              MonitorPanel.log(
                `  tree root: ${node.name} kind=${node.kind} type=${node.type} children=${(node.children || []).length}`
              );
            }
          }
          this.panel.webview.postMessage({
            command: "updateVariables",
            variables: vars,
            watchTree: tree,
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

  /** Trigger a variable (single-cycle force) via WebSocket. */
  private targetTrigger(variable: string, value: string) {
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
    MonitorPanel.log(`Trigger ${variable} = ${JSON.stringify(jsonValue)}`);
    this.wsSend({ method: "trigger", params: { variable, value: jsonValue } });
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

  /** Fetch target information (agent, program, status) and display in the info panel. */
  public async fetchTargetInfo() {
    if (!this.selectedTargetHost) {
      this.panel.webview.postMessage({
        command: "updateTargetInfo",
        info: { error: "No target selected. Choose a target from the dropdown." },
      });
      return;
    }

    const host = this.selectedTargetHost;
    const port = this.selectedTargetPort;
    const base = `http://${host}:${port}`;
    const info: any = {};

    // 1. Health check — is the agent reachable?
    try {
      const healthResp = await fetch(`${base}/api/v1/health`, {
        signal: AbortSignal.timeout(3000),
      });
      if (!healthResp.ok) {
        this.panel.webview.postMessage({
          command: "updateTargetInfo",
          info: { error: `Agent returned HTTP ${healthResp.status}. Runtime may need reinstalling.` },
        });
        return;
      }
    } catch {
      this.panel.webview.postMessage({
        command: "updateTargetInfo",
        info: {
          error: `Cannot reach ${host}:${port}. The PLC runtime is not installed or the target is offline.`,
        },
      });
      return;
    }

    // 2. Target info — OS, arch, version
    try {
      const tiResp = await fetch(`${base}/api/v1/target-info`, {
        signal: AbortSignal.timeout(3000),
      });
      if (tiResp.ok) {
        info.agent = await tiResp.json();
      }
    } catch {
      // Non-fatal — older agents may not have this endpoint
    }

    // 3. Program info — what's deployed?
    try {
      const progResp = await fetch(`${base}/api/v1/program/info`, {
        signal: AbortSignal.timeout(3000),
      });
      if (progResp.ok) {
        info.program = await progResp.json();
      }
      // 404 = no program deployed (expected, not an error)
    } catch {
      // Non-fatal
    }

    // 4. Runtime status
    try {
      const statusResp = await fetch(`${base}/api/v1/status`, {
        signal: AbortSignal.timeout(3000),
      });
      if (statusResp.ok) {
        const body = (await statusResp.json()) as any;
        info.status = body.status || "unknown";
      }
    } catch {
      // Non-fatal
    }

    this.panel.webview.postMessage({ command: "updateTargetInfo", info });
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
    this.stopStatusPolling();
    this.isLocalMonitor = host === "127.0.0.1" || host === "localhost";
    MonitorPanel.log(`connectToMonitor: ${label} → ws://${host}:${port}`);

    // Reset stale data from previous session — clear cached tree/values
    // but keep watchList intact so the WS connect can re-subscribe.
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

    // Sync the webview's watchList with the extension host's persisted list
    // so the webview renders the correct pending rows immediately.
    if (this.watchList.length > 0) {
      this.panel.webview.postMessage({
        command: "updateWatchList",
        watchList: this.watchList,
      });
    }
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

  // ── HTML (loaded from bundled files) ────────────────────────────────

  private setInitialHtml() {
    const webviewDir = vscode.Uri.joinPath(this.extensionUri, "out", "webview");
    const scriptUri = this.panel.webview.asWebviewUri(
      vscode.Uri.joinPath(webviewDir, "monitor.js")
    );
    const stylesUri = this.panel.webview.asWebviewUri(
      vscode.Uri.joinPath(webviewDir, "styles.css")
    );
    const nonce = getNonce();

    // Read the static HTML template
    const htmlPath = path.join(this.extensionUri.fsPath, "out", "webview", "index.html");
    let html = fs.readFileSync(htmlPath, "utf8");

    // Read the extension version from package.json
    const pkgPath = path.join(this.extensionUri.fsPath, "package.json");
    let version = "?";
    try {
      const pkg = JSON.parse(fs.readFileSync(pkgPath, "utf8"));
      version = pkg.version || "?";
    } catch { /* ignore */ }

    // Replace placeholders
    html = html.replace(/{{stylesUri}}/g, stylesUri.toString());
    html = html.replace(/{{scriptUri}}/g, scriptUri.toString());
    html = html.replace(/{{nonce}}/g, nonce);
    html = html.replace(/{{cspSource}}/g, this.panel.webview.cspSource);
    html = html.replace(
      "{{initialState}}",
      JSON.stringify({
        catalog: this.catalog,
        watchList: this.watchList,
        expandedNodes: this.loadExpandedNodes(),
        version,
      })
    );

    this.panel.webview.html = html;
  }

  // ── DELETED: inline HTML/CSS/JS template was here (970 lines)
  // Now loaded from out/webview/index.html + monitor.js + styles.css

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

/** Generate a random nonce for CSP. */
function getNonce(): string {
  let text = "";
  const possible = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
  for (let i = 0; i < 32; i++) {
    text += possible.charAt(Math.floor(Math.random() * possible.length));
  }
  return text;
}
