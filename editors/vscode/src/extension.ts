import * as path from "path";
import * as fs from "fs";
import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;
let cycleStatusBar: vscode.StatusBarItem | undefined;

interface PlcCycleStats {
  schema: number;
  cycle_count: number;
  last_us: number;
  min_us: number;
  max_us: number;
  avg_us: number;
  instructions_per_cycle: number;
  watchdog_us: number | null;
  devices_ok: number;
  devices_err: number;
  target_us: number | null;
  last_period_us: number;
  min_period_us: number;
  max_period_us: number;
  jitter_max_us: number;
}

function formatMicros(us: number): string {
  if (us >= 1000) {
    return `${(us / 1000).toFixed(us >= 10000 ? 0 : 1)}ms`;
  }
  return `${us}µs`;
}

function renderStatusBar(stats: PlcCycleStats) {
  if (!cycleStatusBar) {
    return;
  }
  const dots: string[] = [];
  for (let i = 0; i < stats.devices_ok; i++) dots.push("●");
  for (let i = 0; i < stats.devices_err; i++) dots.push("○");
  const commIndicator = dots.length > 0 ? `  ${dots.join("")}` : "";
  cycleStatusBar.text =
    `$(pulse) PLC  ${formatMicros(stats.last_us)}  ` +
    `#${stats.cycle_count.toLocaleString()}  ` +
    `${formatMicros(stats.min_us)}/${formatMicros(stats.max_us)}` +
    commIndicator;
  const jitterLine = stats.target_us
    ? `jitter: ${formatMicros(stats.jitter_max_us)} max · ` +
      `period: ${formatMicros(stats.last_period_us)} ` +
      `(${formatMicros(stats.min_period_us)}/${formatMicros(stats.max_period_us)})\n`
    : "";
  const targetLine = stats.target_us
    ? `target: ${formatMicros(stats.target_us)}\n`
    : "";
  cycleStatusBar.tooltip =
    `Scan cycle: last ${formatMicros(stats.last_us)} · avg ${formatMicros(stats.avg_us)}\n` +
    `min ${formatMicros(stats.min_us)} · max ${formatMicros(stats.max_us)}\n` +
    targetLine +
    jitterLine +
    `cycles ${stats.cycle_count.toLocaleString()} · ` +
    `${stats.instructions_per_cycle.toLocaleString()} instr/cycle\n` +
    (stats.devices_err > 0
      ? `comm: ${stats.devices_ok} ok / ${stats.devices_err} error`
      : `comm: ${stats.devices_ok} device(s) ok`);

  // Color coding against the watchdog budget (if set)
  if (stats.watchdog_us && stats.watchdog_us > 0) {
    const ratio = stats.max_us / stats.watchdog_us;
    if (ratio >= 1.0 || stats.devices_err > 0) {
      cycleStatusBar.backgroundColor = new vscode.ThemeColor("statusBarItem.errorBackground");
    } else if (ratio >= 0.75) {
      cycleStatusBar.backgroundColor = new vscode.ThemeColor("statusBarItem.warningBackground");
    } else {
      cycleStatusBar.backgroundColor = undefined;
    }
  } else if (stats.devices_err > 0) {
    cycleStatusBar.backgroundColor = new vscode.ThemeColor("statusBarItem.errorBackground");
  } else {
    cycleStatusBar.backgroundColor = undefined;
  }
  cycleStatusBar.show();
}

/**
 * DAP message tracker that sniffs `output` events with category `telemetry`
 * and an `output` field of `plc/cycleStats`. The structured payload lives in
 * the `data` field of the OutputEventBody.
 */
/// In-memory cache of the variable catalog from the most recent
/// `plc/varCatalog` event. The MonitorPanel pulls from this when it opens.
let plcVarCatalog: Array<{ name: string; type: string }> = [];

export function getPlcVarCatalog(): Array<{ name: string; type: string }> {
  return plcVarCatalog;
}

