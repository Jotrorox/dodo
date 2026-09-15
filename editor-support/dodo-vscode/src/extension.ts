import * as path from "node:path";
import * as vscode from "vscode";
import { LanguageClient } from "vscode-languageclient/node";

let client: LanguageClient | undefined;
let pending: Promise<void> = Promise.resolve();
let shuttingDown = false;

function serverPath(configured: string, folder: vscode.WorkspaceFolder | undefined): string {
  if (!configured.trim()) {
    throw new Error("Set dodo.server.path to the Dodo executable.");
  }
  if (configured.includes("${workspaceFolder}")) {
    if (!folder) {
      throw new Error("dodo.server.path uses ${workspaceFolder}, but no local workspace folder is open.");
    }
    configured = configured.replaceAll("${workspaceFolder}", folder.uri.fsPath);
  }
  if (path.isAbsolute(configured) || !/[\\/]/.test(configured)) {
    return configured;
  }
  if (!folder) {
    throw new Error("A relative dodo.server.path requires a local workspace folder. Use an absolute path or dodo on PATH.");
  }
  return path.resolve(folder.uri.fsPath, configured);
}

async function stopServer(): Promise<void> {
  const previous = client;
  client = undefined;
  await previous?.dispose();
}

export async function activate(context: vscode.ExtensionContext): Promise<void> {
  shuttingDown = false;
  const output = vscode.window.createOutputChannel("Dodo Language Server", { log: true });
  const bundledSources = new vscode.EventEmitter<vscode.Uri>();
  context.subscriptions.push(output, bundledSources);
  context.subscriptions.push(vscode.workspace.registerTextDocumentContentProvider("dodo-stdlib", {
    onDidChange: bundledSources.event,
    async provideTextDocumentContent(uri, token): Promise<string> {
      if (!client) {
        throw new Error("Start the Dodo language server to view bundled library sources.");
      }
      return client.sendRequest<string>("dodo/stdlibSource", { uri: uri.toString() }, token);
    },
  }));

  // Serialize configuration changes and manual restarts so only one process owns
  // the open-document overlays. Recreating the client resends initialization options.
  function restart(): Promise<void> {
    pending = pending.then(async () => {
      await stopServer();
      if (shuttingDown || !vscode.workspace.isTrusted) {
        return;
      }
      const config = vscode.workspace.getConfiguration("dodo");
      if (!config.get<boolean>("server.enabled", true)) {
        return;
      }
      const folders = vscode.workspace.workspaceFolders;
      const folder = folders?.[0];
      if (folder && folder.uri.scheme !== "file") {
        return;
      }
      const command = serverPath(config.get<string>("server.path", "dodo"), folder);
      const target = config.get<string>("target", "").trim();
      output.info(`Starting ${command} lsp`);
      client = new LanguageClient("dodo", "Dodo Language Server", {
        command,
        args: ["lsp"],
        // Executables default to stdio. Setting TransportKind.stdio would append
        // --stdio, which Dodo's `lsp` command does not accept.
        options: { cwd: folder?.uri.fsPath, shell: false },
      }, {
        documentSelector: [
          { scheme: "file", language: "dodo" },
          { scheme: "untitled", language: "dodo" },
          { scheme: "dodo-stdlib", language: "dodo" },
        ],
        initializationOptions: {
          checkMode: config.get<string>("checkMode", "file"),
          ...(target ? { target } : {}),
        },
        outputChannel: output,
        traceOutputChannel: output,
        // One client for all roots preserves overlays across folder boundaries.
        // Server options intentionally have window scope, matching that lifetime.
      });
      await client.start();
      for (const document of vscode.workspace.textDocuments) {
        if (document.uri.scheme === "dodo-stdlib") {
          bundledSources.fire(document.uri);
        }
      }
    }).catch(async (error: unknown) => {
      const message = error instanceof Error ? error.message : String(error);
      output.error(`Language server failed: ${message}`);
      await stopServer().catch((stopError: unknown) => {
        output.error(`Could not stop the language server: ${String(stopError)}`);
      });
      if (!shuttingDown) {
        void vscode.window.showErrorMessage(
          `Dodo language server could not start. Install Dodo or check dodo.server.path. ${message}`,
          "Open Settings", "Show Output",
        ).then((action) => {
          if (action === "Open Settings") {
            void vscode.commands.executeCommand("workbench.action.openSettings", "@ext:Jotrorox.dodo-vscode");
          } else if (action === "Show Output") {
            output.show();
          }
        });
      }
    });
    return pending;
  }

  context.subscriptions.push(
    vscode.commands.registerCommand("dodo.restartServer", restart),
    vscode.commands.registerCommand("dodo.showOutput", () => output.show()),
    vscode.workspace.onDidGrantWorkspaceTrust(() => { void restart(); }),
    vscode.workspace.onDidChangeWorkspaceFolders(() => { void restart(); }),
    vscode.workspace.onDidChangeConfiguration((event) => {
      if (["dodo.server", "dodo.checkMode", "dodo.target"].some((key) => event.affectsConfiguration(key))) {
        void restart();
      }
    }),
  );
  await restart();
}

export async function deactivate(): Promise<void> {
  shuttingDown = true;
  await pending;
  await stopServer();
}
