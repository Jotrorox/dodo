const assert = require("node:assert/strict");
const fs = require("node:fs/promises");
const path = require("node:path");
const vscode = require("vscode");

async function eventually(label, check) {
  const deadline = Date.now() + 15000;
  while (Date.now() < deadline) {
    const value = await check();
    if (value) return value;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error(`Timed out waiting for ${label}`);
}

async function replace(document, text) {
  const edit = new vscode.WorkspaceEdit();
  edit.replace(document.uri, new vscode.Range(document.positionAt(0), document.positionAt(document.getText().length)), text);
  assert.ok(await vscode.workspace.applyEdit(edit));
}

exports.run = async function () {
  const folder = vscode.workspace.workspaceFolders[0].uri.fsPath;
  const source = "package editor_test\nfn add(left: i32, right: i32) -> i32 { return left + right }\nfn main() -> i32 {\nlet answer = add(20, 22)\nreturn answer\n}\n";
  const uri = vscode.Uri.file(path.join(folder, "main.dodo"));
  await fs.writeFile(uri.fsPath, source);
  const document = await vscode.workspace.openTextDocument(uri);
  assert.equal(document.languageId, "dodo", "file association");
  await vscode.window.showTextDocument(document);
  const extension = vscode.extensions.getExtension("Jotrorox.dodo-vscode");
  assert.ok(extension);
  await extension.activate();

  const at = (text, offset = 0) => document.positionAt(document.getText().lastIndexOf(text) + offset);
  const hover = () => vscode.commands.executeCommand("vscode.executeHoverProvider", uri, at("answer", 2));
  await eventually("hover from dodo lsp", async () => (await hover())?.length);
  assert.match((await hover()).flatMap((item) => item.contents.map((content) => content.value)).join("\n"), /i32/);

  const completions = await vscode.commands.executeCommand("vscode.executeCompletionItemProvider", uri, at("answer", 2));
  assert.ok(completions.items.some((item) => item.label === "answer"));
  const definitions = await vscode.commands.executeCommand("vscode.executeDefinitionProvider", uri, at("add(", 1));
  assert.equal(definitions.length, 1);
  assert.equal((definitions[0].range || definitions[0].targetRange).start.line, 1);
  const references = await vscode.commands.executeCommand("vscode.executeReferenceProvider", uri, at("answer", 2));
  assert.ok(references.length >= 2);
  const rename = await vscode.commands.executeCommand("vscode.executeDocumentRenameProvider", uri, at("answer", 2), "total");
  assert.equal(rename.get(uri).length, 2);
  const signatures = await vscode.commands.executeCommand("vscode.executeSignatureHelpProvider", uri, at("22)", 1));
  assert.ok(signatures.signatures.length);
  assert.equal(signatures.activeParameter, 1);

  await replace(document, source.replace("return answer", "return missing_name"));
  await eventually("unsaved diagnostics", () => vscode.languages.getDiagnostics(uri).some((d) => /missing_name/.test(d.message)));
  assert.equal(await fs.readFile(uri.fsPath, "utf8"), source, "analysis leaves disk untouched");
  await replace(document, source);
  await eventually("cleared diagnostics", () => vscode.languages.getDiagnostics(uri).length === 0);

  // Real client conversion preserves type hints, their exact positions, and ranges.
  const hintSource = "package editor_test\nfn main() -> i32 {\nlet answer = 42i32\nlet annotated: i32 = 7\nreturn answer + annotated\n}\n";
  await replace(document, hintSource);
  const inlayHints = (range = new vscode.Range(document.positionAt(0), document.positionAt(document.getText().length))) =>
    vscode.commands.executeCommand("vscode.executeInlayHintProvider", uri, range);
  const labelText = (hint) => typeof hint.label === "string" ? hint.label : hint.label.map((part) => part.value).join("");
  const answerPosition = document.positionAt(hintSource.indexOf("answer") + "answer".length);
  const hints = await eventually("inferred type inlay hint", async () => {
    const result = await inlayHints();
    return result?.some((hint) => hint.position.isEqual(answerPosition) && labelText(hint) === ": i32") && result;
  });
  const answerHint = hints.find((hint) => hint.position.isEqual(answerPosition));
  assert.equal(answerHint.kind, vscode.InlayHintKind.Type);
  assert.ok(!hints.some((hint) => hint.position.line === 3), "explicit annotation has no redundant hint");
  const otherLineHints = await inlayHints(new vscode.Range(3, 0, 4, 0));
  assert.ok(!otherLineHints.some((hint) => hint.position.isEqual(answerPosition)), "hint requests respect their range");
  await replace(document, hintSource.replace("let answer =", "let answer: i32 ="));
  await eventually("inlay hints update for unsaved annotations", async () => (await inlayHints())?.length === 0);

  const quickFix = async (diagnostic, title) => {
    const actions = await vscode.commands.executeCommand(
      "vscode.executeCodeActionProvider", uri, diagnostic.range, vscode.CodeActionKind.QuickFix.value);
    return actions?.find((action) => action.title === title && action.edit);
  };
  await replace(document, "package editor_test\nfn main() -> i32 {\nlet count = 1i32\ncount = 2\nreturn count\n}\n");
  const immutableDiagnostic = await eventually("immutable assignment diagnostic", () =>
    vscode.languages.getDiagnostics(uri).find((diagnostic) => /immutable binding/.test(diagnostic.message)));
  const mutableFix = await eventually("make mutable quick fix", () => quickFix(immutableDiagnostic, "Make `count` mutable"));
  assert.equal(mutableFix.kind.value, vscode.CodeActionKind.QuickFix.value);
  assert.ok(await vscode.workspace.applyEdit(mutableFix.edit));
  assert.match(document.getText(), /\ncount := 1i32\n/);
  await eventually("mutable quick fix clears diagnostics", () => vscode.languages.getDiagnostics(uri).length === 0);
  await vscode.commands.executeCommand("undo");
  assert.match(document.getText(), /\nlet count = 1i32\n/);
  await eventually("undo restores immutable assignment diagnostic", () =>
    vscode.languages.getDiagnostics(uri).some((diagnostic) => /immutable binding/.test(diagnostic.message)));
  await vscode.commands.executeCommand("redo");
  assert.match(document.getText(), /\ncount := 1i32\n/);
  await eventually("redo clears immutable assignment diagnostic", () => vscode.languages.getDiagnostics(uri).length === 0);

  const missingImportUri = vscode.Uri.file(path.join(folder, "math.dodo"));
  await fs.writeFile(missingImportUri.fsPath, "package math\npub fn answer() -> i32 { return 42 }\n");
  await replace(document, "package editor_test\nfn main() -> i32 { return math.answer() }\n");
  const importDiagnostic = await eventually("missing import diagnostic", () =>
    vscode.languages.getDiagnostics(uri).find((diagnostic) =>
      /cannot resolve the receiver type|unknown.*math/.test(diagnostic.message)));
  const importFix = await eventually("missing import quick fix", () => quickFix(importDiagnostic, "Import `math`"));
  assert.equal(importFix.kind.value, vscode.CodeActionKind.QuickFix.value);
  assert.ok(await vscode.workspace.applyEdit(importFix.edit));
  assert.match(document.getText(), /\bimport "math"/);
  await eventually("import quick fix clears diagnostics", () => vscode.languages.getDiagnostics(uri).length === 0);
  assert.equal(await fs.readFile(uri.fsPath, "utf8"), source, "quick fixes edit the unsaved buffer");
  await replace(document, source);
  await fs.unlink(missingImportUri.fsPath);
  await eventually("quick fix fixture cleanup", () => vscode.languages.getDiagnostics(uri).length === 0);

  const formatted = await vscode.commands.executeCommand("vscode.executeFormatDocumentProvider", uri, { tabSize: 2, insertSpaces: true });
  assert.ok(formatted.length);
  const formattingEdit = new vscode.WorkspaceEdit();
  formattingEdit.set(uri, formatted);
  assert.ok(await vscode.workspace.applyEdit(formattingEdit));
  assert.match(document.getText(), /\n    let answer/);

  await vscode.commands.executeCommand("dodo.restartServer");
  await eventually("hover after restart", async () => (await hover())?.length);

  await replace(document, "package editor_test\nimport \"std/math\"\nfn main() { _ = math.abs(-1.0) }\n");
  const libraryDefinitions = await eventually("bundled definition", async () => {
    const result = await vscode.commands.executeCommand("vscode.executeDefinitionProvider", uri, at("abs", 1));
    return result?.some((item) => (item.uri || item.targetUri).scheme === "dodo-stdlib") && result;
  });
  const libraryLocation = libraryDefinitions[0];
  const libraryUri = libraryLocation.uri || libraryLocation.targetUri;
  const libraryDocument = await vscode.workspace.openTextDocument(libraryUri);
  assert.equal(libraryDocument.languageId, "dodo");
  assert.match(libraryDocument.lineAt((libraryLocation.range || libraryLocation.targetRange).start.line).text, /pub fn abs/);
  await vscode.window.showTextDocument(libraryDocument);
  const libraryPosition = libraryDocument.positionAt(libraryDocument.getText().indexOf("from_bits(0x7ff"));
  await eventually("navigation inside bundled source", async () => {
    const result = await vscode.commands.executeCommand("vscode.executeDefinitionProvider", libraryUri, libraryPosition);
    return result?.some((item) => (item.uri || item.targetUri).toString() === libraryUri.toString());
  });
  await vscode.commands.executeCommand("dodo.restartServer");
  await eventually("bundled hover after restart", async () =>
    (await vscode.commands.executeCommand("vscode.executeHoverProvider", libraryUri, libraryPosition))?.length);

  // Change a closed import on disk without saving or editing its open caller.
  const dependencyUri = vscode.Uri.file(path.join(folder, "lib", "value.dodo"));
  const dependencySource = "package lib\npub fn value() -> i32 { return 7 }\n";
  await fs.mkdir(path.dirname(dependencyUri.fsPath));
  await fs.writeFile(dependencyUri.fsPath, dependencySource);
  await replace(document, "package editor_test\nimport \"lib\"\nfn main() -> i32 { return lib.value() }\n");
  let dependencyDefinitions;
  await eventually("import navigation", async () => {
    dependencyDefinitions = await vscode.commands.executeCommand("vscode.executeDefinitionProvider", uri, at("value", 1));
    return dependencyDefinitions?.some((item) => (item.uri || item.targetUri).toString() === dependencyUri.toString());
  }).catch((error) => {
    throw new Error(`${error.message}: expected ${dependencyUri.toString()}, got ${JSON.stringify(dependencyDefinitions)}; diagnostics: ${JSON.stringify(vscode.languages.getDiagnostics(uri))}`, { cause: error });
  });
  await fs.writeFile(dependencyUri.fsPath, dependencySource.replace("return 7", "return missing_disk"));
  await eventually("watched dependency diagnostics", () => vscode.languages.getDiagnostics(dependencyUri).some((d) => /missing_disk/.test(d.message)));
  await fs.writeFile(dependencyUri.fsPath, dependencySource);
  await eventually("watched dependency repair", () => vscode.languages.getDiagnostics(dependencyUri).length === 0);
  await fs.unlink(dependencyUri.fsPath);
  await eventually("watched dependency deletion", () => vscode.languages.getDiagnostics(uri).some((d) => /no .dodo|cannot resolve/.test(d.message)));
  await fs.writeFile(dependencyUri.fsPath, dependencySource);
  await eventually("watched dependency recreation", () => vscode.languages.getDiagnostics(uri).length === 0);

  // A workspace option must actually reach the server and take effect after restart.
  const config = vscode.workspace.getConfiguration("dodo");
  await config.update("checkMode", "package", vscode.ConfigurationTarget.Workspace);
  await fs.writeFile(path.join(folder, "helper.dodo"), "package editor_test\npub fn helper() -> i32 { return 42 }\n");
  await replace(document, "package editor_test\nfn main() -> i32 { return helper() }\n");
  await eventually("package navigation", async () => {
    const result = await vscode.commands.executeCommand("vscode.executeDefinitionProvider", uri, at("helper", 2));
    return result?.some((item) => (item.uri || item.targetUri).fsPath.endsWith("helper.dodo"));
  });
  assert.equal(vscode.languages.getDiagnostics(uri).length, 0);

  const helper = await vscode.workspace.openTextDocument(vscode.Uri.file(path.join(folder, "helper.dodo")));
  await replace(helper, "package editor_test\npub fn helper() -> i32 { return missing_overlay }\n");
  await eventually("unsaved sibling overlay", () => vscode.languages.getDiagnostics(helper.uri).some((d) => /missing_overlay/.test(d.message)));
  await replace(helper, "package editor_test\npub fn helper() -> i32 { return 42 }\n");

  await config.update("target", "wasm32-unknown-unknown", vscode.ConfigurationTarget.Workspace);
  await replace(document, "package editor_test\nfn main() -> i32 {\n    value: usize = 4294967296\n    return 0\n}\n");
  await eventually("32-bit target diagnostics", () => vscode.languages.getDiagnostics(uri).some((d) => /usize|range|fit/.test(d.message)));

  const untitled = await vscode.workspace.openTextDocument({ language: "dodo", content: "package scratch\nfn main() -> i32 { return missing_scratch }\n" });
  await vscode.window.showTextDocument(untitled);
  await eventually("untitled diagnostics", () => vscode.languages.getDiagnostics(untitled.uri).some((d) => /missing_scratch/.test(d.message)));

  // Use VS Code's snippet parser and the real server to validate a complete program.
  const snippets = JSON.parse(await fs.readFile(path.join(extension.extensionPath, "snippets/dodo.json"), "utf8"));
  await replace(untitled, "");
  const editor = await vscode.window.showTextDocument(untitled);
  assert.ok(await editor.insertSnippet(new vscode.SnippetString(snippets["Main program"].body.join("\n"))));
  assert.match(untitled.getText(), /package main/);
  await eventually("snippet diagnostics clear", () => vscode.languages.getDiagnostics(untitled.uri).length === 0);

  const scratchHover = () => vscode.commands.executeCommand("vscode.executeHoverProvider", untitled.uri, new vscode.Position(4, 11));
  await eventually("snippet hover", async () => (await scratchHover())?.length);
  await config.update("server.enabled", false, vscode.ConfigurationTarget.Workspace);
  await eventually("disabled server", async () => !(await scratchHover())?.length);
  await config.update("server.enabled", true, vscode.ConfigurationTarget.Workspace);
  await eventually("reenabled server", async () => (await scratchHover())?.length);
  console.log("Dodo integration passed: activation, hover, completion, definition, bundled sources, watched dependencies, references, rename, signatures, diagnostics, type inlay hints, mutable and missing-import quick fixes, formatting, restart, settings, overlays, untitled buffers, snippets, and disable.");
};
