---
title: "Diagnostics and common mistakes"
description: "Read compiler errors, fix beginner mistakes, and follow borrow, move, and returned-reference diagnostics."
section: "Using Dodo"
order: 110
---

`dodo check` shows the code responsible for ownership errors. A primary label
uses `^` for the rejected operation; related labels use `-` to explain where the
borrow or move began and which use still needs it. Every snippet includes its
source filename and line/column. Labels from imported files retain their own
locations.

## Read an error in order

1. Read the first diagnostic message before changing several unrelated lines.
2. Open the file and line shown by its primary label. Imported code can have a
   different filename from your entry file.
3. Read the related labels: they often identify the earlier declaration, move,
   or borrow that explains the conflict.
4. Make one change and run `dodo check` again. Later errors may disappear once
   the first error is fixed.

An editor can show the same diagnostics as you type. A successful `check`
verifies the language rules; it does not link the program or prove that runtime
inputs will never overflow, exceed bounds, or trigger a library error.

## Common beginner mistakes

| Problem | Why it happens | How to proceed |
| --- | --- | --- |
| Assigning to a `let` binding | `let` is immutable. | Use `:=` or a typed mutable binding if reassignment is intended. |
| Ignoring a fallible call | A `T!E` Result must be handled. | Use `match`, propagate with `?`, or deliberately unwrap with `!`. |
| Using `?` in ordinary `main` | Propagation needs a compatible Result-returning function. | Put fallible work in a helper and match its Result in `main`. |
| Adding a loop element to a number | Ordinary collection iteration yields references. | Dereference the element or use `for &value in values` for copyable values. |
| Returning a reference to a local array | Its storage disappears at function exit. | Return an owned value or borrow storage supplied by the caller. |
| An ambiguous generic or empty value | Inference has no concrete type to choose. | Add a binding type or an explicit generic argument at the call. |
| A missing method or package | The import, alias, or compiler version differs. | Check the [API signature](stdlib-api.md), import path, and installed compiler. |
| `check` works but `run` fails to link | Linking is a separate stage. | Configure the [C toolchain](installation.md); inspect missing dependencies. |

See [bindings](language-basics.md), [Results](patterns-and-results.md),
[generics](generics.md), and [ownership](ownership.md) for complete working
examples of these rules.

## Borrow conflicts

For example, run this intentionally invalid program:

```sh
dodo check examples/diagnostics/borrow_conflict.dodo
```

The diagnostic connects these three expressions:

```text
view := &value
        ------ shared borrow begins here

value = 2
^^^^^ cannot modify `value` while this borrow is live

return consume(view)
               ---- borrow is used here
```

Move the modification after the last use of `view`, or move that use before the
modification. For struct fields, borrowing independent fields can also avoid a
conflict. A loop can keep a borrow live into its next iteration, and a custom
destructor can need borrowed data at scope exit; diagnostics explain those
reasons when there is no later ordinary use.

## Moves and returned borrows

Moves and borrowed returns have related labels too:

```sh
dodo check examples/diagnostics/moved_value.dodo
dodo check examples/diagnostics/return_source.dodo
```

The first command connects the ownership transfer to the rejected second use.
The second connects the returned borrow to its parameter and the `from(a)`
contract that excludes it. Return an allowed source, or change the contract to
describe the function's intended inputs. `from(static)` cannot make a reference
to local storage survive its destruction.

## Try a working borrowing example

These three files are rejection examples: each command should fail. The working
[borrowing example](https://github.com/Jotrorox/dodo/blob/main/examples/borrowing.dodo) demonstrates shared, mutable, and
consuming receivers, inferred and explicit return sources, and ending a borrow
before mutation:

```sh
dodo check examples/borrowing.dodo
dodo run examples/borrowing.dodo
```


## Editors

Set up [editor diagnostics and hovers](editors.md) to see these locations while
you write code. Hovers also expose inferred types, receiver behavior, and
borrowed-return contracts.

## Compiler library consumers

Library consumers can inspect `Diagnostic.labels` directly instead of parsing
terminal output. Spans are byte offsets into the source passed to the parser;
package-loaded spans use the offsets in `Loaded.sources`. Render package errors
with `Loaded::render` so each label resolves to the correct file.

Diagnostics used by quick fixes also expose a `Diagnostic.kind`. Its typed
payload identifies the affected local binding (name, declaration span, and use
span) or the unresolved symbol name. Use this metadata instead of matching
English messages or extracting names from backticks. Indirect storage has no
direct binding payload, and complex unresolved receivers have no name payload.

`DiagnosticKind::code()` returns stable codes, also published in LSP diagnostics:
`immutable-assignment`, `immutable-borrow`, `unknown-function`, `unknown-binding`,
`unknown-type`, `unknown-struct`, `unknown-variant`, and `unresolved-receiver`.
Other diagnostics remain `Unclassified` and have no code.
