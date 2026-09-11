# Dodo compiler 0.1.0: decisions and limits

This document describes the implemented compiler. It supplements the cleaned
[0.1 language design](language-spec-0.1.md), whose open questions remain open.
The specification includes the implemented September 2026 ergonomics revision;
implementation-specific limits remain documented here.

## Lexing and expressions

Source is UTF-8. Identifiers are case-sensitive ASCII letters/underscores followed
by ASCII letters/digits/underscores. Comments use `//`. Newlines terminate
statements; semicolons also separate statements and three-part loop clauses.
Delimited expressions and expressions continued after an operator accept
newlines. A newline also continues an expression when the next non-newline token
is `.`; indentation, blank lines, and line comments do not change this rule.
An explicit semicolon ends the expression. Other leading operators do not gain
continuation outside delimited expressions. Braces delimit control-flow bodies;
match arms also accept expressions.

Integer literals support decimal, `0x`, `0o`, `0b`, digit separators, and type
suffixes. Unsuffixed integer literals use contextual types and otherwise default
to `isize`. Floating literals use decimal fractions/exponents and optional
`f32`/`f64` suffixes; their default is `f64`. Boolean literals are `true` and
`false`. Strings and byte strings have immutable program-lifetime storage.
Strings support conventional escapes; `&str.len` counts UTF-8 bytes. String indexing is rejected; use a byte slice
for byte indexing. There is no implicit string
allocation or mandatory NUL terminator.

From lowest to highest, binary precedence is `||`, `&&`, `|`, `^`, `&`, equality
(`==`, `!=`), ordering (`<`, `<=`, `>`, `>=`), shifts (`<<`, `>>`), addition and
subtraction, multiplication/division/remainder, then `as`. Prefix operators
bind more tightly, followed by calls, fields, indices, and propagation `?`.
Binary operators associate left. Operands, call arguments, and literal fields
are evaluated left to right; `&&` and `||` short-circuit. Compound assignment
evaluates the destination address once, then its previous value, then the right
operand. Numeric operands must have compatible types; there are no implicit
mixed-width conversions between already typed values.

Canonical bindings, fields, constants, and parameters use `name: Type`. The earlier
type-first forms remain supported for locals, fields, constants, and enum
payloads. An omitted function return type means `void`. Struct receivers accept
`self`, `&self`, and `&mut self`; field literals accept same-name shorthand.
Non-void functions return their final expression unless it ends with a semicolon;
final conditionals, matches, and blocks follow the same rule. Explicit `return`
provides early exits. Void functions retain statement semantics.

`let name = value` and `let name: Type = value` create immutable runtime bindings.
`:=` and ordinary typed locals remain mutable; `const` remains compile-time-only.
Immutability prevents reassignment, mutable borrowing of owned storage, and writes
to owned fields/elements. It does not weaken a stored `&mut T` or mutable slice:
writes through those references remain permitted, including field/index writes.

Explicit generic calls canonically use `f::<T>()`; the earlier `f<T>()` spelling
remains accepted. Types and generic declarations use angle brackets without `::`.

`dodo fmt [FILE|DIRECTORY]` migrates historical syntax and applies four-space
indentation and consistent spacing while preserving comments and literal text.
Directory formatting recurses through `.dodo` files, excluding hidden directories,
`target`, `build`, and symlinks. The default input is the current directory.
`--check` exits 1 for formatting differences and writes nothing; `--stdout`
previews one file. Input `-` reads stdin and writes stdout (or checks with
`--check`). Formatting parses every input before replacing any file, requires no
imports or type checking, and stages replacements for atomic per-file renames.
An unchanged file is not rewritten. Syntax errors leave all inputs untouched.
Legacy forms remain supported in 0.1; removal or warnings belong to a future
announced deprecation after this migration path is available.

