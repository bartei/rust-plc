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
let updateStatusBar: vscode.StatusBarItem | undefined;

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

/** Active local debug monitor port (0 = no active session). */
let localMonitorPort = 0;

/**
 * DAP message tracker: forwards cycle stats to the status bar, handles
 * source path remapping for remote debug, and picks up the monitor WS
 * port from the embedded DAP monitor server.
 */
class PlcDapTracker implements vscode.DebugAdapterTracker {
  onWillReceiveMessage(message: any): void {
    if (message?.type === "request") {
      console.log(`[DAP-TRACKER] → ${message.command} (seq=${message.seq})`);
    }
  }

  onDidSendMessage(message: any): void {
    if (message?.type === "response") {
      console.log(`[DAP-TRACKER] ← ${message.command} success=${message.success}`);
    }
    if (message?.type === "event") {
      console.log(`[DAP-TRACKER] ← event: ${message.event}`);
    }

    // Source path remapping is handled adapter-side via localRoot/remoteRoot.
    // This tracker only forwards telemetry events.

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
      // Status bar only — the Monitor panel gets data via WebSocket now.
      const stats = data as PlcCycleStats;
      renderStatusBar(stats);
      return;
    }

    if (sentinel === "plc/monitorPort") {
      // The DAP server started an embedded WS monitor on this port.
      // Store it so the Monitor panel can connect whenever it opens.
      const port = (data as any).port;
      if (typeof port === "number" && port > 0) {
        console.log(`[DAP-TRACKER] Monitor WS port received: ${port}`);
        localMonitorPort = port;
        if (MonitorPanel.currentPanel) {
          MonitorPanel.currentPanel.connectToMonitor("127.0.0.1", port, "Local Debug");
        }
      }
      return;
    }
  }

  onWillStartSession(): void {
    // Nothing to reset — catalog/variables flow through WebSocket now
  }
  onWillStopSession(): void {
    cycleStatusBar?.hide();
  }
  onError(error: Error): void {
    console.log(`[DAP-TRACKER] ERROR: ${error.message}`);
    cycleStatusBar?.hide();
  }
  onExit(code: number | undefined, signal: string | undefined): void {
    console.log(`[DAP-TRACKER] EXIT: code=${code} signal=${signal}`);
    cycleStatusBar?.hide();
  }
}

class PlcDapTrackerFactory implements vscode.DebugAdapterTrackerFactory {
  createDebugAdapterTracker(): vscode.ProviderResult<vscode.DebugAdapterTracker> {
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

  // If the resolved path exists, use it directly
  if (path.isAbsolute(serverPath) && fs.existsSync(serverPath)) {
    return serverPath;
  }

  // Search order for the binary:
  // 1. Relative to extension (dev layout: extension is symlinked from editors/vscode)
  // 2. /usr/local/bin/st-cli (devcontainer post-create symlink)
  // 3. Fall through with the original name (PATH lookup by LanguageClient)
  const candidates = [
    path.resolve(context.extensionPath, "..", "..", "target", "container", "debug", "st-cli"),
    path.resolve(context.extensionPath, "..", "..", "target", "debug", "st-cli"),
    "/usr/local/bin/st-cli",
  ];
  for (const candidate of candidates) {
    if (fs.existsSync(candidate)) {
      return candidate;
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
        `Failed to start ST language server (${stCliPath}): ${err.message}.\n` +
        `Build with: cargo build -p st-cli, or set structured-text.serverPath in settings.`
      );
    }
  );

  // ── Debug Adapter ────────────────────────────────────────────────
  const debugAdapterFactory = new StDebugAdapterFactory(stCliPath);
  context.subscriptions.push(
    vscode.debug.registerDebugAdapterDescriptorFactory("st", debugAdapterFactory)
  );

