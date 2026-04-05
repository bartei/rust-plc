import * as path from "path";
import * as fs from "fs";
import * as vscode from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;

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