Array lists infer their length and element type from context or their elements.
Bracket arrays are canonical. A binding annotation supplies explicit types,
as in `values: [2]u16 = [1, 2]`. A list in any expression can retain its type and
length with `([1, 2]: [2]u16)` or `([]: [0]u8)`. The annotation applies only to
an unannotated bracket list and lowers to the same typed literal as the legacy
`[2]u16{1, 2}`. The formatter migrates legacy typed literals to that annotated
bracket form, including nested and constant arrays. This preserves element types
and length checks without introducing copying or runtime conversions.
Repeated arrays evaluate their initializer once, even for a zero length, and
require a copyable element. Constant initializers support checked scalar
operators/conversions, strings, arrays, repeated arrays, and struct literals.
They may reference other constants. Package constants permit forward references;
local constants follow lexical scope. Cycles and mutable-static dependencies are
rejected. Array lengths accept integer constant expressions and use the selected
target width. Compile-time function calls are not implemented.

Syntax nesting is bounded to 64 parser levels. Constant dependency depth is
bounded to 128; expansion work is bounded to 200,000 nodes. Array lengths must
fit the target's usize and LLVM's 32-bit element-count limit. Nonzero repeated
constant arrays are limited to 1,000,000 elements; zero initialization has a
compact representation. These limits produce diagnostics.

`if`, `match`, `unsafe`, and plain blocks produce values in expression positions.
Every continuing path must yield the same type; at least one value-producing
path is required. Results are transferred before local cleanup. Explicit returns,
propagation, break, and continue retain their surrounding control-flow meaning.
The checker rejects local-storage borrows escaping a value block and preserves
loans from earlier arguments while checking nested blocks.

`for i in start..end` captures both integer bounds once and excludes end. Empty
and reversed ranges do not iterate. Loop bindings are fresh, and assigning them
does not change the counter. Ranges have no general value or iterator protocol.
Collection loops still bind shared `&T` elements or exclusive `&mut T` elements.
`for &value in values` and `for index, &value in values` explicitly copy a shared
element into a fresh `T` binding. `T` must satisfy the ordinary copyability rule;
owned arrays, structs, enums, and mutable references/slices are rejected. Assigning
the copied binding cannot mutate the source. The collection is evaluated once and
stays borrowed for the loop, and copied references retain their borrow dependencies.
The index remains `usize`. Reference patterns on integer ranges, on the index, or
on mutable iteration are rejected; use a shared reborrow to copy from a mutable
collection view. `&mut value` binding patterns are not supported.

Subslices use `&data[start..end]` or `&mut data[start..end]`; bare slicing produces
a shared view. Omitted bounds default to zero and length. Source and bounds are
evaluated once and checked in every optimization profile. The captured source
is reserved while bounds run: they may read it, but cannot move or mutate it.
Exclusive access is established after the bounds. Indexed loans remain
conservative: separate ranges do not prove mutable slices disjoint.

Signed integers use two's complement. `isize` and `usize` follow the selected
target's pointer width. Integer arithmetic and left shifts trap if the result
cannot be represented. Both division and remainder trap for a signed minimum
value and divisor `-1`, as well as for zero divisors. Shift counts must be
nonnegative and smaller than the value's bit width. Integer conversions check
range; they do not truncate or wrap. Float-to-integer conversion truncates toward
zero after checking the source lies within the target range and is finite.
Integer-to-float conversion rounds to the target floating representation.
Narrowing floats checks finite range. Floating arithmetic follows LLVM's IEEE
operations without fast-math flags. There are no named wrapping operations yet.

## Packages and visibility

A file input compiles that file and its imports. A directory input combines its
immediate `.dodo` files, in sorted order, into one package. All files in that
unit must declare the same package. Subdirectories are not implicitly included.

`import "math"` resolves relative to the importing package directory to either
`math.dodo` or a `math/` directory containing `.dodo` files. Nested import paths
are supported; the imported package declaration must match the final path
component. An ambiguous file-and-directory match, cycle, duplicate import alias,
or conflicting package mapping is diagnosed. Dependencies are local files;
there is no registry, network resolver, alias syntax, or re-export mechanism.
Only directly imported package names are available in a package.

Declarations and fields are private unless `pub`; public functions cannot expose
private types. Public enum variants are available with the enum. Struct methods
are statically dispatched; associated functions use `Type.name(...)`.