  // Dynamic debug configuration provider:
  // - provideDebugConfigurations: populates launch.json when created
  // - resolveDebugConfiguration: intercepts F5 with no launch.json and
  //   shows a quick-pick (local file vs remote targets)
  context.subscriptions.push(
    vscode.debug.registerDebugConfigurationProvider("st", {
      provideDebugConfigurations(): vscode.DebugConfiguration[] {
        const configs: vscode.DebugConfiguration[] = [
          {
            type: "st",
            request: "launch",
            name: "Debug Current File",
            program: "${file}",
            stopOnEntry: true,
          },
        ];
        for (const t of getTargetsFromConfig()) {
          configs.push({
            type: "st",
            request: "attach",
            name: `Debug on ${t.name} (${t.host})`,
            target: t.name,
            stopOnEntry: true,
          });
        }
        return configs;
      },
      async resolveDebugConfiguration(
        folder,
        config
      ): Promise<vscode.DebugConfiguration | undefined> {
        // If the user already has a full config (from launch.json), inject
        // localRoot for attach configs that don't have one explicitly set.
        if (config.request === "attach" && !config.localRoot) {
          config.localRoot = folder?.uri.fsPath
            ?? vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;
        }
        if (config.request) {
          console.log(
            `[ST-DEBUG] resolveDebugConfiguration: request=${config.request} ` +
            `target=${config.target || "none"} host=${config.host || "none"} ` +
            `port=${config.port || "default"} localRoot=${config.localRoot || "none"} ` +
            `stopOnEntry=${config.stopOnEntry}`
          );
          return config;
        }
        // No launch.json or empty config — show a quick-pick
        const targets = getTargetsFromConfig();
        const items: Array<{ label: string; description: string; config: vscode.DebugConfiguration }> = [
          {
            label: "$(file) Debug Current File",
            description: "Launch locally",
            config: {
              type: "st",
              request: "launch",
              name: "Debug Current File",
              program: "${file}",
              stopOnEntry: true,
            },
          },
        ];
        for (const t of targets) {
          items.push({
            label: `$(remote) ${t.name}`,
            description: `${t.host}:${t.agentPort + 1}`,
            config: {
              type: "st",
              request: "attach",
              name: `Debug on ${t.name}`,
              target: t.name,
              stopOnEntry: true,
              localRoot: "${workspaceFolder}",
            },
          });
        }
        if (items.length === 1) {
          // No targets configured — just launch locally
          return items[0].config;
        }
        const pick = await vscode.window.showQuickPick(items, {
          placeHolder: "Select debug target",
          title: "Debug Structured Text",
        });
        return pick?.config;
      },
    })
  );

  // ── Cycle-time status bar (Tier 2 cycle-time feedback) ───────────
  cycleStatusBar = vscode.window.createStatusBarItem(
    vscode.StatusBarAlignment.Right,
    100
  );
  cycleStatusBar.command = "structured-text.openMonitor";
  cycleStatusBar.text = "$(pulse) PLC";
  context.subscriptions.push(cycleStatusBar);

  // ── Update button: deploys + hot-applies the program to the active
  //    target. Visible only when the workspace defines at least one target
  //    in plc-project.yaml; the user can still find the underlying command
  //    in the command palette (`ST: Online Update PLC Program`).
  updateStatusBar = vscode.window.createStatusBarItem(
    vscode.StatusBarAlignment.Right,
    99
  );
  updateStatusBar.command = "structured-text.targetOnlineUpdate";
  updateStatusBar.text = "$(cloud-upload) ST Update";
  updateStatusBar.tooltip = "Build the current project and apply it to the active PLC target (online change when compatible, otherwise a clean restart).";
  context.subscriptions.push(updateStatusBar);
  refreshUpdateStatusBarVisibility();

