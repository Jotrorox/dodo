---
title: "Use the command line"
description: "Format, check, run, and compile Dodo projects; select output formats and target platforms."
section: "Using Dodo"
order: 100
---

These examples build on [your first program](first-program.md). Run commands
from the `hello` project folder containing `main.dodo`. Use `dodo --help` for the
complete option list and `dodo --version` for the compiler version.

## Everyday commands

| Command | What it does |
| --- | --- |
| `dodo fmt` | Format the project folder recursively. |
| `dodo check` | Check syntax, types, ownership, and borrowing. |
| `dodo run` | Build a temporary executable and run it. |
| `dodo test` | Discover and run tests recursively, in isolated processes. |
| `dodo compile` | Keep an executable at `build/hello`. |
| `dodo build` | Alias for `dodo compile`. |
| `dodo lsp` | Start the language server for an editor. |

`check` does not need a `main` function or a C toolchain. Building and running an
executable require a C toolchain and a hosted entry point: `fn main()`,
`fn main() -> void`, or `fn main() -> i32`.

`compile` writes its final output only after compilation and linking succeed.
Choose a different location with `-o` or `--output`:

```sh
dodo compile -o build/my-program
./build/my-program
```

`run` cleans up its temporary executable and returns the program's exit status.
Arguments after `--` are passed to the executable;
[`std/env`](environment.md) provides access to them:

```sh
dodo run -- example-argument
```

For editor configuration and protocol support, see [editor setup](editors.md).
See [compiler diagnostics](diagnostics-and-editors.md) for error examples.

`dodo test` scans the current directory for test functions and explicitly
executable documentation examples. Use `dodo test --list` to inspect discovery,
`--filter TEXT` to select tests, and `dodo test --help` for its options. See
[Test your code](testing.md) for assertions, companion test files, and failure
reports. Testing does not require `main.dodo`.

## Project folders and source files

With no input path, `check`, `compile`, `build`, and `run` select `main.dodo`
in the current folder. Compiler options can follow the command directly.
An explicit directory also selects its `main.dodo`. A project requires no
manifest, lockfile, package manager, or special library folder.

```sh
dodo check
dodo run -O 2
dodo compile --emit llvm-ir
dodo run .
dodo compile path/to/project -o build/my-program
dodo run examples/hello.dodo
```

The entry file and its imports are loaded. Other files beside `main.dodo` and
unimported subfolders are not automatically included. If `main.dodo` is missing,
the command reports an error; it does not search parent folders, `src/`, or
alternate entry names. Pass an explicit source file to use another filename.

Shared code lives in ordinary imported subfolders. An imported folder combines
its immediate `.dodo` files into one package; those files must declare the same
package, and need no `main.dodo` or `lib.dodo`. See
[projects and imports](packages.md) for a complete example and import rules.

The default output uses the folder containing `main.dodo`: a project named
`hello` produces `build/hello`. This applies whether you omit the input, pass a
project folder, or pass its `main.dodo` explicitly. Other explicit source files
use `build/<source name>`. Artifact extensions are appended to the full name,
such as `build/hello.ll` for LLVM IR.

Output paths are relative to the shell's current folder, even when compiling
another project folder. Use `-o` to choose a different path.

## Format source

`fmt` accepts a file or directory. With no path, it formats the current directory.
Directory inputs recursively include `.dodo` files, skipping hidden directories,
`target`, `build`, and symlinks.

```sh
dodo fmt main.dodo
dodo fmt .
dodo fmt --check .
dodo fmt --stdout main.dodo
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

`compile` produces an executable by default. Other output formats do not require
an entry point or an external linker:

| `--emit` value | Output |
| --- | --- |
| `exe` | Native executable; requires a C toolchain. |
| `obj` | Object file. |
| `asm` | Assembly. |
| `llvm-ir` | Textual LLVM IR. |
| `bitcode` | LLVM bitcode. |

```sh
dodo compile --emit llvm-ir -o build/hello.ll
dodo compile --emit obj -o build/hello.o
dodo compile --emit asm -o build/hello.s
dodo compile --emit bitcode -o build/hello.bc
```

These compiler artifacts contain no startup code. Use executable output when
you want to run a hosted program directly.

## Set optimization

Choose `-O 0`, `1`, `2`, or `3`. The default is `0`:

```sh
dodo compile -O 2 -o build/hello
```

Overflow, division, shift, conversion, and bounds checks remain active at every
level. LLVM may remove a check only when it proves the check redundant. A failed
runtime check traps using `llvm.trap`; it does not unwind or run destructors.

## Targets and linking

`--target`, `--cpu`, and `--features` select LLVM code generation. All LLVM
targets are included in the compiler. For example, emit a WebAssembly object:

```sh
dodo compile --emit obj --target wasm32-unknown-unknown -o build/hello.wasm
```

Cross-target object generation needs no host entry point or C runtime. Linking
firmware still requires the platform's startup code, linker script, and an
appropriate linker. `run` executes only the host target. Linux x86-64 native
execution and wasm32 object generation are covered by tests; other LLVM targets
have not been validated.

For executables, `--linker` selects the C linker driver, overriding `DODO_CC` and
the default `cc`. Use repeatable `--link-arg` options to pass arguments to it:

```sh
dodo compile --linker cc --link-arg -s -o build/hello
```

The argument `-s` in this Linux example asks the linker to strip symbols. Linker
arguments depend on the selected toolchain.
