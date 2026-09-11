---
title: "Packages and imports"
description: "Organize Dodo source files, resolve local imports, and control public declarations."
section: "Using Dodo"
order: 130
---

Each source file declares a package. You can compile a single file with its
imports or combine sibling files into one directory package.

## Files and directory packages

A file input compiles that file and its imports. A directory input combines its
immediate `.dodo` files, in sorted order, into one package. All files in that
unit must declare the same package. Subdirectories are not implicitly included.

## Local imports

`import "math"` resolves relative to the importing package directory to either
`math.dodo` or a `math/` directory containing `.dodo` files. Nested import paths
are supported; the imported package declaration must match the final path
component. An ambiguous file-and-directory match, cycle, duplicate import alias,
or conflicting package mapping is diagnosed. Dependencies are local files;
there is no registry, network resolver, alias syntax, or re-export mechanism.
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

The compiler also embeds the Dodo source packages listed in
[the portable standard library](standard-library.md). Imports beginning with
`core/`, `alloc/`, or `std/` always resolve from this bundled library, independent of the current
directory; local files cannot shadow them. Unknown standard imports are errors.
Only the imported packages and their dependencies are loaded.

Package names `core`, `mem`, `ptr`, and `mmio` are reserved. Other package names
still use the final path component globally: two distinct imported packages
named `layout`, for example, conflict. Each package must directly import the
intrinsics it uses; a dependency's import does not grant access to its callers.

## A two-file example

Create this directory layout:

```text
app/
  main.dodo
  math.dodo
```

Put the entry point in `app/main.dodo`:

```dodo
package app

import "math"

fn main() -> i32 {
    math.answer() - 42
}
```

Put the imported package in `app/math.dodo`:

```dodo
package math

pub fn answer() -> i32 {
    42
}
```

Check and run the entry file:

```sh
dodo check app/main.dodo
dodo run app/main.dodo
```

The program exits successfully with no output. Use the file path here:
`dodo check app` would combine both files into one package and reject their
different package declarations. To compile `app` as a directory, place the
`math` package in `app/math/` instead.

The [editor setup guide](editors.md#file-and-package-checking) explains how to
select the same file or directory behavior for editor checks.
