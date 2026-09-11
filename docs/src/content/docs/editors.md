---
title: "Editor setup"
description: "Configure dodo lsp for diagnostics, type hovers, and file or package checking."
section: "Using Dodo"
order: 120
---

Dodo includes a Language Server Protocol (LSP) server. After
[installing the compiler](installation.md), connect it to your editor's LSP
client to get diagnostics and ownership information for `.dodo` files.

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
synchronization, save notifications, hover, and published diagnostics. It accepts
local `file:` URIs and uses zero-based LSP line numbers, UTF-16 columns, and the
host's pointer width. Open buffers supply unsaved source, including new files;
local imports use those buffers when available. Changes recheck open documents,
saves refresh diagnostics, and closing a buffer restores its imported contents
from disk.

Untitled buffers receive standalone analysis, including diagnostics and hovers.

## File and package checking

By default each document is checked as a file with its imports, like
`dodo check <file>`. To combine sibling files as a directory package, set the
client's `initializationOptions` to `{"checkMode": "package"}`. This checks the
document's parent directory, including new unsaved `.dodo` siblings. All files in
that directory package must declare the same package.

## Current limits

Checks stop at the first compiler error per file or package. Error and warning
severities are supported, although the compiler currently only produces errors.
The server does not provide completion, rename, formatting, incremental analysis,
a background file watcher, or checks of unopened workspace roots. Syntax errors
can prevent hover analysis; a semantic error can leave later expressions without
inferred types.

See [ownership diagnostics](diagnostics-and-editors.md) for annotated examples
of the errors shown in the editor.
