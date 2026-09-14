---
title: "Test your code"
description: "Discover and run native Dodo tests, write assertions, isolate traps, and execute documentation examples."
section: "Using Dodo"
order: 105
---

Run `dodo test` in your project folder. It finds tests recursively, compiles
them, and reports each result. No manifest, test dependency, or `main` function
is needed. Like `dodo run`, it needs a C toolchain for linking.

## Write your first test

Save this as `arithmetic.dodo`:

```dodo test
package arithmetic

fn add(a: i32, b: i32) -> i32 {
    a + b
}

@test
fn adds_numbers() {
    assert_eq(add(20, 22), 42)
    assert(add(1, 2) > 0, "positive inputs should produce a positive sum")
}
```

Run it from the same folder:

```sh
dodo test
```

The output includes `arithmetic.dodo::adds_numbers ... ok` and a summary of
passed, failed, ignored, and filtered tests. A failing assertion reports the
source file, line, column, expression, and comparison values.

## Choose where tests live

Any `.dodo` file can contain `@test` functions. Functions named `test_...` are
also discovered without an attribute. Tests must be safe, top-level functions
with no generic parameters, no arguments, and a `void` return type. Use an
`unsafe` block inside a test when the operation needs one.

An inline test loads its source file and imports, just like an ordinary source
file passed to `dodo run`. It can call private helpers in that file. For tests
in a separate companion file, name it `arithmetic_test.dodo` or
`arithmetic.test.dodo`. Companion tests load all immediate `.dodo` files in
their folder as one package, so they can call private helpers in adjacent
files. All those files must declare the same package. The test runner does
not call the application's `main` function.

For a separate integration suite, use ordinary imports to exercise public
package APIs. Import paths resolve relative to the test source, following the
same rules as [other Dodo programs](packages.md).

Directory discovery skips hidden files and folders, `build`, `target`, `dist`,
`node_modules`, `vendor`, and symlinks. Ordinary application entry points and
unmarked Markdown snippets are not executed. Each test is identified by its
source path and function name, and imported tests are not executed a second
time through their importer. Files and tests have deterministic order: paths
are sorted, and functions follow source order.

## Assert behavior

Assertions are available without imports:

| Call | Succeeds when |
| --- | --- |
| `assert(condition)` | The Boolean condition is true. |
| `assert_eq(left, right)` | The two values are equal. |
| `assert_ne(left, right)` | The two values are different. |

Each call accepts an optional final `&str` message. Arguments are evaluated
once, from left to right, including the message. Equality assertions accept
matching numeric types, Booleans, raw pointers, fieldless enums, and `&str`.
Strings compare their bytes, including embedded NULs. Numbers use the language's
normal equality rules; use an explicit tolerance when testing approximate
floating-point calculations. Compare array elements or structure fields
explicitly.

```dodo test
package assertions

fn test_values() {
    assert_eq(255u8, 255, "the literal uses the other operand's type")
    assert_ne("ready", "waiting")
    assert_eq("héllo", "héllo")
}
```

These assertions also work in ordinary programs, where failure traps without
adding a hosted test runtime. Checks remain enabled at every optimization
level. Use `core.assert`, `core.assert_eq`, or `core.assert_ne` when a local
function has the same name. Test functions remain ordinary type-checked Dodo
functions during `check` and `compile`; attributes do not hide invalid code.

## Find and select tests

```sh
dodo test --list
dodo test arithmetic.dodo
dodo test path/to/project
dodo test --filter adds
dodo test --exact --filter arithmetic.dodo::adds_numbers
dodo test --skip slow
dodo test -O 3
```

`--filter` matches a substring of the path or function name. Repeat it to match
any of several filters. `--exact` matches the whole function name or the full
`path::name` identifier printed by `--list`. Repeat `--skip` to exclude several
substrings. `--list` parses and lists tests without resolving imports, checking
their bodies, or invoking a linker.

An empty suite or a filter matching no tests exits with status 1 and a discovery
hint. Use `--allow-empty` when an empty suite is expected. This keeps misspelled
filters from silently passing in CI.

## Ignore a test with a reason

```dodo test
package devices

@test
@ignore("requires a connected device")
fn device_round_trip() {
    assert(true)
}
```

Ignored tests are listed with their reason and skipped by default. Run only
ignored tests with `--ignored`, or all selected tests with `--include-ignored`.
Ignored functions in a compiled source still need to type-check.

## Understand failures

Each test runs in a fresh native process. An assertion failure, checked runtime
trap, nonzero exit, or signal fails that test; remaining tests continue.
Compiler-generated traps report the checked expression's location, including
inside imported helpers. A raw native crash reports the process status and test
declaration location. Tests do not unwind or run destructors after a trap.

Standard output and standard error are captured separately and shown for failed
tests. Add `--show-output` to see successful tests' output too. Reports show up
to 64 KiB per stream, with a truncation notice for longer output. A test has
30 seconds to execute by default; compilation time is separate.

```sh
dodo test --show-output
dodo test --timeout 5
dodo test --timeout 0
dodo test --fail-fast
```

`--timeout 0` disables the deadline. Tests run sequentially with the caller's
working directory and environment, and with standard input closed. Relative
fixture paths therefore resolve from the folder where you invoke the command.
Process isolation resets memory between tests; files and other external state
are shared. On Unix, the runner also terminates each test's process group when
the case ends. Temporary executables and captured output are removed afterwards.

The final exit status is 0 when the selected suite succeeds, and 1 for test
failures, discovery errors, build errors, or an empty selection. `--linker`,
`DODO_CC`, and repeatable `--link-arg` options work as with `compile`. Tests run
on the host platform.

## Execute documentation examples

Mark complete, runnable Markdown examples with `dodo test` on the opening
fence. They participate in normal discovery; `--doc` selects documentation
only, and `--no-doc` selects source tests only.

````markdown
```dodo test
package example

fn main() {
    assert_eq(6 * 7, 42)
}
```
````

Each marked fence is an independent Dodo source file, with its own `package`
declaration and imports. It needs `fn main()`, `fn main() -> i32`, or test
functions. A documentation `main` passes when it returns normally with status
0. Test functions within a fence run independently. Imports resolve beside the
Markdown file, and failures point to the original document's line and column.
Backtick and tilde fences are supported in `.md` and `.mdx` files.

Keep incomplete, illustrative, or environment-dependent snippets marked simply
`dodo`. Only explicitly marked examples execute. The examples in this guide
are executable and checked in compiler CI.

```sh
dodo test docs --doc
```

## Test this repository

From a source checkout, build the compiler and run the same command application
authors use:

```sh
cargo build --locked
target/debug/dodo test
target/debug/dodo test -O 3
target/debug/dodo test examples
target/debug/dodo test tests/stdlib --filter math
```

The native suite includes language example boundaries, allocation and ownership,
collections, byte/text processing, formatting, memory I/O, hashes, numerical
reference vectors, fake clocks, and in-memory network/HTTP/web protocols.
Existing self-contained fixtures expose test entry points; larger core, math,
and time fixtures provide separate tests for each independent group. Generated
MPFR vectors have one test per mathematical function, and their generator keeps
those entry points in sync.

The Rust suite remains responsible for compiler rejection tests, cross-target
emission, and integration fixtures needing external peers or platform setup.
Run it with `cargo test --locked --all-targets`. CI runs both suites, including
the native suite at optimization levels 0 and 3.
