import * as vscode from "vscode";

/**
 * WebView panel for PLC online monitoring.
 * Shows live variable values, cycle stats, and force table.
 */
export class MonitorPanel {
  public static currentPanel: MonitorPanel | undefined;
  private readonly panel: vscode.WebviewPanel;
  private disposables: vscode.Disposable[] = [];
  private variables: Map<string, { value: string; type: string }> = new Map();
  private forcedVars: Set<string> = new Set();
  private cycleInfo = { cycle_count: 0, last_cycle_us: 0, min_cycle_us: 0, max_cycle_us: 0, avg_cycle_us: 0 };

  private constructor(panel: vscode.WebviewPanel) {
    this.panel = panel;
    this.panel.onDidDispose(() => this.dispose(), null, this.disposables);
    this.panel.webview.onDidReceiveMessage(
      (msg) => this.handleWebviewMessage(msg),
      null,
      this.disposables
    );
    this.updateContent();
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

  public updateVariables(vars: Array<{ name: string; value: string; type: string }>) {
    for (const v of vars) {
      this.variables.set(v.name, { value: v.value, type: v.type });
    }
    this.updateContent();
  }

  public updateCycleInfo(info: typeof this.cycleInfo) {
    this.cycleInfo = info;
    this.updateContent();
  }

  private handleWebviewMessage(msg: any) {
    switch (msg.command) {
      case "force":
        this.forcedVars.add(msg.variable);
        // TODO: Send force command to monitor server
        break;
      case "unforce":
        this.forcedVars.delete(msg.variable);
        // TODO: Send unforce command to monitor server
        break;
      case "refresh":
        this.updateContent();
        break;
    }
  }

  private updateContent() {
    this.panel.webview.html = this.getHtml();
  }

  private getHtml(): string {
    const varRows = Array.from(this.variables.entries())
      .map(([name, { value, type }]) => {
        const forced = this.forcedVars.has(name) ? "forced" : "";
        return `<tr class="${forced}">
          <td>${name}</td>
          <td class="value">${value}</td>
          <td class="type">${type}</td>
          <td>
            <button onclick="forceVar('${name}')">Force</button>
            <button onclick="unforceVar('${name}')">Unforce</button>
          </td>
        </tr>`;
      })
      .join("\n");

    return `<!DOCTYPE html>
<html>
<head>
  <style>
    body { font-family: var(--vscode-font-family); color: var(--vscode-foreground); background: var(--vscode-editor-background); padding: 10px; }
    h2 { margin-top: 0; border-bottom: 1px solid var(--vscode-panel-border); padding-bottom: 4px; }
    table { width: 100%; border-collapse: collapse; margin-bottom: 20px; }
    th, td { text-align: left; padding: 4px 8px; border-bottom: 1px solid var(--vscode-panel-border); }
    th { background: var(--vscode-editor-selectionBackground); font-weight: bold; }
    .value { font-family: monospace; font-weight: bold; }
    .type { color: var(--vscode-descriptionForeground); font-style: italic; }
    .forced { background: var(--vscode-inputValidation-warningBackground); }
    .stats { display: grid; grid-template-columns: 1fr 1fr; gap: 4px; }
    .stat-label { color: var(--vscode-descriptionForeground); }
    .stat-value { font-family: monospace; font-weight: bold; }
    button { padding: 2px 8px; cursor: pointer; background: var(--vscode-button-background); color: var(--vscode-button-foreground); border: none; border-radius: 2px; }
    button:hover { background: var(--vscode-button-hoverBackground); }
  </style>
</head>
<body>
  <h2>Scan Cycle</h2>
  <div class="stats">
    <span class="stat-label">Cycles:</span><span class="stat-value">${this.cycleInfo.cycle_count}</span>
    <span class="stat-label">Last:</span><span class="stat-value">${this.cycleInfo.last_cycle_us} &micro;s</span>
    <span class="stat-label">Min:</span><span class="stat-value">${this.cycleInfo.min_cycle_us} &micro;s</span>
    <span class="stat-label">Max:</span><span class="stat-value">${this.cycleInfo.max_cycle_us} &micro;s</span>
    <span class="stat-label">Avg:</span><span class="stat-value">${this.cycleInfo.avg_cycle_us} &micro;s</span>
  </div>

  <h2>Variables</h2>
  <table>
    <tr><th>Name</th><th>Value</th><th>Type</th><th>Actions</th></tr>
    ${varRows || '<tr><td colspan="4" style="text-align:center;color:var(--vscode-descriptionForeground)">No variables</td></tr>'}
  </table>

  <script>
    const vscode = acquireVsCodeApi();
    function forceVar(name) {
      const value = prompt('Force value for ' + name + ':');
      if (value !== null) {
        vscode.postMessage({ command: 'force', variable: name, value: value });
      }
    }
    function unforceVar(name) {
      vscode.postMessage({ command: 'unforce', variable: name });
    }
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