  // Re-evaluate visibility when the workspace changes: opening folders,
  // editing plc-project.yaml, or switching the active editor.
  context.subscriptions.push(
    vscode.workspace.onDidChangeWorkspaceFolders(() => refreshUpdateStatusBarVisibility()),
    vscode.window.onDidChangeActiveTextEditor(() => refreshUpdateStatusBarVisibility()),
    vscode.workspace.onDidSaveTextDocument((doc) => {
      const base = path.basename(doc.fileName);
      if (base === "plc-project.yaml" || base === "plc-project.yml") {
        refreshUpdateStatusBarVisibility();
      }
    }),
  );

  context.subscriptions.push(
    vscode.debug.registerDebugAdapterTrackerFactory("st", new PlcDapTrackerFactory())
  );

  // Hide the widget when the last `st` debug session ends.
  context.subscriptions.push(
    vscode.debug.onDidTerminateDebugSession((session) => {
      if (session.type === "st") {
        console.log(
          `[ST-DEBUG] Session terminated: ${session.name} (${session.configuration.request})`
        );
        cycleStatusBar?.hide();
        localMonitorPort = 0;
        // Disconnect the local monitor WS (the DAP server is shutting down)
        if (MonitorPanel.currentPanel) {
          MonitorPanel.currentPanel.disconnectLocalMonitor();
        }
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
      if (MonitorPanel.currentPanel) {
        // Push targets from plc-project.yaml into the panel dropdown.
        const targets = getTargetsFromConfig();
        MonitorPanel.currentPanel.setTargets(targets);
        // If a local debug session is active, connect to its monitor server.
        if (localMonitorPort > 0) {
          MonitorPanel.currentPanel.connectToMonitor(
            "127.0.0.1", localMonitorPort, "Local Debug"
          );
        }
      }
    })
  );

  // ── Refresh Monitor targets command ──────────────────────────────
  context.subscriptions.push(
    vscode.commands.registerCommand("structured-text.refreshMonitorTargets", () => {
      const { MonitorPanel } = require("./monitorPanel");
      if (MonitorPanel.currentPanel) {
        const targets = getTargetsFromConfig();
        MonitorPanel.currentPanel.setTargets(targets);
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

  // ── Deployment toolbar commands ──────────────────────────────────
  context.subscriptions.push(
    vscode.commands.registerCommand("structured-text.targetInstall", async (arg?: { host: string; port: number }) => {
      const target = arg
        ? await resolveTargetFromArg(arg)
        : await resolveActiveTargetFull("Install PLC Runtime");
      if (!target) return;
      const sshTarget = `${target.user}@${target.host}`;
      vscode.window.showInformationMessage(`Installing PLC runtime on ${target.name} (${target.host})...`);
      const terminal = vscode.window.createTerminal(`PLC Install — ${target.name}`);
      terminal.show();
      // Match the SSH options st-deploy uses (see crates/st-deploy/src/ssh.rs):
      // host key verification fully disabled because targets get reflashed
      // routinely during dev and would otherwise trip strict checking.
      const sshOpts = "-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o GlobalKnownHostsFile=/dev/null -o LogLevel=ERROR";
      terminal.sendText(
        `st-cli target install ${sshTarget} && echo "\\n--- Rebooting ${target.host} ---" && ssh ${sshOpts} ${sshTarget} "sudo reboot"`
      );
    }),

    vscode.commands.registerCommand("structured-text.targetUpload", async (arg?: { host: string; port: number }) => {
      const resolved = arg
        ? { host: arg.host, port: arg.port }
        : await resolveActiveTargetWithPort("Upload PLC Program");
      if (!resolved) return;
      vscode.window.showInformationMessage(`Uploading to ${resolved.host}:${resolved.port}...`);
      const terminal = vscode.window.createTerminal(`PLC Upload — ${resolved.host}`);
      terminal.show();
      terminal.sendText(`st-cli bundle && curl -X POST -F "file=@$(ls -t *.st-bundle | head -1)" http://${resolved.host}:${resolved.port}/api/v1/program/upload`);
    }),

    vscode.commands.registerCommand("structured-text.targetOnlineUpdate", async (arg?: { host: string; port: number; bundlePath?: string }) => {
      const resolved = arg
        ? { host: arg.host, port: arg.port }
        : await resolveActiveTargetWithPort("Online Update");
      if (!resolved) return undefined;
      const { host, port } = resolved;

      // Build a fresh bundle if the caller didn't hand one in.
      let bundlePath = arg?.bundlePath;
      if (!bundlePath) {
        const built = await buildBundleInWorkspace();
        if (!built) return undefined;
        bundlePath = built;
      }

      vscode.window.showInformationMessage(`Updating PLC program on ${host}:${port}...`);
      try {
        const result = await postProgramUpdate(host, port, bundlePath);
        const summary = formatUpdateResult(result);
        vscode.window.showInformationMessage(`Update applied: ${summary}`);
        const { MonitorPanel } = require("./monitorPanel");
        if (MonitorPanel.currentPanel) MonitorPanel.currentPanel.pollTargetStatus();
        return result;
      } catch (e: any) {
        vscode.window.showErrorMessage(`Update failed: ${e.message}`);
        return undefined;
      }
    }),

    vscode.commands.registerCommand(
      "structured-text.targetLiveAttach",
      async (arg?: { host?: string; port?: number; localRoot?: string }): Promise<vscode.DebugSession | undefined> => {
        // Resolve host + DAP port: prefer explicit args (the Monitor panel
        // toolbar passes them), then fall back to the active target.
        let host: string | undefined = arg?.host;
        let dapPort: number | undefined = arg?.port;
        if (!host) {
          const resolved = await resolveActiveTargetWithPort("Live Attach Debugger");
          if (!resolved) return undefined;
          host = resolved.host;
          // resolveActiveTargetWithPort returns the AGENT port; the DAP
          // proxy listens on agent_port + 1, matching the convention used
          // by the manual attach launch.json snippet.
          dapPort = resolved.port + 1;
        }

        // Sensible default for localRoot — the workspace folder. Without
        // it, source-mapped breakpoints would not resolve to the agent's
        // extracted source paths.
        const localRoot = arg?.localRoot
          ?? vscode.workspace.workspaceFolders?.[0]?.uri.fsPath;

        const config: vscode.DebugConfiguration = {
          type: "st",
          name: `Live Attach — ${host}:${dapPort}`,
          request: "attach",
          host,
          port: dapPort,
          stopOnEntry: false,
          ...(localRoot ? { localRoot } : {}),
        };

        const sessionPromise = new Promise<vscode.DebugSession | undefined>((resolve) => {
          const startD = vscode.debug.onDidStartDebugSession((s) => {
            startD.dispose();
            resolve(s);
          });
          // Safety: don't wait forever if the session fails to start.
          setTimeout(() => { startD.dispose(); resolve(undefined); }, 6000);
        });

        const launched = await vscode.debug.startDebugging(
          vscode.workspace.workspaceFolders?.[0],
          config,
        );
        if (!launched) {
          vscode.window.showErrorMessage(
            `Live Attach failed to start (host=${host}, port=${dapPort}).`,
          );
          return undefined;
        }
        const session = await sessionPromise;
        if (session) {
          vscode.window.showInformationMessage(
            `Live Attach connected to ${host}:${dapPort} — execution continues, breakpoints active`,
          );
        }
        return session;
      },
    ),

    vscode.commands.registerCommand("structured-text.targetRun", async () => {
      const { host, port } = await resolveActiveTargetWithPort("Start PLC Program") || {};
      if (!host) return;
      try {
        const resp = await fetch(`http://${host}:${port}/api/v1/program/start`, { method: "POST" });
        if (resp.ok) {
          vscode.window.showInformationMessage("PLC program started");
        } else {
          const body = await resp.json().catch(() => ({}));
          vscode.window.showErrorMessage(`Start failed: ${(body as any).error || resp.statusText}`);
        }
      } catch (e: any) {
        vscode.window.showErrorMessage(`Cannot reach target: ${e.message}`);
      }
      // Poll actual status from the target
      if (MonitorPanel.currentPanel) {
        MonitorPanel.currentPanel.pollTargetStatus();
      }
    }),

    vscode.commands.registerCommand("structured-text.targetStop", async () => {
      const { host, port } = await resolveActiveTargetWithPort("Stop PLC Program") || {};
      if (!host) return;
      try {
        const resp = await fetch(`http://${host}:${port}/api/v1/program/stop`, { method: "POST" });
        if (resp.ok) {
          vscode.window.showInformationMessage("PLC program stopped");
        } else {
          const body = await resp.json().catch(() => ({}));
          vscode.window.showErrorMessage(`Stop failed: ${(body as any).error || resp.statusText}`);
        }
      } catch (e: any) {
        vscode.window.showErrorMessage(`Cannot reach target: ${e.message}`);
      }
      // Poll actual status from the target
      if (MonitorPanel.currentPanel) {
        MonitorPanel.currentPanel.pollTargetStatus();
      }
    })
  );

  // ── Cleanup ──────────────────────────────────────────────────────
  context.subscriptions.push({
    dispose: () => {
      client?.stop();
    },
  });
}

/** Parsed target entry from plc-project.yaml. */
interface TargetEntry {
  name: string;
  host: string;
  agentPort: number;
  user: string;
}

/**
 * Read targets from plc-project.yaml in the workspace.
 * Simple YAML extraction — no dependency on a YAML parser.
 */
function getTargetsFromConfig(): TargetEntry[] {
  // Search for plc-project.yaml in multiple locations:
  // 1. Workspace root
  // 2. Active editor's directory (and parents up to 5 levels)
  // 3. All workspace folders
  const searchDirs: string[] = [];
  for (const f of vscode.workspace.workspaceFolders || []) {
    searchDirs.push(f.uri.fsPath);
  }
  // Walk up from active editor's file
  const activeFile = vscode.window.activeTextEditor?.document.uri.fsPath;
  if (activeFile) {
    let dir = path.dirname(activeFile);
    for (let i = 0; i < 6; i++) {
      if (!searchDirs.includes(dir)) searchDirs.push(dir);
      const parent = path.dirname(dir);
      if (parent === dir) break;
      dir = parent;
    }
  }

  for (const dirPath of searchDirs) {
  for (const yamlName of ["plc-project.yaml", "plc-project.yml"]) {
    const p = path.join(dirPath, yamlName);
    if (!fs.existsSync(p)) continue;
    try {
      const text: string = fs.readFileSync(p, "utf8");
      const targets: TargetEntry[] = [];
      const lines = text.split("\n");
      let inTargets = false;
      let current: Partial<TargetEntry> = {};
      /** Strip YAML inline comments and surrounding quotes/whitespace. */
      const cleanVal = (raw: string) =>
        raw.replace(/\s+#.*$/, "").trim().replace(/^["']|["']$/g, "");
      for (const line of lines) {
        if (/^targets:/.test(line)) { inTargets = true; continue; }
        if (inTargets && /^\S/.test(line)) { inTargets = false; }
        if (!inTargets) continue;
        const nameMatch = line.match(/^\s+-\s*name:\s*(.+)/);
        if (nameMatch) {
          if (current.name && current.host) {
            targets.push({
              name: current.name,
              host: current.host,
              agentPort: current.agentPort || 4840,
              user: current.user || "plc",
            });
          }
          current = { name: cleanVal(nameMatch[1]) };
          continue;
        }
        const hostMatch = line.match(/host:\s*(.+)/);
        if (hostMatch) current.host = cleanVal(hostMatch[1]);
        const portMatch = line.match(/agent_port:\s*(\d+)/);
        if (portMatch) current.agentPort = parseInt(portMatch[1], 10);
        const userMatch = line.match(/user:\s*(.+)/);
        if (userMatch) current.user = cleanVal(userMatch[1]);
      }
      if (current.name && current.host) {
        targets.push({
          name: current.name,
          host: current.host,
          agentPort: current.agentPort || 4840,
          user: current.user || "plc",
        });
      }
      return targets;
    } catch {
      continue;
    }
  }
  }
  return [];
}

/**
 * Resolve a target name to host + DAP port from plc-project.yaml.
 */
function resolveTarget(targetName: string): { host: string; dapPort: number } | undefined {
  const targets = getTargetsFromConfig();
  const t = targets.find(t => t.name === targetName);
  if (!t) return undefined;
  return { host: t.host, dapPort: t.agentPort + 1 };
}

/**
 * Resolve a target from an explicit arg (passed by the monitor panel toolbar).
 * Matches against configured targets to get the user/name, or falls back to defaults.
 */
async function resolveTargetFromArg(arg: { host: string; port: number }): Promise<TargetEntry> {
  const targets = getTargetsFromConfig();
  const match = targets.find((t: TargetEntry) => t.host === arg.host);
  return match || { name: arg.host, host: arg.host, agentPort: arg.port, user: "plc" };
}

/**
 * Resolve the active target with agent port. Returns { host, port }.
 */
async function resolveActiveTargetWithPort(title: string): Promise<{ host: string; port: number } | undefined> {
  const { MonitorPanel } = require("./monitorPanel");
  if (MonitorPanel.currentPanel?.selectedTargetHost) {
    return {
      host: MonitorPanel.currentPanel.selectedTargetHost,
      port: MonitorPanel.currentPanel.selectedTargetPort,
    };
  }
  const targets = getTargetsFromConfig();
  const host = await pickOrInputTarget(targets, title);
  if (!host) return undefined;
  const t = targets.find((t: TargetEntry) => t.host === host);
  return { host, port: t?.agentPort || 4840 };
}

/**
 * Resolve the active target as a full TargetEntry (including user).
 * Uses the Monitor panel's dropdown selection, falls back to quick-pick.
 */
async function resolveActiveTargetFull(title: string): Promise<TargetEntry | undefined> {
  const targets = getTargetsFromConfig();
  const { MonitorPanel } = require("./monitorPanel");
  const selectedHost = MonitorPanel.currentPanel?.selectedTargetHost;
  if (selectedHost) {
    const t = targets.find((t: TargetEntry) => t.host === selectedHost);
    if (t) return t;
  }
  const host = await pickOrInputTarget(targets, title);
  if (!host) return undefined;
  return targets.find((t: TargetEntry) => t.host === host) || {
    name: host,
    host,
    agentPort: 4840,
    user: "plc",
  };
}

/**
 * Resolve the active target: use the Monitor panel's dropdown selection if
 * available, otherwise fall back to the quick-pick / input box flow.
 */
async function resolveActiveTarget(title: string): Promise<string | undefined> {
  const { MonitorPanel } = require("./monitorPanel");
  if (MonitorPanel.currentPanel?.selectedTargetHost) {
    return MonitorPanel.currentPanel.selectedTargetHost;
  }
  const targets = getTargetsFromConfig();
  return pickOrInputTarget(targets, title);
}

/**
 * Show a quick-pick with known targets, or an input box if none configured.
 * Returns the host string of the selected target.
 */
async function pickOrInputTarget(targets: TargetEntry[], title: string): Promise<string | undefined> {
  if (targets.length > 0) {
    const items = targets.map(t => ({
      label: t.name,
      description: `${t.host}:${t.agentPort}`,
      host: t.host,
    }));
    items.push({ label: "$(add) Enter manually...", description: "", host: "" });
    const pick = await vscode.window.showQuickPick(items, { title, placeHolder: "Select target" });
    if (!pick) return undefined;
    if (pick.label.includes("Enter manually")) {
      return vscode.window.showInputBox({ prompt: "Target host (IP or hostname)", title });
    }
    return pick.host;
  }
  return vscode.window.showInputBox({ prompt: "Target host (IP or hostname)", placeHolder: "192.168.1.50", title });
}

export function deactivate(): Thenable<void> | undefined {
  return client?.stop();
}

/**
 * Show the "ST Update" status bar item only when the workspace has at
 * least one target configured (i.e., `targets:` section in
 * plc-project.yaml resolves to ≥1 entry). Hidden otherwise so we don't
 * clutter the bar in unrelated workspaces.
 */
function refreshUpdateStatusBarVisibility(): void {
  if (!updateStatusBar) return;
  const targets = getTargetsFromConfig();
  if (targets.length === 0) {
    updateStatusBar.hide();
    return;
  }
  // When more than one target is configured, the click goes through the
  // existing target picker (handled by `resolveActiveTargetWithPort`); we
  // just nudge the user towards the active one in the tooltip.
  const active = targets[0];
  updateStatusBar.tooltip = targets.length === 1
    ? `Update ${active.name} (${active.host}:${active.agentPort})`
    : `Update PLC target — ${targets.length} configured, will prompt`;
  updateStatusBar.show();
}

// ─── Online update helpers ────────────────────────────────────────────────

/**
 * Result reported by the agent's `POST /api/v1/program/update` endpoint.
 */
export interface ProgramUpdateResult {
  success: boolean;
  method: "online_change" | "restart" | "cold_replace" | "initial_deploy";
  downtime_ms: number;
  program: { name: string; version: string; mode: string; bytecode_checksum: string };
  online_change?: { preserved_vars: string[]; new_vars: string[]; removed_vars: string[] };
}

function formatUpdateResult(r: ProgramUpdateResult): string {
  switch (r.method) {
    case "online_change":
      return `online change in ${r.downtime_ms}ms (${r.online_change?.preserved_vars.length ?? 0} vars preserved)`;
    case "restart":
      return `full restart in ${r.downtime_ms}ms`;
    case "cold_replace":
      return "cold replace (engine was idle)";
    case "initial_deploy":
      return "initial deploy (no previous program)";
  }
}

/**
 * Build a `.st-bundle` from the workspace by shelling out to `st-cli bundle`,
 * then return the path to the most recent bundle. Returns `undefined` if the
 * build fails or no workspace folder is open.
 */
async function buildBundleInWorkspace(): Promise<string | undefined> {
  const folder = vscode.workspace.workspaceFolders?.[0];
  if (!folder) {
    vscode.window.showErrorMessage("No workspace folder open — cannot build bundle.");
    return undefined;
  }
  const cwd = folder.uri.fsPath;

  return await new Promise<string | undefined>((resolve) => {
    const cp = require("child_process") as typeof import("child_process");
    const proc = cp.spawn("st-cli", ["bundle"], { cwd, stdio: ["ignore", "pipe", "pipe"] });
    let stderr = "";
    proc.stderr.on("data", (d: Buffer) => { stderr += d.toString(); });
    proc.on("error", (e: Error) => {
      vscode.window.showErrorMessage(`st-cli bundle failed to launch: ${e.message}`);
      resolve(undefined);
    });
    proc.on("close", (code: number | null) => {
      if (code !== 0) {
        vscode.window.showErrorMessage(`st-cli bundle exited with ${code}: ${stderr}`);
        resolve(undefined);
        return;
      }
      // Pick the newest .st-bundle in the workspace root.
      try {
        const entries = fs.readdirSync(cwd)
          .filter((n: string) => n.endsWith(".st-bundle"))
          .map((n: string) => ({ name: n, mtime: fs.statSync(path.join(cwd, n)).mtimeMs }));
        if (entries.length === 0) {
          vscode.window.showErrorMessage("No .st-bundle file produced by st-cli bundle.");
          resolve(undefined);
          return;
        }
        entries.sort((a: { mtime: number }, b: { mtime: number }) => b.mtime - a.mtime);
        resolve(path.join(cwd, entries[0].name));
      } catch (e: any) {
        vscode.window.showErrorMessage(`Cannot find produced bundle: ${e.message}`);
        resolve(undefined);
      }
    });
  });
}

/**
 * POST a bundle file to `/api/v1/program/update` as multipart/form-data.
 * Throws on non-2xx. Returns the parsed agent response.
 */
async function postProgramUpdate(host: string, port: number, bundlePath: string): Promise<ProgramUpdateResult> {
  const data = fs.readFileSync(bundlePath);
  const boundary = "----PlcUpdate" + Date.now().toString(16);
  const head = Buffer.from(
    `--${boundary}\r\nContent-Disposition: form-data; name="file"; filename="${path.basename(bundlePath)}"\r\n` +
    `Content-Type: application/octet-stream\r\n\r\n`
  );
  const tail = Buffer.from(`\r\n--${boundary}--\r\n`);
  const body = Buffer.concat([head, data, tail]);

  const http = require("http") as typeof import("http");
  return await new Promise<ProgramUpdateResult>((resolve, reject) => {
    const req = http.request({
      hostname: host,
      port,
      path: "/api/v1/program/update",
      method: "POST",
      headers: {
        "Content-Type": `multipart/form-data; boundary=${boundary}`,
        "Content-Length": body.length,
      },
    }, (res) => {
      let raw = "";
      res.on("data", (chunk: Buffer) => { raw += chunk.toString(); });
      res.on("end", () => {
        if (!res.statusCode || res.statusCode < 200 || res.statusCode >= 300) {
          reject(new Error(`HTTP ${res.statusCode}: ${raw}`));
          return;
        }
        try { resolve(JSON.parse(raw)); }
        catch (e) { reject(new Error(`Invalid JSON: ${raw}`)); }
      });
    });
    req.on("error", reject);
    req.write(body);
    req.end();
  });
}

/**
 * Debug adapter factory that supports both local launch and remote attach.
 *
 * - **launch**: Spawns `st-cli debug <file>` as a local subprocess (existing behavior).
 * - **attach**: Connects to a remote target agent's DAP proxy TCP port. The agent
 *   bridges the TCP connection to `st-cli debug` running on the target device.
 *   VS Code sends/receives DAP messages directly over TCP (Content-Length framing).
 */
class StDebugAdapterFactory implements vscode.DebugAdapterDescriptorFactory {
  constructor(private stCliPath: string) {}

  createDebugAdapterDescriptor(
    session: vscode.DebugSession,
    _executable: vscode.DebugAdapterExecutable | undefined
  ): vscode.ProviderResult<vscode.DebugAdapterDescriptor> {
    const config = session.configuration;

    if (config.request === "attach") {
      let host: string = config.host;
      let port: number = config.port;

      console.log(`[ST-DEBUG] Attach config: target=${config.target} host=${host} port=${port}`);

      // If "target" is specified, resolve host/port from plc-project.yaml
      if (config.target && !host) {
        const resolved = resolveTarget(config.target);
        if (resolved) {
          host = resolved.host;
          port = port || resolved.dapPort;
          console.log(`[ST-DEBUG] Resolved target '${config.target}' → ${host}:${port}`);
        } else {
          // Target name not found — try using the target name directly as a hostname
          console.log(`[ST-DEBUG] Target '${config.target}' not in plc-project.yaml, using as hostname`);
          host = config.target;
          port = port || 4841;
        }
      }

      host = host || "127.0.0.1";
      port = port || 4841;

      console.log(`[ST-DEBUG] Creating DebugAdapterServer(${port}, ${host})`);
      vscode.window.setStatusBarMessage(`Connecting to ${host}:${port}...`, 3000);
      return new vscode.DebugAdapterServer(port, host);
    }

    // Local launch: spawn st-cli debug as a subprocess
    const program = config.program || "";
    return new vscode.DebugAdapterExecutable(this.stCliPath, ["debug", program]);
  }
}
