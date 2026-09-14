# Dodo for Zed

Language support for [Dodo](https://github.com/Jotrorox/dodo), using the compiler's
built-in `dodo lsp` server and a bundled Tree-sitter grammar.

- Syntax highlighting for `.dodo` files, including declarations, types, attributes,
  strings, byte literals, numeric literals, borrowing, and operators.
- Diagnostics, completion, hover, definition, references, rename, signature help,
  and document formatting through `dodo lsp`.
- Comment toggling, bracket pairing, indentation, function/type outlines, and
  Vim function, class, and comment text objects.

## Install from this checkout

Install [Dodo](https://jotrorox.github.io/dodo/installation/) 0.1.2 or newer and
make sure `dodo --version` works. Install a current Zed (0.205 or newer), Rust via
rustup, Python 3.11 or newer, and Git. The extension uses an existing compiler; it does not
download Dodo or require the compiler's LLVM development libraries.

From the repository root, run:

```sh
python3 editor-support/dodo-zed/prepare-dev.py
```

In Zed's command palette, run **zed: install dev extension** and select the
generated **build/dodo-zed** directory. Zed compiles the extension and grammar,
installing the Rust WebAssembly target and WASI SDK as needed. Open a `.dodo` file.
Highlighting also works when the compiler is not installed.

The preparation script copies the extension and creates a local Git snapshot of
the grammar under `build/tree-sitter-dodo`. The generated manifest points to this
snapshot with a `file://` URL and exact revision. This lets Zed build unpublished
grammar changes without changing the source manifest or committing your work.
Keep both build directories available while using the dev extension. Rerun the
script after editing extension sources, then rebuild the dev extension in Zed's
Extensions page.

This extension is not yet published in Zed's extension registry. The source
manifest's `main` grammar reference is intended for development after these files
land upstream. Before registry publication, pin `grammars.dodo.rev` to a pushed
commit containing `editor-support/tree-sitter-dodo/src/parser.c` and its headers.
Use the prepared directory for local installation, including before that commit
exists upstream.

## Configure the language server

The extension finds `dodo` on the worktree's PATH and runs `dodo lsp`. To use a
specific compiler or change checking mode and target, add to Zed's settings:

```json
{
  "lsp": {
    "dodo": {
      "binary": {
        "path": "/absolute/path/to/dodo"
      },
      "initialization_options": {
        "checkMode": "package",
        "target": "wasm32-unknown-unknown"
      }
    }
  }
}
```

Set only the executable path, without shell quotes or arguments. Paths with
spaces work. On Windows, use `dodo.exe` and JSON-escaped backslashes or forward
slashes. For remote projects, install Dodo on the remote host.

Omit `initialization_options` to check each file and its imports for the host
target. Package mode checks immediate sibling `.dodo` files, which must declare
the same package. Restart the language server after changing initialization
options. The server's options and limits are described in the
[editor guide](https://jotrorox.github.io/dodo/editors/).

For wrappers, `lsp.dodo.binary.arguments` replaces the default `["lsp"]` argument
list and `lsp.dodo.binary.env` supplies additional environment variables.

Use Zed's document formatting action for Dodo's canonical four-space style.
To format on save:

```json
{
  "languages": {
    "Dodo": {
      "formatter": "language_server",
      "format_on_save": "on"
    }
  }
}
```

For startup errors, run **zed: open log** and check the compiler path. Use
**editor: restart language server** after installing or rebuilding Dodo.

## Develop and verify

The extension is a standalone Cargo package. Building it does not build Dodo or
link LLVM. From the repository root:

```sh
cargo fmt --manifest-path editor-support/dodo-zed/Cargo.toml -- --check
cargo test --locked --manifest-path editor-support/dodo-zed/Cargo.toml
cargo clippy --locked --manifest-path editor-support/dodo-zed/Cargo.toml --all-targets -- -D warnings
rustup target add wasm32-wasip2
cargo build --locked --release --target wasm32-wasip2 --manifest-path editor-support/dodo-zed/Cargo.toml
python3 -m unittest discover -s editor-support/dodo-zed -p 'test_*.py'
```

To regenerate and test the grammar, install Node.js 22 or newer and a C compiler:

```sh
cd editor-support/tree-sitter-dodo
npm ci
npm test
```

Check in generated `src` files when changing `grammar.js`. Grammar tests cover
declaration structure, literal handling, highlighting captures, all repository
examples/stdlib/test `.dodo` files, every Zed query, and outline ranges. The
preparation tests verify that Zed can fetch the local snapshot and that source
edits produce a new immutable revision.

The Tree-sitter grammar is deliberately permissive: it recognizes declarations,
return types, balanced groups, and tokens rather than duplicating the compiler's
expression parser or enforcing statement newlines. This keeps syntax highlighting
available during incomplete edits. The LSP provides authoritative syntax and
semantic diagnostics; highlighting for user-defined types in expression bodies
uses capitalization conventions.

The implementation follows Zed's [language extension documentation](https://zed.dev/docs/extensions/languages)
and [development guide](https://zed.dev/docs/extensions/developing-extensions).

## License

[BSD-2-Clause](LICENSE), like Dodo. Tree-sitter's generated parser headers use
the [MIT license](LICENSE.tree-sitter).
