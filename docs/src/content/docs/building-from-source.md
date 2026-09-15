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
git checkout v0.1.2
```

The tag selects compiler release 0.1.2. For compiler development, use `main`
instead. Run the commands on this page from the repository root.

## Install build prerequisites

Building Dodo **from source** requires Rust 1.98.1 (pinned in
`rust-toolchain.toml`), LLVM 23 development files and a C toolchain.
[`llvm-sys`](https://docs.rs/crate/llvm-sys/latest) provides the LLVM C API
bindings; the language server implements its JSON encoding and decoding,
JSON-RPC messages, LSP parameter validation, and file
URI conversion internally using Rust's standard library. `Cargo.lock`
fixes the dependencies. Ordinary Cargo builds prefer static LLVM and fall back
to a shared LLVM library if static linking is unavailable. A compiler linked to
shared LLVM needs that library installed at runtime.

On Fedora with LLVM 23 packages:

```sh
sudo dnf install llvm-devel llvm-static clang gcc gcc-c++ zlib-ng-compat-devel libzstd-devel libxml2-devel libffi-devel
export LLVM_SYS_231_PREFIX=/usr/lib64/llvm23
cargo build --locked --release
cargo install --locked --path .
```

On Ubuntu 24.04, install LLVM 23 from the
[official LLVM package repository](https://apt.llvm.org/) and a C compiler:

```sh
# After configuring the signed LLVM 23 apt repository:
sudo apt-get install llvm-23-dev libpolly-23-dev build-essential zlib1g-dev libzstd-dev libxml2-dev libffi-dev
export LLVM_SYS_231_PREFIX=/usr/lib/llvm-23
cargo build --locked --release
cargo install --locked --path .
```

The Ubuntu packages are suitable for ordinary Cargo builds. CI uses the
[official prebuilt LLVM archive](#fully-static-linux-compiler) for releases,
so the compiler does not depend on Z3. If LLVM is installed elsewhere, set
`LLVM_SYS_231_PREFIX` to its installation prefix containing `bin/llvm-config`.
To prefer shared LLVM explicitly, use:

```sh
cargo build --locked --release --features llvm-sys/prefer-dynamic
```

Use `--features llvm-sys/force-static` to require static LLVM. The release
packaging recipes select this feature explicitly. These linking features are
mutually exclusive; select only one per build.

Ordinary Cargo builds can also link LLVM's smaller support libraries (such as
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

On an x86-64 Ubuntu 24.04 build machine, install the
[release toolchain below](#fully-static-linux-compiler), then run:

```sh
export LLVM_SYS_231_PREFIX="$PWD/target/llvm-linux"
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
`LLVM_SYS_231_PREFIX`, `CC`, and `CARGO_TARGET_DIR`. Other native x86-64 GNU/Linux
build hosts need the same development libraries, including `libz.a`,
`libzstd.a`, `libstdc++.a`, and `libffi.a`. Their runtime glibc requirement depends
on the build host. Other platforms can use the ordinary Cargo build above;
the reduced support-library dependency list is only enforced for this Linux
release recipe.

The release build checks its ELF dependencies and fails if an unexpected shared
library remains. CI also copies it into a clean Ubuntu container, checks code
and emits native and WebAssembly objects before installing any compiler tools,
then builds and runs a program with only the C toolchain added.

## Windows release archive

