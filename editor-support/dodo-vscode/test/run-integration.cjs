const fs = require("node:fs/promises");
const os = require("node:os");
const path = require("node:path");
const { runTests } = require("@vscode/test-electron");

async function main() {
  // Electron-based terminals can export this; the test runner needs a GUI host.
  delete process.env.ELECTRON_RUN_AS_NODE;
  const extension = path.resolve(__dirname, "..");
  const server = path.resolve(process.env.DODO_TEST_SERVER || path.join(extension, "../../target/debug/dodo" + (process.platform === "win32" ? ".exe" : "")));
  await fs.access(server);
  const temporary = await fs.mkdtemp(path.join(os.tmpdir(), "dodo-vscode-test-"));
  const workspace = path.join(temporary, "workspace with spaces");
  try {
    await fs.mkdir(path.join(workspace, ".vscode"), { recursive: true });
    // Exercise expansion and executable paths containing spaces, without a shell.
    const executable = path.join(workspace, "compiler tools", path.basename(server));
    await fs.mkdir(path.dirname(executable));
    await fs.copyFile(server, executable);
    await fs.chmod(executable, 0o755);
    await fs.writeFile(path.join(workspace, ".vscode/settings.json"), JSON.stringify({
      "dodo.server.path": "${workspaceFolder}/compiler tools/" + path.basename(server),
      "dodo.trace.server": "verbose",
      "editor.formatOnSave": false,
    }));
    await runTests({
      version: process.env.VSCODE_TEST_VERSION || "1.137.0",
      extensionDevelopmentPath: extension,
      extensionTestsPath: path.join(__dirname, "integration.cjs"),
      launchArgs: [
        workspace,
        "--user-data-dir=" + path.join(temporary, "user"),
        "--extensions-dir=" + path.join(temporary, "extensions"),
        "--disable-workspace-trust", "--disable-extensions", "--skip-welcome",
        "--skip-release-notes", "--no-sandbox", "--disable-gpu",
      ],
    });
  } catch (error) {
    const logs = path.join(temporary, "user", "logs");
    for (const file of await fs.readdir(logs, { recursive: true }).catch(() => [])) {
      if (/dodo.*\.log$/i.test(file)) {
        console.error(await fs.readFile(path.join(logs, file), "utf8"));
      }
    }
    throw error;
  } finally {
    await fs.rm(temporary, { recursive: true, force: true });
  }
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
