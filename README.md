# Dodo 0.1

Dodo is an ahead-of-time systems language compiler written in Rust with
[Inkwell](https://github.com/TheDan64/inkwell) and LLVM 22. It produces native
executables, object files, assembly, LLVM IR, and bitcode. Ordinary generated
code needs no garbage collector, heap allocator, scheduler, or Dodo runtime.

```dodo
package hello

fn main() -> i32 {
    values := [4]i32{1, 2, 3, 4}
    total := 0i32
    for item in values {
        total += *item
    }
    return total
}
```

This is the first compiler release, **0.1.0**, licensed under
[BSD-2-Clause](LICENSE). The [language specification](docs/language-spec-0.1.md)
is a broader design contract. The implemented features and remaining limits are
explicit below; this release does not claim complete specification conformance
or a proof of memory safety.

## Build and install

Prerequisites: Rust 1.95.0 (pinned in `rust-toolchain.toml`), LLVM 22 development
files, and a C toolchain for linking hosted executables. Inkwell is the only
direct Cargo dependency; `Cargo.lock` fixes the transitive dependencies.

On Fedora with LLVM 22 packages:

```sh
sudo dnf install llvm-devel clang gcc
export LLVM_SYS_221_PREFIX=/usr/lib64/llvm22
cargo build --locked --release
cargo install --locked --path .
```

On Ubuntu 24.04, install LLVM 22 from the
[official LLVM package repository](https://apt.llvm.org/) and a C compiler:

```sh
# After configuring the signed LLVM 22 apt repository:
sudo apt-get install llvm-22-dev clang-22 build-essential
export LLVM_SYS_221_PREFIX=/usr/lib/llvm-22
cargo build --locked --release
cargo install --locked --path .
```

The exact signed-repository setup used by CI is in
[ci.yml](.github/workflows/ci.yml). If LLVM is installed elsewhere, set
`LLVM_SYS_221_PREFIX` to its installation prefix containing `bin/llvm-config`.
Dodo links dynamically to LLVM; keep the matching LLVM runtime installed.

## Use

```sh
dodo --help
dodo check examples/samples.dodo
dodo run examples/hello.dodo
dodo build examples/hello.dodo -O 2 -o build/hello
./build/hello
```

`check` parses and validates a file or package directory without requiring an
entry point. `build` defaults to `build/<source-name>`; it writes the final output
only after compilation and linking succeed. `run` uses a temporary executable,
cleans it up, and returns the program's exit status. Arguments after `--` are
passed to the executable. Hosted entry points are `fn main() -> void` or
`fn main() -> i32`; there is no built-in argument-access library yet.

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

## Language support

- Fixed-width and pointer-sized integers, floats, Booleans, byte/string literals,
  arrays, checked references, slices, and raw pointers.
- Explicitly typed and inferred bindings, constant expressions, definite
  initialization, moves, structs with methods, enums with payloads, `Option`,
  `Result`, `?`, and exhaustive `match`.
- All specified `for` forms, `break`, `continue`, conditional blocks, explicit
  returns, short-circuit Boolean expressions, and checked numeric conversions.
- Shared/exclusive borrow checking, separate struct-field loans, reborrowing,
  last-use loan expiry, borrowed-return `from(...)` contracts, and borrow
  dependencies carried through aggregates.
- Deterministic destruction in reverse declaration order, custom `drop`,
  destruction before overwrite, and cleanup on returns, propagation, and loop
  exits. Moved values are not destroyed twice.
- Explicit type-argument generics through monomorphization, including generic
  structs and their methods.
- Local packages, public/private declarations and fields, unsafe blocks and
  functions, primitive/raw-pointer C ABI calls, `@repr(C)` struct layout,
  volatile MMIO, and a small compiler-provided memory/pointer core.

See [implementation decisions](docs/implementation.md) for exact lexical,
operator, package, layout, intrinsic, and release-limit details. Compiler errors
include source filenames, line/column locations, and notes for borrow conflicts.

## Documentation

The cleaned specification is maintained in three formats in `docs/`:

- [Markdown](docs/language-spec-0.1.md)
- [Plain text](docs/language-spec-0.1.txt)
- [PDF](docs/language-spec-0.1.pdf)

It preserves the supplied specification's requirements and unresolved items.
[Requirements](docs/spec-requirements.md) organize the original obligations;
[implementation decisions](docs/implementation.md) distinguish compiler choices
and current restrictions. Regenerate the text and PDF with
`python3 scripts/render_spec.py`; no document dependencies are needed.

## Development

```sh
cargo fmt --all --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets
cargo build --locked --release
python3 scripts/render_spec.py --check
```

CI runs these checks. Tests include rejected programs, specification examples,
native execution at `-O0` and `-O3`, destructor ordering, error propagation,
cross-target object emission, local imports, and expected runtime traps. They
exercise the actual compiler and generated binaries rather than matching LLVM
text alone. On systems that save core dumps, `ulimit -c 0` before running the
trap tests avoids creating crash artifacts.

The pipeline is organized into [`lexer`](src/lexer.rs),
[`parser`](src/parser.rs), [`package`](src/package.rs),
[`sema`](src/sema.rs), [`consteval`](src/consteval.rs), and
[`codegen`](src/codegen.rs), with a reusable library and a small CLI driver.