`core/mmio`, `core/ptr`, and `core/mem` are compiler-provided imports.
`core.drop(value)` destroys an owned value early. Intrinsics are ordinary checked
calls with compiler lowering, not user-definable macros.

## Ownership and generics

Scalars, shared references/slices, strings, and raw pointers copy. Structs,
arrays, enums, `Result`, `Option`, and mutable references/slices move. Mutable
reference arguments are reborrowed as appropriate. Non-copy field/index partial
moves are rejected; borrow or transfer the whole aggregate instead.

The checker follows source-place dependencies through references and aggregates,
permits separate field loans, and conservatively overlaps all indices into the
same collection. Loans can end at last use. Loops, control-flow joins, and custom
destruction extend liveness conservatively, so some valid programs may be
rejected. Borrowed returns are checked against inferred or explicit `from(...)`
sources; contracts do not extend the lifetime of local storage. Self-referential
owning values and independently varying borrowed-field lifetimes are unsupported.

Ownership errors retain source spans for the conflicting access, the originating
borrow, and the use keeping it live. Moves retain their transfer location, and
borrowed-return errors label the declared contract and the returned source.
The renderer resolves each label to its own file and shows labeled snippets.
`dodo lsp` exposes inferred types, receiver ownership, and inferred or explicit
borrowed-return sources through hover, and publishes errors with related source
locations. See [diagnostics and editors](diagnostics-and-editors.md) for examples
and the supported editor protocol.

A `Result` must be forwarded, propagated, or matched with explicit `ok` and `err`
arms. Binding it and leaving scope, overwriting it unhandled, assigning it to
`_`, or passing it to `core.drop` is rejected.

Patterns compose recursively across enum payloads and structs. They include
bindings, wildcards, Boolean/integer literals, literal integer ranges (`..` and
`..=`), alternatives (`|`), and optional Boolean match guards. Struct patterns
list every field or use `..` for the remainder. Alternatives must introduce the
same bindings with compatible types and borrow modes. Pattern tests and guards
run in source order, retrying alternatives after a false guard. Ownership
transfers and destruction occur only after the guard succeeds. The current
checker conservatively rejects moves of non-copy values, assignment, and mutable
borrows anywhere in a guard, including operations on unrelated local storage.
Shared borrows and observer calls are permitted. Guarded arms do not count toward
coverage. Existing match-arm bindings remain mutable locals; names introduced by
`let` and `if let` are immutable bindings with the reference permissions above.
Exhaustiveness checks preserve correlations between nested fields and partition
integer ranges by their endpoints. Each pattern may expand to at most 4,096
alternatives; coverage checking is limited to 131,072 work units. Excessive
patterns produce a diagnostic.

`if let` scopes bindings to the success block. Destructuring `let` introduces
immutable bindings in the surrounding block; refutable patterns require an
`else` that diverges. Both forms consume owned scrutinees on success or failure
and borrow reference scrutinees. Conditional patterns must cover every state
whose active payload contains a Result, including borrowed values. For example,
`some(result)` may match `Option<Result<T, E>>` if the bound result is handled;
its unmatched `none` path has no obligation. Success-only `ok` patterns and
ignored nested Results are rejected. Borrowed patterns preserve shared/mutable
permissions recursively, including separate loans for disjoint struct fields;
owned patterns cannot destructure structs with custom `drop`.

See [patterns.dodo](../examples/patterns.dodo) for conditional bindings and
[hex.dodo](../examples/hex.dodo) for range matching with Result propagation.

Generics are monomorphized. Function calls and struct literals infer omitted
type arguments from arguments, fields, and expected result types. Examples are
`identity(42i32)`, `value: i32 = identity(42)`, and `Box{value: 42i32}`. `some(value)`
infers its Option payload. Ambiguous calls require explicit arguments, such as
`make::<i32>()`; explicit forms remain accepted. Inference follows local expression
context, not later uses of a binding. Function signatures remain declared.
Constraints/traits, specialization, and separately compiled generic interfaces
are not implemented. Recursive expansion is bounded and diagnosed.

