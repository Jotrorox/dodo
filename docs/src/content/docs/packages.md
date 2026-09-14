---
title: "Projects and imports"
description: "Start with main.dodo, organize shared code in subfolders, and import local packages."
section: "Using Dodo"
order: 130
---

A Dodo project is an ordinary folder with a `main.dodo` entry file. There is no
manifest, lockfile, package manager, dependency cache, or separate library
project type. Every source file still starts with a `package` declaration.

## Project entry and subfolders

Run `dodo run`, `dodo check`, or `dodo compile` from the project folder to use
`main.dodo`. `dodo build` is an alias for `dodo compile`. Passing a project
folder explicitly selects the same entry: `dodo run app` uses `app/main.dodo`.
Passing a file, such as `dodo run app/main.dodo`, uses that file directly.

Only the entry file and its imports are loaded. Other files beside `main.dodo`
and unimported subfolders are not included automatically. Missing `main.dodo`
is an error; the compiler does not search parent folders or infer another entry.

Put shared code in subfolders and import their paths. An imported folder's
immediate `.dodo` files form one package, in sorted order. They must all declare
the same package and share declarations and import aliases. Nested folders are
included only through imports. No filename is special inside an imported
folder: it needs neither `main.dodo` nor `lib.dodo`, and a folder named `lib`
behaves like any other local folder.

## Local imports

`import "math"` resolves relative to the importing package directory to either
`math.dodo` or a `math/` directory containing `.dodo` files. Nested import paths
are supported; the imported package declaration must match the final path
component. An ambiguous file-and-directory match, cycle, duplicate import alias,
or conflicting package mapping is diagnosed. Dependencies are local files;
there is no registry, network resolver, or re-export mechanism.
Only directly imported package names are available in a package.

## Visibility and methods

Declarations and fields are private unless `pub`; public functions cannot expose
private types. Public enum variants are available with the enum. Struct methods
are statically dispatched; associated functions use `Type.name(...)`.

## Compiler-provided packages

`core/mmio`, `core/ptr`, and `core/mem` are compiler-provided imports.
`core.drop(value)` destroys an owned value early. Intrinsics are ordinary checked
calls with compiler lowering, not user-definable macros. See [memory and foreign calls](memory-and-ffi.md#implemented-core-calls)
for the supported intrinsic signatures.

`core.wrapping_add`, `core.wrapping_sub`, and `core.wrapping_mul` provide
[explicit wrapping integer arithmetic](standard-library.md#portable-core-utilities)
without an import.

The compiler also embeds the Dodo source packages listed in
[the standard library](standard-library.md). Imports beginning with `core/`,
`alloc/`, or `std/` always resolve from this bundled library, independent of the current
directory; local files cannot shadow them. Unknown standard imports are errors.
Only the imported packages and their dependencies are loaded.

Package names `core`, `mem`, `ptr`, and `mmio` are reserved. Other package identities follow their resolved paths.
Names default to the final path component; `import "core/bytes" as raw` assigns
a package-local alias. This allows importing `std/bytes` and `core/bytes`
together, and allows transitive dependencies to use their own aliases without
conflicts. Importing two distinct paths under the same local name is an error;
assign an explicit alias to disambiguate. An alias does not rename the imported
package declaration and does not grant access to private declarations. Aliases
are shared across files in one directory package; inconsistent aliases for the
same path are rejected. Each package must directly import the
intrinsics it uses; a dependency's import does not grant access to its callers.

## A project with shared code

Create this directory layout:

```text
app/
  main.dodo
  math/
    answer.dodo
    base.dodo
```

Put the entry point in `app/main.dodo`:

```dodo
package main

import "math"

fn main() -> i32 {
    math.answer() - 42
}
```

Put the public function in `app/math/answer.dodo`:

```dodo
package math

pub fn answer() -> i32 {
    base() + 2
}
```

Put its private helper in `app/math/base.dodo`:

```dodo
package math

fn base() -> i32 {
    40
}
```

Both files in `math/` are loaded by `import "math"`. The entry file can call
`math.answer()` because it is public. The helper is available within `math/`.

Check, run, and compile from the project folder:

```sh
cd app
dodo check
dodo run
dodo compile
./build/app
```

The executable is named after the project folder: `build/app`.
The program exits successfully with no output. You can add more ordinary
subfolders in the same way; there is no library registration step.

The [editor setup guide](editors.md#file-and-package-checking) explains how to
check the entry file and combine files while editing an imported folder.