class PlcDapTracker implements vscode.DebugAdapterTracker {
  onDidSendMessage(message: any): void {
    if (
      message?.type !== "event" ||
      message?.event !== "output" ||
      message?.body?.category !== "telemetry"
    ) {
      return;
    }
    const sentinel = message.body.output;
    const data = message.body.data;
    if (!data) return;

    const { MonitorPanel } = require("./monitorPanel");

    if (sentinel === "plc/cycleStats") {
      const stats = data as PlcCycleStats;
      renderStatusBar(stats);
      if (MonitorPanel.currentPanel) {
        MonitorPanel.currentPanel.updateCycleInfo({
          cycle_count: stats.cycle_count,
          last_cycle_us: stats.last_us,
          min_cycle_us: stats.min_us,
          max_cycle_us: stats.max_us,
          avg_cycle_us: stats.avg_us,
          target_us: stats.target_us,
          jitter_max_us: stats.jitter_max_us,
          last_period_us: stats.last_period_us,
        });
        // Route watched variable snapshots to the monitor panel.
        const vars = (data as any).variables;
        if (Array.isArray(vars)) {
          MonitorPanel.currentPanel.updateVariables(vars);
        }
      }
      return;
    }

    if (sentinel === "plc/varCatalog") {
      const vars = (data as any).variables;
      if (Array.isArray(vars)) {
        plcVarCatalog = vars;
        if (MonitorPanel.currentPanel) {
          MonitorPanel.currentPanel.updateCatalog(plcVarCatalog);
        }
      }
      return;
    }
  }

  onWillStartSession(): void {
    plcVarCatalog = [];
  }
  onWillStopSession(): void {
    cycleStatusBar?.hide();
  }
  onError(): void {
    cycleStatusBar?.hide();
  }
  onExit(): void {
    cycleStatusBar?.hide();
  }
}

class PlcDapTrackerFactory implements vscode.DebugAdapterTrackerFactory {
  createDebugAdapterTracker(
    _session: vscode.DebugSession
  ): vscode.ProviderResult<vscode.DebugAdapterTracker> {
    return new PlcDapTracker();
  }
}

function resolveStCliPath(context: vscode.ExtensionContext): string {
  let serverPath = vscode.workspace
    .getConfiguration("structured-text")
    .get<string>("serverPath", "st-cli");

  // Resolve VSCode/devcontainer variables
  const folders = vscode.workspace.workspaceFolders;
  if (folders && folders.length > 0) {
    const wsPath = folders[0].uri.fsPath;
    serverPath = serverPath
      .replace(/\$\{workspaceFolder\}/g, wsPath)
      .replace(/\$\{containerWorkspaceFolder\}/g, wsPath);
  }

  // If not an absolute path, try to find it relative to the extension
  if (!path.isAbsolute(serverPath) && !serverPath.includes(path.sep)) {
    const devBinary = path.resolve(
      context.extensionPath,
      "..",
      "..",
      "target",
      "debug",
      "st-cli"
    );
    if (fs.existsSync(devBinary)) {
      serverPath = devBinary;
    }
  }

  return serverPath;
}