Owned local values are destroyed in reverse declaration order on normal exits.
Custom `drop` runs before fields are destroyed, and fields/array elements are
destroyed in reverse order. A live flag prevents repeated destruction after
moves along different control-flow paths. Temporary owned places are retained
in the enclosing block until cleanup. Traps abort and do not run cleanup.

## Layout, entry points, and foreign calls

Struct fields keep declaration order and LLVM target ABI alignment/padding.
`@repr(C)` uses this C-compatible field layout. Native Dodo function ABI is
compiler-internal and unstable. Primitive and raw-pointer `extern "C"`
parameters/results use LLVM's C calling convention; C aggregate-by-value ABI
classification and variadics are not supported. Pass pointers to `@repr(C)`
structs instead. Imported foreign declarations use the declared final name as
the external symbol; Dodo functions use `dodo.<root-package>.<qualified-name>`.

Enum storage is an explicit tag plus storage for each variant's payload. Result
storage is an error tag plus success/error slots; Option storage is a presence
tag plus a payload slot. There are no niche optimizations or stable enum ABI.
`void` Result success uses an internal placeholder slot.

Hosted executables add a C `main` wrapper around the source `main`. Object,
assembly, bitcode, and IR emission do not add startup code. A native C linker
driver links executables; the driver is executed directly with argument vectors.
Custom targets require caller-supplied startup and linker configuration.

## Implemented core calls

| Call | Return | Requirements |
| --- | --- | --- |
| `core.drop(value)` | `void` | Consumes a value; cannot discard a Result. |
| `mem.size_of::<T>()` | `usize` | Selected-target ABI size; `void` is zero. |
| `mem.align_of::<T>()` | `usize` | Selected-target ABI alignment; `void` is one. |
| `mmio.read8/16/32/64(address)` | Corresponding unsigned integer | Unsafe; `address: usize`. |
| `mmio.write8/16/32/64(address, value)` | `void` | Unsafe; width-matched value. |
| `ptr.read(pointer)` | Pointee value | Unsafe; raw pointer. |
| `ptr.write(pointer, value)` | `void` | Unsafe; mutable raw pointer; overwrites without dropping. |
| `ptr.read_unaligned(pointer)` | Pointee value | Unsafe; load alignment one. |
| `ptr.write_unaligned(pointer, value)` | `void` | Unsafe; store alignment one. |
| `ptr.read_volatile(pointer)` | Pointee value | Unsafe; volatile load. |
| `ptr.write_volatile(pointer, value)` | `void` | Unsafe; volatile store. |
| `ptr.offset(pointer, offset)` | Same pointer type | Unsafe; `offset: isize`, in elements. |

Aliases above require their matching `core/...` import. Raw memory operations
remain subject to the specification's validity and aliasing obligations.
Reading/writing checked-borrow-carrying values through raw pointer intrinsics and
raw-pointer-to-checked-reference casts are rejected in this release. Unsafe
functions still need explicit unsafe blocks for unchecked operations.

MMIO uses LLVM volatile loads/stores directly, without fabricating checked
references. The compiler rejects widths above the target pointer width; actual
device access legality remains a target/board obligation. Volatile operations
are not atomics or CPU barriers.

## Remaining design surface

The broad design's `MaybeUninit`, `offset_of`, checked slice splitting, allocator
interfaces, atomics, DMA/interrupt-safe abstractions, target barriers, inline
assembly, section/alignment/export/interrupt attributes, and custom panic-handler
integration are not implemented. Unsupported syntax and unknown intrinsics
produce diagnostics. The trap implementation is `llvm.trap`; it is not a
platform reset driver.

This compiler provides a usable hosted core and target object emission. It is an
initial implementation, with a conservative borrow checker and explicit platform
limits, not the entire embedded ecosystem described by the design. Tests cover
many accepted/rejected cases and native behavior; they are not a soundness proof.

LLVM lowering follows the [LLVM Language Reference](https://llvm.org/docs/LangRef.html),
including explicit overflow intrinsics, volatile instructions, and x86 SysV
C-ABI integer extension attributes on declarations and call sites.
