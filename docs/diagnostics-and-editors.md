# Ownership diagnostics and editor hovers

`dodo check` shows the code responsible for ownership errors. A primary label
uses `^` for the rejected operation; related labels use `-` to explain where the
borrow or move began and which use still needs it. Every snippet includes its
source filename and line/column. Labels from imported files retain their own
locations.

For example, run this intentionally invalid program:

```sh
dodo check examples/diagnostics/borrow_conflict.dodo
```

The diagnostic connects these three expressions:

```text
view := &value
        ------ shared borrow begins here

value = 2
^^^^^ cannot modify `value` while this borrow is live

return consume(view)
               ---- borrow is used here
```

Move the modification after the last use of `view`, or move that use before the
modification. For struct fields, borrowing independent fields can also avoid a
conflict. A loop can keep a borrow live into its next iteration, and a custom
destructor can need borrowed data at scope exit; diagnostics explain those
reasons when there is no later ordinary use.

Moves and borrowed returns have related labels too:

```sh
dodo check examples/diagnostics/moved_value.dodo
dodo check examples/diagnostics/return_source.dodo
```

The first command connects the ownership transfer to the rejected second use.
The second connects the returned borrow to its parameter and the `from(a)`
contract that excludes it. Return an allowed source, or change the contract to
describe the function's intended inputs. `from(static)` cannot make a reference
to local storage survive its destruction.

These three files are rejection examples: each command should fail. The working
[borrowing example](../examples/borrowing.dodo) demonstrates shared, mutable, and
consuming receivers, inferred and explicit return sources, and ending a borrow
before mutation:

```sh
dodo check examples/borrowing.dodo
dodo run examples/borrowing.dodo
```

## Editors

Configure an editor's Language Server Protocol client to start `dodo lsp` for
`.dodo` files. The process speaks LSP over standard input/output; it takes no
source filename. Set the server executable to `dodo`, its argument list to
`["lsp"]` (or `["--lsp"]`), and the language/filetype to `dodo` in the client's
configuration.

Hover information includes:

- The inferred type of a local binding or expression, such as `view: &i32`.
- A method's receiver behavior: a shared borrow, a mutable borrow, or consumption
  of `self`.
- Borrowed-return sources such as `from(self)` or `from(a, b)`, including whether
  the contract was inferred or explicitly written.

Open [borrowing.dodo](../examples/borrowing.dodo) and hover `view`, the method
names `view`, `replace`, and `finish`, and the `choose` call to inspect these
details. Ownership errors appear as editor diagnostics; their related locations
point to the same source expressions as the command-line labels.

The server supports initialization, shutdown, full-document open/change/close
synchronization, save notifications, hover, and published diagnostics. It accepts
local `file:` URIs and uses zero-based LSP line numbers, UTF-16 columns, and the
host's pointer width. Open buffers supply unsaved source, including new files;
local imports use those buffers when available. Changes recheck open documents,
saves refresh diagnostics, and closing a buffer restores its imported contents
from disk.

Untitled buffers receive standalone analysis, including diagnostics and hovers.

By default each document is checked as a file with its imports, like
`dodo check <file>`. To combine sibling files as a directory package, set the
client's `initializationOptions` to `{"checkMode": "package"}`. This checks the
document's parent directory, including new unsaved `.dodo` siblings. All files in
that directory package must declare the same package.

Checks stop at the first compiler error per file or package. Error and warning
severities are supported, although the compiler currently only produces errors.
The server does not provide completion, rename, formatting, incremental analysis,
a background file watcher, or checks of unopened workspace roots. Syntax errors
can prevent hover analysis; a semantic error can leave later expressions without
inferred types.

Library consumers can inspect `Diagnostic.labels` directly instead of parsing
terminal output. Spans are byte offsets into the source passed to the parser;
package-loaded spans use the offsets in `Loaded.sources`. Render package errors
with `Loaded::render` so each label resolves to the correct file.