export function activate(context: vscode.ExtensionContext) {
  const stCliPath = resolveStCliPath(context);

  // ── LSP Client ───────────────────────────────────────────────────
  const serverOptions: ServerOptions = {
    command: stCliPath,
    args: ["serve"],
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "structured-text" }],
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher("**/*.{st,scl}"),
    },
  };

  client = new LanguageClient(
    "structured-text",
    "Structured Text Language Server",
    serverOptions,
    clientOptions
  );

  client.start().then(
    () => {},
    (err: Error) => {
      vscode.window.showErrorMessage(
        `Failed to start ST language server: ${err.message}.\n` +
        `Build it with: cargo build -p st-cli`
      );
    }
  );

  // ── Debug Adapter ────────────────────────────────────────────────
  const debugAdapterFactory = new StDebugAdapterFactory(stCliPath);
  context.subscriptions.push(
    vscode.debug.registerDebugAdapterDescriptorFactory("st", debugAdapterFactory)
  );

  // ── Cycle-time status bar (Tier 2 cycle-time feedback) ───────────
  cycleStatusBar = vscode.window.createStatusBarItem(
    vscode.StatusBarAlignment.Right,
    100
  );
  cycleStatusBar.command = "structured-text.openMonitor";
  cycleStatusBar.text = "$(pulse) PLC";
  context.subscriptions.push(cycleStatusBar);

  context.subscriptions.push(
    vscode.debug.registerDebugAdapterTrackerFactory("st", new PlcDapTrackerFactory())
  );

  // Hide the widget when the last `st` debug session ends.
  context.subscriptions.push(
    vscode.debug.onDidTerminateDebugSession((session) => {
      if (session.type === "st") {
        cycleStatusBar?.hide();
      }
    })
  );

  // ── Monitor Panel ─────────────────────────────────────────────────
  // Hand the workspace state to the panel so the watch list survives
  // panel close / window reload (scoped per workspace folder).
  const { MonitorPanel } = require("./monitorPanel");
  MonitorPanel.setWorkspaceState(context.workspaceState);
  context.subscriptions.push(
    vscode.commands.registerCommand("structured-text.openMonitor", () => {
      MonitorPanel.createOrShow(context.extensionUri);
      // If we already cached a catalog from an earlier launch event, push
      // it into the panel immediately so the autocomplete is populated.
      if (MonitorPanel.currentPanel && plcVarCatalog.length > 0) {
        MonitorPanel.currentPanel.updateCatalog(plcVarCatalog);
      }
    })
  );

  // ── PLC Debug Toolbar Commands ───────────────────────────────────
  context.subscriptions.push(
    vscode.commands.registerCommand("structured-text.forceVariable", async () => {
      const session = vscode.debug.activeDebugSession;
      if (!session || session.type !== "st") return;
      const input = await vscode.window.showInputBox({
        prompt: "Force variable (e.g., counter = 42)",
        placeHolder: "variable_name = value",
      });
      if (input) {
        const result = await session.customRequest("evaluate", {
          expression: `force ${input}`,
          context: "repl",
        });
        vscode.window.showInformationMessage(result.result);
      }
    }),
    vscode.commands.registerCommand("structured-text.unforceVariable", async () => {
      const session = vscode.debug.activeDebugSession;
      if (!session || session.type !== "st") return;
      const input = await vscode.window.showInputBox({
        prompt: "Variable name to unforce",
        placeHolder: "variable_name",
      });
      if (input) {
        const result = await session.customRequest("evaluate", {
          expression: `unforce ${input}`,
          context: "repl",
        });
        vscode.window.showInformationMessage(result.result);
      }
    }),
    vscode.commands.registerCommand("structured-text.listForced", async () => {
      const session = vscode.debug.activeDebugSession;
      if (!session || session.type !== "st") return;
      const result = await session.customRequest("evaluate", {
        expression: "listForced",
        context: "repl",
      });
      vscode.window.showInformationMessage(result.result);
    }),
    vscode.commands.registerCommand("structured-text.cycleInfo", async () => {
      const session = vscode.debug.activeDebugSession;
      if (!session || session.type !== "st") return;
      const result = await session.customRequest("evaluate", {
        expression: "scanCycleInfo",
        context: "repl",
      });
      vscode.window.showInformationMessage(result.result);
    })
  );

  // ── Cleanup ──────────────────────────────────────────────────────
  context.subscriptions.push({
    dispose: () => {
      client?.stop();
    },
  });
}

export function deactivate(): Thenable<void> | undefined {
  return client?.stop();
}

/**
 * Spawns `st-cli debug <file>` as the debug adapter process.
 */
class StDebugAdapterFactory implements vscode.DebugAdapterDescriptorFactory {
  constructor(private stCliPath: string) {}

  createDebugAdapterDescriptor(
    session: vscode.DebugSession,
    _executable: vscode.DebugAdapterExecutable | undefined
  ): vscode.ProviderResult<vscode.DebugAdapterDescriptor> {
    const config = session.configuration;
    const program = config.program || "";

    return new vscode.DebugAdapterExecutable(this.stCliPath, ["debug", program]);
  }
}