The Windows release targets `x86_64-pc-windows-msvc`. It embeds LLVM and links
the compiler's C and C++ runtimes statically. Building it requires the Visual
Studio C++ Build Tools with a Windows SDK, Rust 1.98.1, and the official
`clang+llvm-23.1.1-x86_64-pc-windows-msvc.tar.xz` development archive from
[LLVM 23.1.1](https://github.com/llvm/llvm-project/releases/tag/llvmorg-23.1.1).
Use the development archive containing `llvm-config.exe` and static libraries.
The CI workflow pins and verifies its download.

From PowerShell, after extracting LLVM to `C:\llvm-23`:

```powershell
$env:LLVM_SYS_231_PREFIX = "C:\llvm-23"
$env:PATH = "$env:LLVM_SYS_231_PREFIX\bin;$env:PATH"
$env:CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_RUSTFLAGS = "-C target-feature=+crt-static"
rustup target add x86_64-pc-windows-msvc
rustup component add rust-docs
python scripts/build-windows-llvm-support.py
cargo build --locked --features llvm-sys/force-static --release --bin dodo --target x86_64-pc-windows-msvc
python scripts/package-release.py --target x86_64-pc-windows-msvc
python scripts/test_windows_release.py build/release-assets/dodo-0.1.2-x86_64-pc-windows-msvc.zip --linker clang
```

The support script requires CMake and Visual Studio 2022. It builds the static
XML support library omitted from LLVM's archive, using the same configuration
as LLVM's Windows release, and installs it and the dependency notices into
`LLVM_SYS_231_PREFIX`. For cross builds, its `--cmake-toolchain` option accepts
an MSVC CMake toolchain such as the one generated by `cargo-xwin`.

The packaging script writes the Windows ZIP to `build/release-assets/` alongside
any Linux tarball. Both archives include the compiler, installation
instructions, and dependency notices. It does not produce a `SHA256SUMS` file.
The default `python3 scripts/package-release.py` still packages the Ubuntu
Linux release.

The Windows archive test extracts to a temporary directory, checks the version
and embedded standard library, emits native and WebAssembly objects, and builds
and runs a program at `-O 0` and `-O 3`. On Linux, pass `--runner /path/to/wine`
to both packaging and testing to use a cross-built Windows compiler. Set
`WINEPREFIX` to an isolated prefix, supply a Windows Clang executable with
`--linker`, and use repeated `--link-arg=ARG` options for the MSVC SDK and runtime
library search paths required by that toolchain.

## Fully static Linux compiler

Install static archives for the C and math runtimes, the C++ standard library,
zlib, zstd, libffi, and any additional libraries listed by
`llvm-config --link-static --system-libs`.

CI uses the official LLVM 23.1.1 Linux x86-64 archive, which has Z3 disabled.
On Ubuntu 24.04, install its prerequisites and the same toolchain:

```sh
sudo apt-get install build-essential zlib1g-dev libzstd-dev libxml2-dev libffi-dev python3 curl xz-utils
export LLVM_SYS_231_PREFIX="$PWD/target/llvm-linux"
python3 scripts/install-linux-llvm.py
export PATH="$LLVM_SYS_231_PREFIX/bin:$PATH"
```

The installer verifies pinned SHA-256 checksums and extracts LLVM's static
libraries, headers, required tools, and license. The download is about 1.9 GB;
CI caches the extracted installation. To reuse a downloaded archive, pass
`--archive /path/to/LLVM-23.1.1-Linux-X64.tar.xz`. No LLVM or Z3 source build is
needed.

Ubuntu's `llvm-23-dev` package enables Z3 and cannot use this fully static recipe
without an additional static Z3 library. Use the official archive above or an
LLVM build configured with
[`-DLLVM_ENABLE_Z3_SOLVER=OFF`](https://llvm.org/docs/CMake.html#llvm-enable-z3-solver).
This is an LLVM build option, not a Cargo setting. The release script checks all
libraries reported by `llvm-config`; it does not ignore missing dependencies.

On Fedora, additionally install `glibc-static`, `libstdc++-static`,
`zlib-ng-compat-static`, `libzstd-static`, and `libxml2-static` if LLVM uses
libxml2. If your distribution does not ship `libffi.a`, build a static libffi.
Archives in custom locations can be made available through `LIBRARY_PATH`.

On a native x86-64 GNU/Linux build host, use:

```sh
# Keep LLVM_SYS_231_PREFIX set to the selected LLVM installation.
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
and recovery from invalid source and notifications. Independent JSON-RPC fixtures
also check request IDs, malformed envelopes, parameter types, and atomic buffer
updates; unit tests cover bounded framing and file/untitled URI validation. Use the
[LSP benchmark](lsp-performance.md) to measure responsiveness separately from
correctness tests.

The pipeline is organized into [`lexer`](https://github.com/Jotrorox/dodo/blob/main/src/lexer.rs),
[`parser`](https://github.com/Jotrorox/dodo/blob/main/src/parser.rs), [`package`](https://github.com/Jotrorox/dodo/blob/main/src/package.rs),
[`prepare`](https://github.com/Jotrorox/dodo/blob/main/src/prepare.rs), [`sema`](https://github.com/Jotrorox/dodo/blob/main/src/sema.rs),
[`consteval`](https://github.com/Jotrorox/dodo/blob/main/src/consteval.rs), and [`codegen`](https://github.com/Jotrorox/dodo/blob/main/src/codegen.rs), with a reusable
library, an [`LSP server`](https://github.com/Jotrorox/dodo/blob/main/src/lsp.rs), and a small CLI driver.

For documentation changes and website checks, see [Edit these docs](contributing.md).

Debugger smoke tests in `tests/debugging.rs` run batch GDB sessions and validate
DWARF with `llvm-dwarfdump-23`. Install both tools and run:

```sh
DODO_REQUIRE_DEBUGGER_TESTS=1 cargo test --locked --test debugging
```

CI requires these tools. Local runs skip the relevant smoke tests if a tool is
missing, unless `DODO_REQUIRE_DEBUGGER_TESTS` is set. Runtime-reporting and panic
hook tests always run.
