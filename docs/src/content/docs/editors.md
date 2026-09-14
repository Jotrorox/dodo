---
title: "Editor setup"
description: "Configure dodo lsp for completion, navigation, rename, signatures, formatting, and target-aware diagnostics."
section: "Using Dodo"
order: 120
---

Dodo includes a Language Server Protocol (LSP) server. After
[installing the compiler](installation.md), connect it to your editor's LSP
client to get diagnostics, ownership information, and editing tools for `.dodo` files.

## Visual Studio Code

The [Dodo VS Code extension](https://github.com/Jotrorox/dodo/tree/main/editor-support/dodo-vscode)
connects to `dodo lsp` automatically and adds syntax highlighting, bracket and
comment support, and snippets for programs, functions, types, loops, matches,
and tests. It requires VS Code 1.91 or newer and Dodo 0.1.2 or newer.

Download `dodo-vscode-0.1.2.vsix` from
[GitHub Releases](https://github.com/Jotrorox/dodo/releases/tag/v0.1.2), then install it:

```sh
code --install-extension dodo-vscode-0.1.2.vsix
```

To build the extension from a checkout with Node.js 22 or newer:

```sh
cd editor-support/dodo-vscode
npm ci
npm run package
code --install-extension dodo-vscode-0.1.2.vsix
```

Alternatively, select the VSIX with **Extensions: Install from VSIX...**.
Open a `.dodo` file to activate the extension. It runs `dodo` from the extension
host's PATH; set `dodo.server.path` to an executable path if needed. A source
build can use `${workspaceFolder}/target/debug/dodo` (with `.exe` on Windows).
Relative executable paths resolve against the first workspace folder.

Set these options in VS Code's settings as needed:

```json
{
  "dodo.server.path": "dodo",
  "dodo.checkMode": "file",
  "dodo.target": "",
  "[dodo]": {
    "editor.defaultFormatter": "Jotrorox.dodo-vscode",
    "editor.formatOnSave": true
  }
}
```

The checking mode and target correspond to the initialization options described
below. Changes restart the server automatically. Settings apply to the whole
window, including multi-root workspaces. In SSH, WSL, and dev containers, Dodo
must be installed on the remote extension host.

Use **Dodo: Restart Language Server** after rebuilding the compiler and
**Dodo: Show Language Server Output** to inspect startup errors. Set that output
channel's log level to **Trace** with **Developer: Set Log Level** for protocol
logs; `dodo.trace.server` selects message or verbose detail. Highlighting and snippets work without the
compiler; the language server runs only in trusted workspaces.

## Start the language server

Configure your editor's LSP client with these settings:

| Client setting | Value |
| --- | --- |
| Executable | `dodo` (available on the editor's `PATH`) |
| Arguments | `["lsp"]` |
| Language or filetype | `dodo` |
| File extension | `.dodo` |
| Transport | Standard input/output |

The exact setting names depend on your editor's LSP client. The server takes no
source filename. `dodo --lsp` is an equivalent entry point.

## Diagnostics and hovers

Hover information includes:

- The inferred type of a local binding or expression, such as `view: &i32`.
- A method's receiver behavior: a shared borrow, a mutable borrow, or consumption
  of `self`.
- Borrowed-return sources such as `from(self)` or `from(a, b)`, including whether
  the contract was inferred or explicitly written.

Open [borrowing.dodo](https://github.com/Jotrorox/dodo/blob/main/examples/borrowing.dodo) and hover `view`, the method
names `view`, `replace`, and `finish`, and the `choose` call to inspect these
details. Ownership errors appear as editor diagnostics; their related locations
point to the same source expressions as the command-line labels.

## Documents and synchronization

The server supports initialization, shutdown, full-document open/change/close
synchronization, save notifications, hover, completion, definition, references,
prepare-rename/rename, signature help, document formatting, and published diagnostics.
It accepts local `file:` URIs and uses zero-based LSP line numbers and UTF-16 columns.
Open buffers supply unsaved source, including new files;
local imports use those buffers when available. Changes recheck open documents,
saves refresh diagnostics, and closing a buffer restores its imported contents
from disk.

Untitled buffers receive standalone analysis and editing tools.

## Completion, navigation, and refactoring

- Completion suggests visible declarations, local bindings, keywords, and types.
  Typing `.` requests members of an imported package or a typed receiver. Imported
  private declarations are excluded. Suggestions use the current unsaved buffer.
- Go-to-definition and references use source locations across the loaded package
  and imports. Local bindings retain their identity through nested shadowing.
  References honor the client's option to include the declaration.
- Rename returns a workspace edit for the identified declaration and its indexed
  uses. It excludes comments and strings, rejects invalid names and possible name
  capture, and includes open-buffer versions when the client supports versioned
  document changes. The editor applies the edits; the server never writes files.
- Signature help triggers on `(` and `,`, selects the active argument, and omits
  the implicit receiver parameter for instance methods. It also works while a
  call's argument list is incomplete.

## Formatting

Use your editor's document-format command or format-on-save setting. The server
uses the same comment-preserving canonical formatter as `dodo fmt`, including its
syntax migration and indentation rules. Client indentation preferences do not
override the canonical style. It returns one whole-document edit, or no edits if
the buffer is already formatted. Invalid syntax produces a request error and
leaves the buffer untouched.

## Compilation target

Set `initializationOptions.target` to the same LLVM triple used by your project's
`dodo check --target` or `dodo compile --target` command. For example:

```json
{"checkMode": "package", "target": "wasm32-unknown-unknown"}
```

The target controls both pointer-width checking (`usize`, `isize`, array bounds)
and selection of hosted imports such as `std/fs/native`. It applies to file,
package, dependency, fallback, and untitled analysis. Invalid triples are rejected
during initialization. The default is LLVM's host target. Restart the server after
changing the initialization options; there is no project manifest to infer them from.

## File and package checking

By default each document is checked as a file with its imports, like
`dodo check <file>`. This matches the project entry behavior when editing
`main.dodo`. To work on multiple files within an imported subfolder, set the
client's `initializationOptions` to `{"checkMode": "package"}`. This checks the
document's parent directory, including new unsaved `.dodo` siblings. All files in
that directory package must declare the same package. Package checking combines
all immediate `.dodo` files, like a directory import; it does not select a
`main.dodo` entry as the CLI does for an explicit project folder.

## Current limits

Editor parsing recovers at statement and declaration boundaries. Semantic checking
restores checker state after failed statements and continues with independent
statements and functions, producing multiple diagnostics. Shared declaration,
import-resolution, and preparation errors can still stop a package check; recovery
can leave some expressions without inferred types or produce follow-on errors.
Compilation and formatting retain strict error handling.

Navigation and rename cover open documents and the packages/imports they load;
there is no scan of unopened workspace roots or reverse dependencies. Bundled
standard-library declarations contribute completion and signatures, but currently
have no editor-readable source URI for navigation or edits. Rename conservatively
refuses possible collisions, generic parameters, destructuring bindings, and fields
or bindings requiring shorthand/pattern expansion. These restrictions avoid edits
that could change a different binding or leave a partial rename.

Checks remain synchronous and rebuild full-document analysis. There is no
incremental analysis or background file watcher. Save a document to refresh its
on-disk dependencies. See the [responsiveness measurements](lsp-performance.md)
and benchmark commands before changing the analysis model.

See [ownership diagnostics](diagnostics-and-editors.md) for annotated examples
of the errors shown in the editor.
