# Dodo compiler 0.1.0: decisions and limits

This document describes the implemented compiler. It supplements the cleaned
[0.1 language design](language-spec-0.1.md), whose open questions remain open.
Implementation choices below are not silent changes to that design.

## Lexing and expressions

Source is UTF-8. Identifiers are case-sensitive ASCII letters/underscores followed
by ASCII letters/digits/underscores. Comments use `//`. Newlines terminate
statements; semicolons also separate statements and three-part loop clauses.
Delimited expressions and expressions continued after an operator accept
newlines. Braces delimit every control-flow body.

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

Constant initializers support literal expressions, checked scalar operators and
conversions, strings, arrays, and struct literals. References to other named
constants inside a constant initializer and compile-time function calls are not
implemented; constants may be used normally in runtime expressions.

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

A `Result` must be forwarded, propagated, or matched with explicit `ok` and `err`
arms. Binding it and leaving scope, overwriting it unhandled, assigning it to
`_`, or passing it to `core.drop` is rejected.

Generics are monomorphized. Use explicit type arguments, for example
`identity<i32>(42)` and `Box<i32>{value: 42}`. Generic argument inference,
constraints/traits, specialization, and separately compiled generic interfaces
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
| `mem.size_of<T>()` | `usize` | Selected-target ABI size; `void` is zero. |
| `mem.align_of<T>()` | `usize` | Selected-target ABI alignment; `void` is one. |
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
