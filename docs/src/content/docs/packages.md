---
title: "Projects and imports"
description: "Start with main.dodo, organize shared code in subfolders, and import local packages."
section: "Using Dodo"
order: 130
---

A Dodo project is an ordinary folder with a `main.dodo` entry file. Start with one
file, then move reusable code into imported files or subfolders as the program
grows. Every source file starts with a `package` declaration.

Three names have different jobs:

| Name | Example | Meaning |
| --- | --- | --- |
| Entry filename | `main.dodo` | The file selected by `dodo run` in a project folder. |
| Package declaration | `package math` | The package named by a source unit. |
| Import path and local qualifier | `import "math"`, then `math.answer()` | Find a dependency and access its public declarations. |

There is no manifest, lockfile, package manager, dependency cache, or separate
library project type. Follow the [complete shared-code example](#a-project-with-shared-code)
below to make a two-package project.

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

Paths use `/` even on Windows. They must be relative, with nonempty components;
absolute paths, `.` and `..` components, and backslashes are rejected. A local
import is resolved from the importing package's directory, so organize shared
dependencies within that directory tree. The compiler does not fetch remote
dependencies.

For a single-file dependency:

```text
app/
  main.dodo       // imports "math"
  math.dodo      // begins with package math
```

For a directory dependency, use `math/` instead of `math.dodo`, as in the complete
example below. Having both at the same import path is ambiguous and is rejected.

## Visibility and methods

Declarations and fields are private unless `pub`; public functions cannot expose
private types. Public enum variants are available with the enum. Struct methods
are statically dispatched; associated functions use `Type.name(...)`.
Public methods of a private struct may mention their own struct type, allowing
the struct to implement generic protocols without publishing its name. Other
private types remain prohibited in public signatures.

```dodo
package counter

pub struct Counter {
    value: u32

    pub fn new(start: u32) -> Self {
        Counter { value: start }
    }

    pub fn current(&self) -> u32 {
        self.value
    }
}
```

A caller importing `counter` can construct `counter.Counter.new(0)` and call
`current()`, but cannot read or write its private `value` field directly. A field
needs its own `pub` if callers should access it. Names beginning with capital
letters are not automatically public.

### Object exports and reachability

An import makes public declarations available to the source checker; it does
not export every imported function from the compiled object. The compiler keeps
root-package public functions and methods, root `main`, and all `extern "C"`
definitions as exports. Other functions, including generic specializations, are
removed when unreachable, at every optimization level. Reachability includes
implicit destructors and callback addresses. ELF and COFF output place functions
in separate code sections so a linker can discard unused exported code as well.

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

### Resolve a name collision with aliases

```dodo test
package main

import "core/bytes" as raw
import "std/bytes" as binary

fn main() {
    core.assert_eq(raw.compare(b"a", b"a"), 0i32)
}
```

Both imports normally use the name `bytes`; explicit aliases give them distinct
local qualifiers. You can use `raw` and `binary` only in this package. The
dependencies keep their original declarations and their own import scopes.

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

On Windows, the compiled executable has an `.exe` suffix; run it as
`./build/app.exe` from PowerShell. `dodo run` selects and launches the executable
for your host automatically.

## Initialization and global state

Imports load declarations, not executable initialization code. A function named
`init` runs only if your program calls it. Package constants and statics require
compile-time initializers, so the order of imports does not define a runtime
initialization sequence. Constants can refer forward to other constants;
dependency cycles are diagnosed.

Prefer constructing state in `main` and passing it to functions. When static
storage is needed, see [static storage](implementation-syntax.md#static-storage)
for initialization, unsafe mutable access, and program-exit behavior.

## Troubleshoot package errors

| Diagnostic or symptom | Check |
| --- | --- |
| Missing `main.dodo` | Run from the project folder, or pass the desired file explicitly. |
| Helper declaration is missing | Entry-file siblings are not loaded automatically; move helpers to an imported package. |
| Import cannot resolve | Resolve the path from the importing package's directory, and check its final component and package declaration. |
| Both file and directory match | Keep either `name.dodo` or `name/` for that import. |
| Conflicting local package name | Add `as alias` to distinguish imports with the same last component. |
| Declaration or field is private | Mark the intended API `pub`; import aliases do not bypass visibility. |
| A dependency's dependency is unavailable | Import the package directly where you use it; imports are not re-exports. |
| Import cycle | Move the shared declarations into a lower-level package both callers can import. |

Use `dodo check main.dodo` to verify the complete reachable project before
compiling. The compiler loads only imported packages and their dependencies,
including bundled packages; an unused folder does not participate in checking.
