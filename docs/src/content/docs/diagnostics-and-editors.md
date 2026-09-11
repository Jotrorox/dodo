---
title: "Ownership diagnostics"
description: "Read borrow conflicts, move errors, and borrowed-return labels from dodo check."
section: "Using Dodo"
order: 110
---

`dodo check` shows the code responsible for ownership errors. A primary label
uses `^` for the rejected operation; related labels use `-` to explain where the
borrow or move began and which use still needs it. Every snippet includes its
source filename and line/column. Labels from imported files retain their own
locations.

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
