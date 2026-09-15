# Dodo

Dodo is an ahead-of-time systems language with checked borrowing and explicit
hardware access. Its Rust and LLVM 23 compiler produces native executables,
object files, assembly, LLVM IR, and bitcode. Ordinary generated code needs no
garbage collector, heap allocator, scheduler, or Dodo runtime.

**Compiler release: 0.1.2 · Language version: 0.1 · [BSD-2-Clause](LICENSE)**

This README describes `main`, which can include changes made after the latest
release. Use the [`v0.1.2` tag](https://github.com/Jotrorox/dodo/tree/v0.1.2) for
the released source and [CHANGELOG.md](CHANGELOG.md) for release notes.

## Get started

Download a compiler from [GitHub Releases](https://github.com/Jotrorox/dodo/releases)
and follow the [installation guide](https://jotrorox.github.io/dodo/installation/).
Prebuilt compilers include LLVM; running Dodo programs also requires a C
toolchain such as `cc`.

Create a project folder named `hello` and save this as `main.dodo` inside it:

```dodo
package main

import "std/console"

fn main() -> i32 {
    match console.println("Hello, world!") {
        ok(_) => { return 0 }
        err(_) => { return 1 }
    }
}
```

Run it from the same directory:

```sh
dodo run
```

The program prints `Hello, world!` and exits successfully. `console.println`
returns a Result: `ok` reports the written byte count, and `err` reports an I/O
failure. The example returns exit status 1 if printing fails. A project needs
only `main.dodo`: no manifest, lockfile, or package manager. Put shared code in
ordinary subfolders and import it by path.
The [first program guide](https://jotrorox.github.io/dodo/first-program/)
explains each line and shows how to check the program and keep an executable.
Use the [console guide](docs/src/content/docs/console.md) to print primitive values,
report errors to stderr, and read a line into a fixed buffer. Hosted console
access supports Linux GNU x86-64 and Windows x64.

## Everyday commands

```sh
dodo fmt
dodo check
dodo test
dodo compile -O 2
./build/hello
dodo --help
```

`run`, `check`, and `compile` default to `main.dodo` in the current folder.
`build` remains an alias for `compile`. An explicit folder selects its
`main.dodo`; you can also pass another source file directly. The default output
uses the project folder name: `hello/main.dodo` compiles to `build/hello`.

`dodo test` scans the current folder recursively for `@test` functions,
`test_` functions, and Markdown examples marked `dodo test`. Add a test beside
your code and run it without a manifest or a test dependency:

```dodo test
package example

@test
fn adds_numbers() {
    assert_eq(20 + 22, 42)
}
```

Each test runs in its own process, so a trap fails only that test. Use
`dodo test --list`, `--filter TEXT`, and `--show-output` to inspect the suite.
The [testing guide](https://jotrorox.github.io/dodo/testing/) covers companion
test files, assertions, failure locations, timeouts, and executable docs.

Use [the command-line guide](https://jotrorox.github.io/dodo/command-line/) for
formatting options, project folders, compiler outputs, optimization, and
cross-target builds. Add `-g -O 0` to debug generated programs with source
breakpoints and local variables. Hosted runtime checks report their kind and
location; embedded builds can select a non-returning C ABI board handler with
`--panic-hook` for fault reporting, halt, or reset behavior. For
diagnostics, completion, navigation, rename, signature help, and formatting,
install the [Dodo VS Code extension](editor-support/dodo-vscode) or
[Dodo Zed extension](editor-support/dodo-zed), which also add syntax highlighting,
or configure your editor's LSP client to run
`dodo lsp` and follow the
[editor setup guide](https://jotrorox.github.io/dodo/editors/).

## Language and reference

Dodo supports numeric types, arrays and slices, structs and enums, generics,
pattern matching, local packages, checked references, deterministic destruction,
and explicit unsafe operations for foreign calls and hardware access. The
compiler implements a subset of the broader language design; its current limits
and conservative borrow checking are documented alongside the specification.

- [Documentation home](https://jotrorox.github.io/dodo/): guides and reference pages.
- [Implementation decisions and limits](https://jotrorox.github.io/dodo/implementation/): supported behavior and remaining work.
- [Language specification](https://jotrorox.github.io/dodo/language-spec-0.1/): the Dodo 0.1 design, also available as [plain text](https://jotrorox.github.io/dodo/downloads/language-spec-0.1.txt) and [PDF](https://jotrorox.github.io/dodo/downloads/language-spec-0.1.pdf).
- [Examples](examples): programs covering borrowing, patterns, generics, and hardware access.
- [Standard library](https://jotrorox.github.io/dodo/standard-library/): portable foundations, explicit allocation, I/O, text, collections, mathematics, hashing, time, networking, TLS, HTTP/1.1 and web routing, with independently selected operating-system and execution providers.
- [Mutable slice splitting](docs/src/content/docs/slice-splitting.md): checked, disjoint mutable views with ordinary ownership and source lifetimes.
- [Container element safety](docs/src/content/docs/container-elements.md): current restrictions and proposed checker changes; reference and Result elements remain limited.

## Build and contribute

Source builds require Rust 1.98.1, LLVM 23 development files, and a C toolchain.
Cargo prefers static LLVM and allows a shared-library fallback. After installing
the prerequisites and setting `LLVM_SYS_231_PREFIX` for your LLVM installation:

```sh
git clone https://github.com/Jotrorox/dodo.git
cd dodo
cargo build --locked --release
cargo install --locked --path .
```

The [source build guide](https://jotrorox.github.io/dodo/building-from-source/)
contains Fedora and Ubuntu prerequisites, size-focused and fully static Linux
builds, runtime requirements, and development checks. Its
[Markdown source](docs/src/content/docs/building-from-source.md) is available in
this checkout. Run the core development checks from the repository root:

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo run --locked -- test
cargo run --locked -- test -O 3
```

Editor integrations have separate checks documented in the
[VS Code](editor-support/dodo-vscode/README.md),
[Zed](editor-support/dodo-zed/README.md), and
[Tree-sitter grammar](editor-support/tree-sitter-dodo/README.md) READMEs.

To change the website, edit Markdown in
[docs/src/content/docs](docs/src/content/docs) and follow
[Edit these docs](https://jotrorox.github.io/dodo/contributing/) for local preview,
navigation, and specification downloads. With Node.js 24 or newer and Python
3.10 or newer installed, build and validate the website from the repository root:

```sh
npm ci --prefix docs
npm run build --prefix docs
python3 scripts/render_spec.py --check
python3 scripts/check_docs.py
```

Use `npm run dev --prefix docs` for local preview. GitHub Actions checks changes
and publishes documentation changes from `main`.

## Repository layout

| Path | Contents |
| --- | --- |
| [src/](src) | Compiler library, CLI, formatter, and language server. Semantic checker helpers live in `src/sema/`. |
| [stdlib/](stdlib) | Embedded Dodo standard library and its dependency notices. |
| [tests/](tests) | Compiler regression tests, native programs, and standard-library fixtures. |
| [examples/](examples) | Small Dodo programs to read, check, and run. |
| [docs/](docs) | Astro website, Markdown guides, and the canonical language specification. |
| [editor-support/](editor-support) | VS Code and Zed integrations and the Tree-sitter grammar. |
| [scripts/](scripts) | Release packaging, platform checks, documentation validation, and benchmarks. |
| [.github/workflows/](.github/workflows) | Compiler, editor, release, and documentation automation. |

The [semantic checker experiment](docs/sema-flow-prototype.md) explains the
test-only control-flow prototype in `tests/support/flow/`. It is development
evidence and does not replace the production checker.

## Generated files and repository cleanup

The following local outputs are ignored by Git and can be regenerated when they
are no longer needed:

| Output | Recreate with |
| --- | --- |
| `target/` | Cargo build or test commands. If you installed LLVM under `target/llvm-linux`, removing `target/` also removes that toolchain. |
| `build/` | The compiler or release scripts that produced each output; `python3 editor-support/dodo-zed/prepare-dev.py` recreates the Zed development directories. Keep those directories while using the dev extension. |
| `docs/node_modules/`, `docs/.astro/`, `docs/dist/` | `npm ci --prefix docs` followed by `npm run build --prefix docs`. |
| `docs/public/downloads/language-spec-0.1.txt` and `.pdf` | `python3 scripts/render_spec.py` or the website build. |
| `editor-support/dodo-vscode/node_modules/`, `dist/`, `.vscode-test/`, and `*.vsix` | The extension's install, build, integration-test, and packaging commands. |
| `editor-support/dodo-zed/target/`, `extension.wasm`, and `grammars/` | The Zed extension's Cargo build or Zed's dev-extension build. |
| `editor-support/tree-sitter-dodo/node_modules/` and compiled grammar libraries | `npm ci` and `npm test` in the grammar directory. |
| Python `__pycache__/` directories | Running the corresponding Python scripts. |

Keep tracked source files, lockfiles, licenses, and the generated Tree-sitter
files under `editor-support/tree-sitter-dodo/src/`: those parser files are needed
to build the extension. Commit or back up unfinished work before cleaning a
checkout. `git clean -ndX` previews ignored outputs; remove only the paths you
have reviewed. `cargo clean` removes Cargo build outputs.

To synchronize a clean `main` checkout and inspect branches and worktrees:

```sh
git switch main
git fetch --prune origin
git merge --ff-only origin/main
git worktree list
git branch --merged origin/main
```

After a feature is merged and its worktree has no unfinished work, remove that
worktree with `git worktree remove PATH`, then its local branch with
`git branch -d BRANCH`. Delete the merged remote branch with
`git push origin --delete BRANCH` if it still exists. Use `git worktree prune`
to clear registrations for worktree directories that were already removed.

---

Copyright © 2026 Johannes Müller and Dodo contributors. Licensed under [BSD-2-Clause](LICENSE).
