# Dodo

Dodo is an ahead-of-time systems language with checked borrowing and explicit
hardware access. Its Rust and LLVM 22 compiler produces native executables,
object files, assembly, LLVM IR, and bitcode. Ordinary generated code needs no
garbage collector, heap allocator, scheduler, or Dodo runtime.

**Compiler release: 0.1.1 · Language version: 0.1 · [BSD-2-Clause](LICENSE)**

## Get started

Download a compiler from [GitHub Releases](https://github.com/Jotrorox/dodo/releases)
and follow the [installation guide](https://jotrorox.github.io/dodo/installation/).
Prebuilt compilers include LLVM; running Dodo programs also requires a C
toolchain such as `cc`.

Save this as `hello.dodo`:

```dodo
package hello

fn main() -> i32 {
    return 0
}
```

Run it from the same directory:

```sh
dodo run hello.dodo
```

The program prints nothing and exits successfully. `return 0` supplies its
success exit status. The [first program guide](https://jotrorox.github.io/dodo/first-program/)
explains each line and shows how to check the program and keep an executable.

## Everyday commands

```sh
dodo fmt hello.dodo
dodo check hello.dodo
dodo build hello.dodo -O 2 -o build/hello
./build/hello
dodo --help
```

Use [the command-line guide](https://jotrorox.github.io/dodo/command-line/) for
formatting options, package directories, compiler outputs, optimization, and
cross-target builds. For diagnostics and type hovers in an editor, configure its
LSP client to run `dodo lsp` and follow the
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
- [Core and allocation](https://jotrorox.github.io/dodo/standard-library/): bundled portable utilities, uninitialized storage, and explicit arenas, pools, and owned boxes.

## Build and contribute

Source builds require Rust 1.95.0, LLVM 22 development files and static archives,
and a C toolchain. After installing the prerequisites and setting
`LLVM_SYS_221_PREFIX` for your LLVM installation:

```sh
cargo build --locked --release
cargo install --locked --path .
```

The [source build guide](https://jotrorox.github.io/dodo/building-from-source/)
contains Fedora and Ubuntu prerequisites, size-focused and fully static Linux
builds, runtime requirements, and development checks. Its
[Markdown source](docs/src/content/docs/building-from-source.md) is available in
this checkout.

To change the website, edit Markdown in
[docs/src/content/docs](docs/src/content/docs) and follow
[Edit these docs](https://jotrorox.github.io/dodo/contributing/) for local preview,
navigation, and specification downloads. GitHub Actions checks changes and
publishes the website from `main`.

---

Copyright © 2026 Johannes Müller and Dodo contributors. Licensed under [BSD-2-Clause](LICENSE).
