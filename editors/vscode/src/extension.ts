import * as path from "path";
import * as fs from "fs";
import {
  ExtensionContext,
  workspace,
  window,
} from "vscode";
import {
  LanguageClient,
  LanguageClientOptions,
  ServerOptions,
} from "vscode-languageclient/node";

let client: LanguageClient | undefined;

export function activate(context: ExtensionContext) {
  let serverPath = workspace
    .getConfiguration("structured-text")
    .get<string>("serverPath", "st-cli");

  // Resolve VSCode/devcontainer variables in the setting
  const folders = workspace.workspaceFolders;
  if (folders && folders.length > 0) {
    const wsPath = folders[0].uri.fsPath;
    serverPath = serverPath
      .replace(/\$\{workspaceFolder\}/g, wsPath)
      .replace(/\$\{containerWorkspaceFolder\}/g, wsPath);
  }

  // If not an absolute path, try to find it relative to the extension
  if (!path.isAbsolute(serverPath) && !serverPath.includes(path.sep)) {
    // Look for the binary in the extension's parent project (dev mode)
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

  const serverOptions: ServerOptions = {
    command: serverPath,
    args: ["serve"],
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [{ scheme: "file", language: "structured-text" }],
    synchronize: {
      fileEvents: workspace.createFileSystemWatcher("**/*.{st,scl}"),
    },
  };

  client = new LanguageClient(
    "structured-text",
    "Structured Text Language Server",
    serverOptions,
    clientOptions
  );

  client.start().then(
    () => {
      // Server started successfully
    },
    (err: Error) => {
      window.showErrorMessage(
        `Failed to start ST language server at '${serverPath}': ${err.message}.\n` +
        `Build it with: cargo build -p st-cli`
      );
    }
  );

  context.subscriptions.push({
    dispose: () => {
      client?.stop();
    },
  });
}

export function deactivate(): Thenable<void> | undefined {
  return client?.stop();
}
