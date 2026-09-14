---
title: "Build Dodo from source"
description: "Install build prerequisites, choose a compiler profile, and run development checks."
section: "Project"
order: 310
---

Build from source to work on the compiler or choose its linking profile. To use
a downloaded compiler, follow [installation](installation.md) instead.

## Get the source

```sh
git clone https://github.com/Jotrorox/dodo.git
cd dodo
git checkout v0.1.1
```

The tag selects compiler release 0.1.1. For compiler development, use `main`
instead. Run the commands on this page from the repository root.

## Install build prerequisites

Building Dodo **from source** requires Rust 1.95.0 (pinned in
`rust-toolchain.toml`), LLVM 22 development files and static archives, and a C
toolchain. Inkwell provides LLVM bindings; the language server uses `lsp-server`,
`lsp-types`, and `serde_json`, with internal file URI conversion. `Cargo.lock`
fixes the dependencies. A missing LLVM static archive is a build error; the build
never silently falls back to shared LLVM.

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
[CI workflow](https://github.com/Jotrorox/dodo/blob/main/.github/workflows/ci.yml). If LLVM is installed elsewhere, set
`LLVM_SYS_221_PREFIX` to its installation prefix containing `bin/llvm-config`.
Ordinary Cargo builds can still link LLVM's smaller support libraries (such as
zlib, zstd, and the C++ library) dynamically. Use the [self-contained Linux recipe](#self-contained-linux-release) to
bundle those as well.

## Size-focused compiler build

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

## Self-contained Linux release

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

## Fully static Linux compiler

Install static archives for the C and math runtimes, the C++ standard library,
zlib, zstd, libffi, and any additional libraries listed by
`llvm-config --link-static --system-libs`.

Ubuntu 24.04's development packages above provide most of these archives.
LLVM packages with Z3 support also need a static Z3 library, which Ubuntu's
`libz3-dev` package does not include. Build the pinned Z3 version with the
repository helper, then add its archive directory to the library search path:

```sh
sudo apt-get install cmake ninja-build curl
bash scripts/build-static-z3.sh
export LIBRARY_PATH="$PWD/target/static-deps/lib${LIBRARY_PATH:+:$LIBRARY_PATH}"
```

The helper downloads Z3 4.8.12 from a fixed upstream commit, checks its SHA-256
checksum, and installs the static library, headers, and license under
`target/static-deps/`. It builds with two parallel jobs by default; set
`CMAKE_BUILD_PARALLEL_LEVEL` to change this. CI uses the same helper.
LLVM builds with libxml2 support also need `libxml2-dev`.

On Fedora, additionally install `glibc-static`, `libstdc++-static`,
`zlib-ng-compat-static`, `libzstd-static`, and `libxml2-static` if LLVM uses
libxml2. If your distribution does not ship `libffi.a`, build a static libffi.
Archives in custom locations can be made available through `LIBRARY_PATH`.

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

All LLVM targets remain included. This profile minimizes size through compiler
and linker options; the size still depends on the prebuilt LLVM archives and
platform libraries. It includes glibc, so it can be larger than `release-small`
despite having no shared-library dependencies. Checking code and emitting
compiler artifacts need no external tools; linking and running Dodo programs
still needs a C toolchain. Only native x86-64 GNU/Linux builds are supported by
this recipe.

## Development

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo run --locked -- test
cargo run --locked -- test -O 3
cargo build --locked --release
python3 scripts/render_spec.py
python3 scripts/render_spec.py --check
python3 scripts/test_check_linkage.py
python3 scripts/test_build_release.py
bash scripts/build-release.sh # x86-64 GNU/Linux release dependency check
```

CI runs these checks, including native Dodo tests and executable documentation.
Tests include rejected programs, specification examples,
native execution at `-O0` and `-O3`, destructor ordering, error propagation,
cross-target object emission, local imports, and expected runtime traps. They
exercise the actual compiler and generated binaries rather than matching LLVM
text alone. On systems that save core dumps, `ulimit -c 0` before running the
trap tests avoids creating crash artifacts.

`cargo test --locked --test lsp` exercises the real compiler's LSP lifecycle,
framing, buffer updates, imported diagnostics, directory packages, Unicode
positions, completion, navigation, rename, signatures, formatting, target selection,
and recovery from invalid source and notifications. Use the
[LSP benchmark](lsp-performance.md) to measure responsiveness separately from
correctness tests.

The pipeline is organized into [`lexer`](https://github.com/Jotrorox/dodo/blob/main/src/lexer.rs),
[`parser`](https://github.com/Jotrorox/dodo/blob/main/src/parser.rs), [`package`](https://github.com/Jotrorox/dodo/blob/main/src/package.rs),
[`prepare`](https://github.com/Jotrorox/dodo/blob/main/src/prepare.rs), [`sema`](https://github.com/Jotrorox/dodo/blob/main/src/sema.rs),
[`consteval`](https://github.com/Jotrorox/dodo/blob/main/src/consteval.rs), and [`codegen`](https://github.com/Jotrorox/dodo/blob/main/src/codegen.rs), with a reusable
library, an [`LSP server`](https://github.com/Jotrorox/dodo/blob/main/src/lsp.rs), and a small CLI driver.

For documentation changes and website checks, see [Edit these docs](contributing.md).

Debugger smoke tests in `tests/debugging.rs` run batch GDB sessions and validate
DWARF with `llvm-dwarfdump-22`. Install both tools and run:

```sh
DODO_REQUIRE_DEBUGGER_TESTS=1 cargo test --locked --test debugging
```

CI requires these tools. Local runs skip the relevant smoke tests if a tool is
missing, unless `DODO_REQUIRE_DEBUGGER_TESTS` is set. Runtime-reporting and panic
hook tests always run.
