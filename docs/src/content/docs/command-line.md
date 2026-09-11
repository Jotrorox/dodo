---
title: "Use the command line"
description: "Format, check, run, and build Dodo programs; select output formats and target platforms."
section: "Using Dodo"
order: 100
---

The examples using `hello.dodo` build on [your first program](first-program.md).
Run commands from the directory containing that file. Use `dodo --help` for the
complete option list and `dodo --version` for the compiler version.

## Everyday commands

| Command | What it does |
| --- | --- |
| `dodo fmt hello.dodo` | Format the source file. |
| `dodo check hello.dodo` | Check syntax, types, ownership, and borrowing. |
| `dodo run hello.dodo` | Build a temporary executable and run it. |
| `dodo build hello.dodo` | Keep an executable at `build/hello`. |
| `dodo lsp` | Start the language server for an editor. |

`check` does not need a `main` function or a C toolchain. Building and running an
executable require a C toolchain and a hosted entry point: `fn main()`,
`fn main() -> void`, or `fn main() -> i32`.

`build` writes its final output only after compilation and linking succeed.
Choose a different location with `-o` or `--output`:

```sh
dodo build hello.dodo -o build/my-program
./build/my-program
```

`run` cleans up its temporary executable and returns the program's exit status.
Arguments after `--` are passed to the executable, although Dodo does not yet
provide a built-in library for reading them:

```sh
dodo run hello.dodo -- example-argument
```

For editor configuration and protocol support, see [editor setup](editors.md).
See [compiler diagnostics](diagnostics-and-editors.md) for error examples.

## Files and packages

Pass one source file or one package directory to `check`, `build`, or `run`.
A file input includes that file and its imports. A directory input combines the
immediate `.dodo` files in that directory; they must declare the same package.
Subdirectories are not automatically included.

```sh
dodo check .
dodo build . -o build/my-program
```

Dependencies resolve through local imports. See
[packages and imports](packages.md) for import
resolution and package restrictions.

## Format source

`fmt` accepts a file or directory. With no path, it formats the current directory.
Directory inputs recursively include `.dodo` files, skipping hidden directories,
`target`, `build`, and symlinks.

```sh
dodo fmt hello.dodo
dodo fmt .
dodo fmt --check .
dodo fmt --stdout hello.dodo
```

`--check` reports files needing formatting and exits with status 1 without
writing. `--stdout` previews a single file without changing it. Use `dodo fmt -`
to read stdin and write the formatted source to stdout.

The formatter parses every input before replacing any source file. It preserves
comments and literal spellings, and automatically migrates legacy declarations,
array literals, and explicit generic calls to their canonical spellings:
`name: Type`, `[1, 2]`, and `function::<Type>()`. Legacy forms remain accepted in
language version 0.1. See
[syntax and expressions](implementation-syntax.md) for syntax
details and the migration policy.

## Choose a compiler output

`build` produces an executable by default. Other output formats do not require
an entry point or an external linker:

| `--emit` value | Output |
| --- | --- |
| `exe` | Native executable; requires a C toolchain. |
| `obj` | Object file. |
| `asm` | Assembly. |
| `llvm-ir` | Textual LLVM IR. |
| `bitcode` | LLVM bitcode. |

```sh
dodo build hello.dodo --emit llvm-ir -o build/hello.ll
dodo build hello.dodo --emit obj -o build/hello.o
dodo build hello.dodo --emit asm -o build/hello.s
dodo build hello.dodo --emit bitcode -o build/hello.bc
```

These compiler artifacts contain no startup code. Use executable output when
you want to run a hosted program directly.

## Set optimization

Choose `-O 0`, `1`, `2`, or `3`. The default is `0`:

```sh
dodo build hello.dodo -O 2 -o build/hello
```

Overflow, division, shift, conversion, and bounds checks remain active at every
level. LLVM may remove a check only when it proves the check redundant. A failed
runtime check traps using `llvm.trap`; it does not unwind or run destructors.

## Targets and linking

`--target`, `--cpu`, and `--features` select LLVM code generation. All LLVM
targets are included in the compiler. For example, emit a WebAssembly object:

```sh
dodo build hello.dodo --emit obj --target wasm32-unknown-unknown -o build/hello.wasm
```

Cross-target object generation needs no host entry point or C runtime. Linking
firmware still requires the platform's startup code, linker script, and an
appropriate linker. `run` executes only the host target. Linux x86-64 native
execution and wasm32 object generation are covered by tests; other LLVM targets
have not been validated.

For executables, `--linker` selects the C linker driver, overriding `DODO_CC` and
the default `cc`. Use repeatable `--link-arg` options to pass arguments to it:

```sh
dodo build hello.dodo --linker cc --link-arg -s -o build/hello
```

The argument `-s` in this Linux example asks the linker to strip symbols. Linker
arguments depend on the selected toolchain.
