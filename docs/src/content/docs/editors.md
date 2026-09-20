---
title: "Editor setup"
description: "Configure dodo lsp for type inlay hints, diagnostic quick fixes, completion, navigation, formatting, and target-aware diagnostics."
section: "Using Dodo"
order: 120
---

Dodo includes a Language Server Protocol (LSP) server. After
[installing the compiler](installation.md), connect it to your editor's LSP
client to get diagnostics, ownership information, and editing tools for `.dodo` files.

## Verify the compiler before the editor

Run `dodo --version` and `dodo check path/to/main.dodo` in a terminal first.
This separates installation or source errors from editor configuration. The
language server checks code; it does not need a working executable linker just
to show diagnostics. To run or test programs, finish the C toolchain setup in
[installation](installation.md).

If you are new to Dodo, use the VS Code path below. Other editors need an LSP
client configured to launch `dodo lsp` over standard input/output. You do not
need to run a separate server in a terminal while editing.

## Visual Studio Code

The [Dodo VS Code extension](https://github.com/Jotrorox/dodo/tree/main/editor-support/dodo-vscode)
connects to `dodo lsp` automatically and adds syntax highlighting, bracket and
comment support, and snippets for programs, functions, types, loops, matches,
and tests. It requires VS Code 1.137 or newer and Dodo 0.1.3 or newer.

Download `dodo-vscode-0.1.3.vsix` from
[GitHub Releases](https://github.com/Jotrorox/dodo/releases/tag/v0.1.3), then install it:

```sh
code --install-extension dodo-vscode-0.1.3.vsix
```

To build the extension from a checkout with Node.js 22 or newer:

```sh
cd editor-support/dodo-vscode
npm ci
npm run package
code --install-extension dodo-vscode-0.1.3.vsix
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

## Inferred type hints and quick fixes

The server provides inline type hints for inferred local bindings. For example,
`let answer = 42i32` displays `: i32` after `answer`. An explicitly annotated
binding such as `let answer: i32 = 42` needs no hint. Hints reflect unsaved text;
editing an initializer updates its type, and adding an annotation removes the
redundant hint. Clients supporting `workspace/inlayHint/refresh` also refresh
visible hints when a loaded dependency changes. Types unavailable after a
diagnostic are omitted.

In VS Code, control visibility with **Editor: Inlay Hints: Enabled**, or set
`"editor.inlayHints.enabled": "on"` inside the `"[dodo]"` settings block.
Other clients use the standard `textDocument/inlayHint` request with the visible
document range. Hints have the LSP `Type` kind.

Use your editor's quick-fix action on a diagnostic. In VS Code, choose
**Quick Fix...** or press `Ctrl+.` (`Cmd+.` on macOS). Supported actions include:

- **Make `count` mutable** for a direct assignment to, or mutable borrow of, an
  immutable local binding: changes `let count = 1` to `count := 1`, preserving
  an explicit type annotation when present.
- **Import `math`** for a missing package qualifier such as `math.answer()`:
  inserts the import when a matching package can be resolved.

Clients request `textDocument/codeAction`; these actions have the `quickfix`
kind and carry workspace edits. Applying an action updates diagnostics through
normal document synchronization. The editor handles undo and saving; the server
never writes the edit to disk. Not every diagnostic has an automatic fix.
Use a compiler build that supports these requests.

Missing-import suggestions cover qualified names and check that the requested
member is public. They search bundled libraries, matching sibling files and
package directories, and locations inferred from existing imports. They respect
the compilation target and existing imports. They do not scan every project file
or rewrite unqualified names. Mutability fixes are limited to direct local
bindings; changing a borrowed reference or an ownership contract needs a manual
edit.

## Documents and synchronization

The server supports initialization, shutdown, full-document open/change/close
synchronization, save notifications, hover, completion, definition, references,
prepare-rename/rename, signature help, type inlay hints, diagnostic code actions,
document formatting, and published diagnostics.
It accepts local `file:` URIs, untitled buffers, and read-only `dodo-stdlib:`
documents, using zero-based LSP line numbers and UTF-16 columns.
Open buffers supply unsaved source, including new files;
local imports use those buffers when available. Changes recheck open documents,
saves refresh diagnostics, and closing a buffer restores its imported contents
from disk.

Clients supporting `workspace.didChangeWatchedFiles.dynamicRegistration` receive
file-watch registrations after initialization. These watch `.dodo` files under
open documents' source directories and loaded dependency directories, including
new imports and package siblings. Create, change, and delete events refresh
diagnostics and navigation without a save. Open buffers remain authoritative.
Watches are shared across documents and removed when no open document needs them.
Clients without dynamic registration can send `workspace/didChangeWatchedFiles`
notifications themselves, or save an open document to refresh dependencies.

Untitled buffers receive standalone analysis and editing tools.

## Completion, navigation, and refactoring

- Completion suggests visible declarations, local bindings, keywords, and types.
  Typing `.` requests members of an imported package or a typed receiver. Imported
  private declarations are excluded. Suggestions use the current unsaved buffer.
- Go-to-definition and references use source locations across the loaded package
  and imports. Local bindings retain their identity through nested shadowing.
  References honor the client's option to include the declaration.
- Bundled library definitions open read-only source documents, including navigation
  within those documents. The source comes from the running compiler, so no
  compiler checkout or separate standard-library installation is needed.
- Rename returns a workspace edit for the identified declaration and its indexed
  uses. It excludes comments and strings, rejects invalid names and possible name
  capture, and includes open-buffer versions when the client supports versioned
  document changes. The editor applies the edits; the server never writes files.
- Signature help triggers on `(` and `,`, selects the active argument, and omits
  the implicit receiver parameter for instance methods. It also works while a
  call's argument list is incomplete.

### Bundled sources in other clients

The VS Code extension handles bundled source documents automatically. Other
clients need a read-only document provider for `dodo-stdlib:` URIs. For example,
when a definition returns `dodo-stdlib:/std/math.dodo`, request its UTF-8 text with:

```json
{"jsonrpc":"2.0","id":2,"method":"dodo/stdlibSource","params":{"uri":"dodo-stdlib:/std/math.dodo"}}
```

The result is the source string. The server advertises this extension as
`capabilities.experimental.dodoStdlibSource: true`. Use the URI unchanged, assign
the `dodo` language, and synchronize open/close notifications to enable hover and
navigation inside it. Unknown or noncanonical URIs return `InvalidParams`.
Bundled text cannot be changed through document synchronization, formatting, or
rename. Compiler intrinsics without Dodo source have no source definition.

## Formatting

Use your editor's document-format command or format-on-save setting. The server
uses the same comment-preserving canonical formatter as `dodo fmt`, including its
syntax migration and indentation rules. Client indentation preferences do not
override the canonical style. It returns one whole-document edit, or no edits if
the buffer is already formatted. Invalid syntax produces a request error and
leaves the buffer untouched.

## Project manifest

The server reads `dodo.toml` from `rootUri` (or the first workspace folder), with
no parent search. `initializationOptions.buildTarget` selects a named target;
`initializationOptions.manifestPath` selects another manifest, relative to the
workspace root. `initializationOptions.target` overrides its LLVM triple.
VS Code exposes these as `dodo.buildTarget`, `dodo.manifestPath`, and
`dodo.target`; changing them restarts the server. This server-wide selection
applies across open documents, including multi-folder workspaces.

Manifest file-watch events reload the selected platform and refresh diagnostics.
Invalid edits report an error while keeping the last valid target. Editors that
do not support file-watch registration must restart the server after changing
configuration. Source files and unsaved imports retain the existing checking
mode below. Reading a manifest never executes a build, linker, or program.

## Compilation target

Set `initializationOptions.target` to the same LLVM triple used by your project's
`dodo check --target` or `dodo compile --target` command. For example:

```json
{"checkMode": "package", "target": "wasm32-unknown-unknown"}
```

The target controls both pointer-width checking (`usize`, `isize`, array bounds)
and selection of hosted imports such as `std/fs/native`. It applies to file,
package, dependency, fallback, and untitled analysis. Invalid triples are rejected
during initialization. Restart the server after
changing the initialization options. Without an explicit target, the server uses
the selected target from `dodo.toml` in the workspace root, otherwise the host.

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
there is no scan of unopened workspace roots or reverse dependencies. Rename conservatively
refuses possible collisions, generic parameters, destructuring bindings, and fields
or bindings requiring shorthand/pattern expansion. These restrictions avoid edits
that could change a different binding or leave a partial rename.

Checks remain synchronous and rebuild full-document analysis, including after
file-watch notifications; there is no incremental analysis. File watching is
performed by the editor client. See the [responsiveness measurements](lsp-performance.md)
and benchmark commands before changing the analysis model.

See [ownership diagnostics](diagnostics-and-editors.md) for annotated examples
of the errors shown in the editor.
