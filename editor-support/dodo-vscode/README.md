# Dodo for Visual Studio Code

Language support for [Dodo](https://github.com/Jotrorox/dodo), using the compiler's
built-in `dodo lsp` language server.

- Syntax highlighting for declarations, types, attributes, strings, byte literals,
  numeric literals, borrowing, and operators.
- Diagnostics, completion, hover information, go-to-definition, references,
  rename, signature help, and document formatting.
- Comment toggling, bracket pairing, indentation, folding, and snippets.
- Unsaved file and untitled buffer support, with compiler target and package-checking settings.

## Install

Install [Dodo](https://jotrorox.github.io/dodo/installation/) 0.1.2 or newer and
ensure `dodo --version` works. The extension uses an existing compiler; the VSIX
does not contain the compiler or LLVM.

Download `dodo-vscode-0.1.2.vsix` from
[GitHub Releases](https://github.com/Jotrorox/dodo/releases/tag/v0.1.2), then run:

```sh
code --install-extension dodo-vscode-0.1.2.vsix
```

You can also use **Extensions: Install from VSIX...** in VS Code and select the
downloaded file. Open a `.dodo` file to activate the extension. VS Code 1.91 or
newer is required.

### Build from source

With Node.js 22 or newer, run these commands from this repository's root:

```sh
cd editor-support/dodo-vscode
npm ci
npm run package
code --install-extension dodo-vscode-0.1.2.vsix
```

Building the VSIX does not require Rust or LLVM.

### Configure the compiler

If Dodo is not on the extension host's PATH, set **Dodo: Server Path** to the
compiler executable. Paths with spaces work without shell quoting:

```json
{
  "dodo.server.path": "${workspaceFolder}/target/debug/dodo"
}
```

On Windows, use `dodo.exe` and JSON-escaped backslashes or forward slashes. In SSH,
WSL, or a dev container, install the extension and Dodo on that remote host.

## Settings

| Setting | Default | Meaning |
| --- | --- | --- |
| `dodo.server.enabled` | `true` | Start the language server. |
| `dodo.server.path` | `"dodo"` | Executable on PATH or an absolute/relative compiler path. |
| `dodo.checkMode` | `"file"` | Check each file and its imports, or use `"package"` for all immediate sibling `.dodo` files. |
| `dodo.target` | `""` | LLVM target triple; empty uses the host target. |
| `dodo.trace.server` | `"messages"` | LSP trace detail: `"messages"` or `"verbose"`. Requires the output channel's Trace log level. |

Compiler settings apply to the whole VS Code window. One server handles all
workspace folders so unsaved imports remain available across roots. Relative
compiler paths and `${workspaceFolder}` use the first workspace folder. With no
folder open, use an absolute executable path or `dodo` on PATH. Other variable
substitutions and shell commands are not supported.

The server restarts automatically when its executable, enabled state, checking
mode, or target changes. For example, match a cross-compilation target with:

```json
{
  "dodo.checkMode": "package",
  "dodo.target": "wasm32-unknown-unknown"
}
```

Package checking requires the files in each directory to declare the same package.
Use the default file mode for folders containing independent example programs.

Use **Format Document** for the compiler's canonical four-space style. To enable
formatting on save explicitly:

```json
{
  "[dodo]": {
    "editor.defaultFormatter": "Jotrorox.dodo-vscode",
    "editor.formatOnSave": true
  }
}
```

## Commands and snippets

The Command Palette provides **Dodo: Restart Language Server** and
**Dodo: Show Language Server Output**. The output channel includes startup errors.
For protocol traces, use **Developer: Set Log Level**, select **Dodo Language
Server**, and choose **Trace**. Choose **Info** to turn tracing off again.

Type a snippet prefix and select it from completion, or use **Insert Snippet**:

| Prefixes | Snippets |
| --- | --- |
| `main`, `package`, `import` | Program and package setup |
| `fn`, `pubfn`, `method` | Functions and a borrowing method inside a struct |
| `struct`, `enum` | Type declarations |
| `let`, `var`, `const` | Immutable, mutable, and constant bindings |
| `if`, `ifelse`, `for`, `foreach` | Conditionals and loops |
| `match`, `matchoption`, `matchresult` | Pattern matching |
| `unsafe` | Unsafe block with a safety comment |
| `test`, `assert_eq` | Native tests and assertions |

## Troubleshooting and limits

If the server fails to start, open its output channel and check the executable
path. Specify only the compiler executable: the extension adds `lsp` automatically.
After installing or rebuilding Dodo, use **Dodo: Restart Language Server**.

Highlighting and snippets work without a compiler and in Restricted Mode. The
language server starts after the workspace is trusted. It supports file and
untitled documents in desktop/remote VS Code; virtual workspace files and a
browser-only extension host cannot run the native compiler.

Navigation and rename cover open documents and loaded imports. Standard-library
definitions currently have no navigable source URI. The server does not watch
on-disk dependencies; save a document to refresh them. See the
[editor guide](https://jotrorox.github.io/dodo/editors/) for the server's current limits.

## Develop and test

Open this folder in VS Code, run `npm ci`, then press **F5** to launch an Extension
Development Host. The launch configuration builds the extension first. For
continuous bundling, run `npm run watch`; use `npm run check` to check TypeScript.

```sh
npm test
npm run package
```

`npm test` checks TypeScript, bundles the extension, and tests the TextMate grammar
using VS Code's tokenizer and Oniguruma engine, including the repository examples.
The bundle includes its language-client dependencies; packaging excludes source,
tests, source maps, and development dependencies.

For a real VS Code and compiler integration test, first build Dodo from the
repository root with `cargo build --locked --bin dodo`, then run here:

```sh
npm run test:integration
```

The runner downloads VS Code 1.91.1 to test the minimum supported version and uses
an isolated temporary workspace and profile. It exercises LSP features, settings
changes, restart, unsaved overlays, untitled buffers, and snippet insertion.
Set `DODO_TEST_SERVER` to an absolute compiler path to use another build, and
`VSCODE_TEST_VERSION=stable` to test current VS Code. Linux needs a display; on a
headless machine use `xvfb-run -a npm run test:integration`.

The extension follows VS Code's
[language server extension guide](https://code.visualstudio.com/api/language-extensions/language-server-extension-guide).
Its highlighting rules follow Dodo's lexer and parser; update the grammar tests
when the compiler's syntax changes.

## License

[BSD-2-Clause](LICENSE), like Dodo.
