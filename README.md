# Dodo 0.1

Dodo is an ahead-of-time systems language compiler written in Rust with
[Inkwell](https://github.com/TheDan64/inkwell) and LLVM 22. It produces native
executables, object files, assembly, LLVM IR, and bitcode. Ordinary generated
code needs no garbage collector, heap allocator, scheduler, or Dodo runtime.

```dodo
package hello

fn main() -> i32 {
    values := [1i32, 2, 3, 4]
    total := 0i32
    for &item in values {
        total += item
    }
    return total
}
```

This is the first compiler release, **0.1.0**, licensed under
[BSD-2-Clause](LICENSE). The [language specification](https://jotrorox.github.io/dodo/language-spec-0.1/)
is a broader design contract. The implemented features and remaining limits are
explicit below; this release does not claim complete specification conformance
or a proof of memory safety.

## Build and install

**Users of a built `dodo` binary do not need to install LLVM.** LLVM is linked
statically, including all code generation targets. Copy the binary onto your
`PATH`; there is no LLVM installation or environment variable to configure.
Checking code and emitting objects, assembly, LLVM IR, or bitcode require no
external compiler tools. Building and running hosted executables still requires
a C toolchain (`cc`, or a driver selected with `--linker` / `DODO_CC`).

Building Dodo **from source** requires Rust 1.95.0 (pinned in
`rust-toolchain.toml`), LLVM 22 development files and static archives, and a C
toolchain. Inkwell provides LLVM bindings; the language server uses `lsp-server`,
`lsp-types`, `serde_json`, and `url`. `Cargo.lock` fixes the dependencies. A missing
LLVM static archive is a build error; the build never silently falls back to
shared LLVM.

On Fedora with LLVM 22 packages:

```sh
sudo dnf install llvm-devel llvm-static clang gcc gcc-c++ zlib-ng-compat-devel libzstd-devel libxml2-devel libffi-devel
export LLVM_SYS_221_PREFIX=/usr/lib64/llvm22
cargo build --locked --release
cargo install --locked --path .
```

On Ubuntu 24.04, install LLVM 22 from the
[official LLVM package repository](https://apt.llvm.org/) and a C compiler:

```sh
# After configuring the signed LLVM 22 apt repository:
sudo apt-get install llvm-22-dev libpolly-22-dev build-essential zlib1g-dev libzstd-dev libxml2-dev libffi-dev
export LLVM_SYS_221_PREFIX=/usr/lib/llvm-22
cargo build --locked --release
cargo install --locked --path .
```

The exact signed-repository setup used by CI is in
[ci.yml](.github/workflows/ci.yml). If LLVM is installed elsewhere, set
`LLVM_SYS_221_PREFIX` to its installation prefix containing `bin/llvm-config`.
Ordinary Cargo builds can still link LLVM's smaller support libraries (such as
zlib, zstd, and the C++ library) dynamically. Use the release recipe below to
bundle those as well.

### Size-focused compiler build

Use the `release-small` profile to minimize the Dodo compiler executable with
stable compiler options:

```sh
cargo build --locked --profile release-small --bin dodo
# Binary: target/release-small/dodo
cargo install --locked --path . --profile release-small
```

This profile uses size optimization (`opt-level = "z"`), full link-time
optimization, one codegen unit, and symbol stripping. Compiler panics abort the
process instead of unwinding. Builds may take longer and the compiler may run
more slowly. All LLVM targets remain included; prebuilt LLVM archives limit how
much Rust compiler options can reduce the final size. Actual size depends on the
platform and toolchain.

For the self-contained Linux recipe below, run
`bash scripts/build-release.sh release-small`; its binary is written to
`target/x86_64-unknown-linux-gnu/release-small/dodo`.

### Self-contained Linux release

On an x86-64 Ubuntu 24.04 build machine with the packages above, run:

```sh
export LLVM_SYS_221_PREFIX=/usr/lib/llvm-22
bash scripts/build-release.sh
install -Dm755 target/x86_64-unknown-linux-gnu/release/dodo "$HOME/.local/bin/dodo"
```

The resulting binary contains LLVM, the C++ standard library, zlib, zstd, and
any needed libffi code. Its only permitted shared dependencies are the standard
Linux C runtime (`libc.so.6`), math library (`libm.so.6`), unwind library
(`libgcc_s.so.1`), and ELF loader. The tested runtime baseline is x86-64 Linux
with glibc 2.39 or newer (Ubuntu 24.04). LLVM and its support packages are only
needed on the build machine. Keeping every LLVM target increases the binary
size compared with the old shared-LLVM build.

The script requires Python 3 and `readelf` (binutils) for verification and honors
`LLVM_SYS_221_PREFIX`, `CC`, and `CARGO_TARGET_DIR`. Other native x86-64 GNU/Linux
build hosts need the same development libraries, including `libz.a`,
`libzstd.a`, `libstdc++.a`, and `libffi.a`. Their runtime glibc requirement depends
on the build host. Other platforms can use the ordinary Cargo build above;
the reduced support-library dependency list is only enforced for this Linux
release recipe.

The release build checks its ELF dependencies and fails if an unexpected shared
library remains. CI also copies it into a clean Ubuntu container, checks code
and emits native and WebAssembly objects before installing any compiler tools,
then builds and runs a program with only the C toolchain added.

### Fully static Linux compiler

On a native x86-64 GNU/Linux build host, use:

```sh
export LLVM_SYS_221_PREFIX=/usr/lib/llvm-22 # Fedora: /usr/lib64/llvm22
bash scripts/build-release.sh release-small-static
# Binary: target/x86_64-unknown-linux-gnu/release-small-static/dodo
```

`release-small-static` inherits all `release-small` size settings. The script
adds static C-runtime linking and static relocation, producing a non-PIE
executable, and links LLVM's support libraries statically. It rejects a binary
with any shared-library dependency or ELF interpreter. Use the script: selecting
the Cargo profile alone does not supply the required Linux linker flags.

Install static archives for the C and math runtimes, the C++ standard library,
zlib, zstd, libffi, and any additional libraries listed by
`llvm-config --link-static --system-libs`. Ubuntu 24.04's development packages
listed above provide these archives; LLVM builds with libxml2 support also need
`libxml2-dev`. On Fedora, additionally install `glibc-static`,
`libstdc++-static`, `zlib-ng-compat-static`, `libzstd-static`, and
`libxml2-static` if LLVM uses libxml2. If your distribution does not ship
`libffi.a`, build a static libffi. Archives in custom locations can be made
available through `LIBRARY_PATH`.

All LLVM targets remain included. This profile minimizes size through compiler
and linker options; the size still depends on the prebuilt LLVM archives and
platform libraries. It includes glibc, so it can be larger than `release-small`
despite having no shared-library dependencies. Checking code and emitting
compiler artifacts need no external tools; linking and running Dodo programs
still needs a C toolchain. Only native x86-64 GNU/Linux builds are supported by
this recipe.

## Use

```sh
dodo --help
dodo fmt examples
dodo fmt --check examples
dodo check examples/samples.dodo
dodo run examples/hello.dodo
dodo build examples/hello.dodo -O 2 -o build/hello
./build/hello
```

`fmt` formats Dodo files and automatically migrates legacy declarations, array
literals, and explicit generic calls to their canonical spellings. Omit its path
to format the current directory. Directory inputs recursively include `.dodo`
files, skipping hidden directories, `target`, `build`, and symlinks. `--check`
reports files needing formatting and exits with status 1 without writing;
`--stdout` previews a single file, and `dodo fmt -` reads stdin and writes stdout.
All inputs are parsed before any source file is replaced. Comments and literal
spellings are preserved.

Canonical syntax uses `name: Type`, bracket arrays, and `function::<Type>()` for
explicit generic arguments; inference and explicit annotations remain available.
A typed array can be written `values: [2]u16 = [1, 2]` or, in any expression,
`([1, 2]: [2]u16)`. Legacy forms remain accepted in 0.1; formatting provides the
migration path before a future deprecation.

Use `for &value in values` (or `for index, &value in values`) to copy copyable
shared elements. Ordinary iteration still binds references; mutable iteration
still uses `for value in &mut values`. A newline before `.` continues a field or
method chain, including across comments and blank lines; `;` ends the expression.
See [ergonomics.dodo](examples/ergonomics.dodo) for a complete example.

`check` parses and validates a file or package directory without requiring an
entry point. `build` defaults to `build/<source-name>`; it writes the final output
only after compilation and linking succeed. `run` uses a temporary executable,
cleans it up, and returns the program's exit status. Arguments after `--` are
passed to the executable. Hosted entry points are `fn main()` (optionally
`-> void`) or `fn main() -> i32`; there is no built-in argument-access library yet.

```sh
dodo build examples/gpio.dodo --emit llvm-ir -o build/gpio.ll
dodo build examples/gpio.dodo --emit obj -o build/gpio.o
dodo build examples/fibonacci.dodo --emit asm -O 3 -o build/fibonacci.s
dodo build examples/hex.dodo --emit bitcode -o build/hex.bc
dodo build examples/fibonacci.dodo --emit obj --target wasm32-unknown-unknown
```

Choose `-O 0`, `1`, `2`, or `3`. Overflow, division, shift, conversion, and bounds
checks remain active at every level; LLVM may remove a check only when it proves
it redundant. Traps use `llvm.trap` and do not unwind.

`--target`, `--cpu`, and `--features` select LLVM code generation. Use `--linker`
(or `DODO_CC`) and repeatable `--link-arg` options for custom linking. Cross-target
objects do not require a host entry point or C runtime; linking firmware still
requires the platform's startup code, linker script, and appropriate linker.
`run` executes only the host target. Linux x86-64 native execution and wasm32
object generation are covered by tests; other LLVM targets are not validated.

## Editor diagnostics (LSP)

Configure your editor's LSP client to launch `dodo` with the argument `--lsp`
(or use `dodo lsp`). No input path is required. The server uses standard
[LSP](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.17/specification/)
messages over stdin/stdout; transport errors and logs go to stderr.

The server checks unsaved text on open and edit, refreshes diagnostics on save,
and releases buffers on close. Diagnostics include source ranges, severity, and
compiler notes, with related labels for borrow origins, live uses, moves, and
borrowed-return contracts. Hovers show inferred types, receiver ownership, and
explicit or inferred borrowed-return sources. Local imports use other open buffers when available, and edits
recheck all open files so diagnostics in callers stay current. Source files and
build artifacts are never written by the server.

By default, each document is checked like `dodo check <file>`, including its
imports. For projects that use directory packages, set your LSP client's
`initializationOptions` to `{"checkMode": "package"}`. This checks each open
document's parent directory like `dodo check <directory>`, including unsaved new
`.dodo` siblings. All files in a directory package must declare the same package.

This initial implementation supports local `file:` URIs, full document
synchronization, UTF-16 positions, and host pointer width. Checks stop at the
first compiler error per file/package. Error and warning severities are
supported, but the compiler currently only produces errors; no new warning
rules are introduced. Completion, navigation, incremental analysis, file
watching, and diagnostics for unopened workspace roots are not implemented yet.
See [diagnostics and editor setup](https://jotrorox.github.io/dodo/diagnostics-and-editors/) for labeled
examples and hover details.

## Language support

- Fixed-width and pointer-sized integers, floats, Booleans, byte/string literals,
  arrays, checked references, slices, and raw pointers.
- Consistent name-first declarations, concise receivers and field literals,
  inferred array literals, copy-only array repetition, named constant expressions,
  definite initialization, moves, structs with methods, enums with payloads, `Option`,
  `Result`, `?`, and exhaustive `match`.
- Immutable runtime `let` bindings alongside mutable `:=` locals and `const`.
  Recursive enum/struct patterns, ranges, alternatives, guards, `if let`, and
  `let ... else` share ownership and mandatory Result checks.
- Explicit copy patterns for shared collection iteration, leading-dot method
  chains, and `dodo fmt` with automatic syntax migration.
- All specified `for` forms plus integer ranges, checked subslices, `break`,
  `continue`, value-producing conditionals/matches/blocks, explicit
  and final-expression returns, short-circuit Boolean expressions, and checked numeric conversions.
- Shared/exclusive borrow checking, separate struct-field loans, reborrowing,
  last-use loan expiry, borrowed-return `from(...)` contracts, and borrow
  dependencies carried through aggregates.
- Deterministic destruction in reverse declaration order, custom `drop`,
  destruction before overwrite, and cleanup on returns, propagation, and loop
  exits. Moved values are not destroyed twice.
- Generics through monomorphization, including structs and methods, with local
  type-argument inference and explicit arguments available. Option constructors
  infer their payload types.
- Local packages, public/private declarations and fields, unsafe blocks and
  functions, primitive/raw-pointer C ABI calls, `@repr(C)` struct layout,
  volatile MMIO, and a small compiler-provided memory/pointer core.

See [implementation decisions](https://jotrorox.github.io/dodo/implementation/) for exact lexical,
operator, package, layout, intrinsic, and release-limit details. Compiler errors
include labeled source snippets for borrow origins, conflicting accesses, live
uses, moves, and borrowed-return contracts. Run `dodo lsp` from an editor's LSP
client for inferred types, receiver ownership, and borrowed-return source hovers.
See [diagnostics and editor setup](https://jotrorox.github.io/dodo/diagnostics-and-editors/) and the
[borrowing example](examples/borrowing.dodo).

## Documentation

Read the **[documentation website](https://jotrorox.github.io/dodo/)** for the
language specification, implementation limits, requirements, and editor setup.
The specification includes the ergonomics revision and remaining open design
items. It is also available as [plain text](https://jotrorox.github.io/dodo/downloads/language-spec-0.1.txt)
and [PDF](https://jotrorox.github.io/dodo/downloads/language-spec-0.1.pdf).

The entire `docs/` directory is a small Astro website. Edit or add Markdown files
in [docs/src/content/docs](docs/src/content/docs); frontmatter sets each page's
`title`, `description`, and numeric `order`. Navigation and browser search update
automatically. Use Node.js 24 and Python 3.10 or newer to preview and build:

```sh
cd docs
npm ci
npm run dev
npm run build
npm run preview
```

See [Edit these docs](https://jotrorox.github.io/dodo/contributing/) for authoring
and keyboard shortcut details. GitHub Actions validates pull requests and deploys
the generated website from `main` directly to this repository's GitHub Pages.
After editing the
[specification source](docs/src/content/docs/language-spec-0.1.md), the next website
build automatically regenerates its PDF and plain text downloads. Generated
files are ignored by Git, so only the Markdown edit needs committing. The
exporter uses Python's standard library and needs no third-party dependencies.

## Development

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo build --locked --release
python3 scripts/render_spec.py
python3 scripts/render_spec.py --check
python3 scripts/test_check_linkage.py
bash scripts/build-release.sh # x86-64 GNU/Linux release dependency check
```

CI runs these checks. Tests include rejected programs, specification examples,
native execution at `-O0` and `-O3`, destructor ordering, error propagation,
cross-target object emission, local imports, and expected runtime traps. They
exercise the actual compiler and generated binaries rather than matching LLVM
text alone. On systems that save core dumps, `ulimit -c 0` before running the
trap tests avoids creating crash artifacts.

`cargo test --locked --test lsp` exercises the real compiler's LSP lifecycle,
framing, buffer updates, imported diagnostics, directory packages, Unicode
positions, and recovery from invalid notifications.

The pipeline is organized into [`lexer`](src/lexer.rs),
[`parser`](src/parser.rs), [`package`](src/package.rs),
[`prepare`](src/prepare.rs), [`sema`](src/sema.rs),
[`consteval`](src/consteval.rs), and [`codegen`](src/codegen.rs), with a reusable
library, an [`LSP server`](src/lsp.rs), and a small CLI driver.
