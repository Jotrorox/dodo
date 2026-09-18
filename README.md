# Dodo

Dodo is an ahead-of-time compiled systems language with checked borrowing,
explicit memory ownership, and native output. Start with small terminal programs;
use the same language for portable libraries, hosted applications, and low-level
hardware code.

**Compiler 0.1.3 · Language design 0.1 · [BSD-2-Clause](LICENSE)**

## Start in five minutes

[Install Dodo and a C toolchain](https://jotrorox.github.io/dodo/installation/),
create a folder named `hello`, and save this as `main.dodo` inside it:

```dodo test
package main

import "std/console"

fn main() {
    console.println("Hello, world!")!
}
```

From that folder, run:

```sh
dodo run
```

It prints `Hello, world!`. `println` returns a Result because output can fail;
postfix `!` takes success or panics on failure. The
[first-program tutorial](https://jotrorox.github.io/dodo/first-program/) explains
every line, shows how to change the program, and builds a persistent executable
on Linux or Windows.

The standard library is embedded in the compiler. Projects use ordinary folders
and imports; no manifest, lockfile, or package manager is needed.

## Learn the language

The [documentation](https://jotrorox.github.io/dodo/) has a sequential learning
path and a separate reference:

1. [Variables, values, and expressions](https://jotrorox.github.io/dodo/language-basics/)
2. [Types and functions](https://jotrorox.github.io/dodo/types-and-functions/)
3. [Control flow](https://jotrorox.github.io/dodo/control-flow/)
4. [Ownership and borrowing](https://jotrorox.github.io/dodo/ownership/)
5. [Patterns, options, and Results](https://jotrorox.github.io/dodo/patterns-and-results/)
6. [Generics](https://jotrorox.github.io/dodo/generics/)
7. [A complete temperature-report walkthrough](https://jotrorox.github.io/dodo/practical-program/)
8. [Packages and imports](https://jotrorox.github.io/dodo/packages/) and [testing](https://jotrorox.github.io/dodo/testing/)

Use the [glossary](https://jotrorox.github.io/dodo/glossary/) for unfamiliar terms,
the [syntax reference](https://jotrorox.github.io/dodo/implementation-syntax/) for
exact rules, and [diagnostics](https://jotrorox.github.io/dodo/diagnostics-and-editors/)
when a program is rejected. Runnable examples are also in [examples/](examples).

Dodo implements part of the broader 0.1 design. Read
[implementation decisions and limits](https://jotrorox.github.io/dodo/implementation/)
alongside the [language specification](https://jotrorox.github.io/dodo/language-spec-0.1/).
The specification is also available as
[plain text](https://jotrorox.github.io/dodo/downloads/language-spec-0.1.txt) and
[PDF](https://jotrorox.github.io/dodo/downloads/language-spec-0.1.pdf).

These docs follow `main`, which can contain APIs added after a release. Use the
[`v0.1.3` tag](https://github.com/Jotrorox/dodo/tree/v0.1.3) for released source
and [CHANGELOG.md](CHANGELOG.md) for version changes.

## Find a library

The [standard-library guide](https://jotrorox.github.io/dodo/standard-library/)
helps you choose a package. The
[API reference](https://jotrorox.github.io/dodo/stdlib-api/) contains exact public
signatures, types, fields, methods, and source contracts for every bundled source
package. It is generated from the library at each documentation build.

| Build with | Guides |
| --- | --- |
| Terminal input/output | [Console](https://jotrorox.github.io/dodo/console/), [formatting](https://jotrorox.github.io/dodo/formatting/) |
| Data and storage | [Bytes](https://jotrorox.github.io/dodo/bytes/), [UTF-8 text](https://jotrorox.github.io/dodo/text/), [JSON](https://jotrorox.github.io/dodo/json/), [collections](https://jotrorox.github.io/dodo/collections/), [allocation](https://jotrorox.github.io/dodo/allocation/) |
| Calculations | [Core](https://jotrorox.github.io/dodo/core/), [math](https://jotrorox.github.io/dodo/math/), [hashing](https://jotrorox.github.io/dodo/hash/), [time](https://jotrorox.github.io/dodo/time/) |
| OS services | [Files](https://jotrorox.github.io/dodo/filesystem/), [environment](https://jotrorox.github.io/dodo/environment/), [processes](https://jotrorox.github.io/dodo/processes/), [threads](https://jotrorox.github.io/dodo/threads/), [synchronization](https://jotrorox.github.io/dodo/synchronization/) |
| Network applications | [Sockets and DNS](https://jotrorox.github.io/dodo/networking/), [TLS](https://jotrorox.github.io/dodo/tls/), [HTTP](https://jotrorox.github.io/dodo/http/), [web servers](https://jotrorox.github.io/dodo/web/) |

Portable packages work with explicit storage and no required OS, global
allocator, or scheduler. Hosted adapters currently support Linux GNU x86-64 and
Windows x64. Each guide documents its actual target and dependency requirements.

## Everyday commands

```sh
dodo fmt
dodo check
dodo run
dodo test
dodo compile -O 2
dodo --help
```

`check`, `run`, and `compile` select `main.dodo` by default. Pass a filename or
project folder to choose another input. `build` is an alias for `compile`.
`dodo test` discovers named tests and explicitly executable Markdown examples.

See the [command-line guide](https://jotrorox.github.io/dodo/command-line/) for
output paths, debug information, optimization, cross-compilation, and linkers.
The [VS Code extension](editor-support/dodo-vscode) supplies highlighting and
connects to `dodo lsp`; other editors can use the same
[language server](https://jotrorox.github.io/dodo/editors/).

## Build and contribute

Frontend development needs Rust 1.98.1 and its platform linker. LLVM is optional
for parser, checker, formatter, package-loader, and editor-analysis tests:

```sh
cargo test --locked --no-default-features --all-targets
cargo clippy --locked --no-default-features --all-targets -- -D warnings
```

A full compiler build additionally needs LLVM 23 development files and a C
toolchain. The [source-build guide](docs/src/content/docs/building-from-source.md)
explains prerequisites, Windows setup, release builds, debugging checks, and CI.
After configuring `LLVM_SYS_231_PREFIX`:

```sh
cargo build --locked
cargo test --locked --all-targets
cargo run --locked -- test
cargo run --locked -- test -O 3
```

For repeatable JSON, routing, and loopback HTTP performance measurements, see
the [stdlib benchmark guide](benchmarks/README.md) and [recorded baseline](benchmarks/RESULTS.md).

To edit and verify documentation with Node.js 24+ and Python 3.10+:

```sh
npm ci --prefix docs
npm run build --prefix docs
python3 scripts/generate_api_docs.py --check
python3 scripts/render_spec.py --check
python3 scripts/check_docs.py
```

`npm run dev --prefix docs` previews the site. Follow
[Edit these docs](docs/src/content/docs/contributing.md) for writing standards,
executable examples, generated API pages, and navigation. The existing GitHub
Actions workflow publishes documentation changes on `main`.

## Repository layout

| Path | Contents |
| --- | --- |
| [src/](src) | Compiler, CLI, formatter, semantic checker, and language server. |
| [stdlib/](stdlib) | Bundled Dodo library sources and dependency notices. |
| [examples/](examples) | Small programs to read, check, and run. |
| [tests/](tests) | Acceptance, rejection, execution, and platform fixtures. |
| [docs/](docs) | Guides, canonical specification, generated API reference, and Astro site. |
| [editor-support/](editor-support) | VS Code extension and editor instructions. |
| [scripts/](scripts) | Documentation generation, release tooling, and validation. |
| [.github/workflows/](.github/workflows) | Compiler, editor, release, and documentation automation. |

Generated `target/`, `build/`, documentation build outputs, and extension packages
are ignored by Git. They can contain local toolchains or unfinished work; inspect
paths before deleting them. The [contributor guide](docs/src/content/docs/contributing.md)
explains which documentation files are generated.

Copyright © 2026 Johannes Müller and Dodo contributors. Licensed under [BSD-2-Clause](LICENSE).
