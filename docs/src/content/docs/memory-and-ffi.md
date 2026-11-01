---
title: "Memory and foreign calls"
description: "Implemented layouts, C interoperation, pointer intrinsics, and MMIO limits in Dodo 0.1.6."
section: "Language reference"
order: 230
---

Use this reference when writing a C interface, hardware driver, allocator, or
other low-level library. Ordinary application code can usually stay with checked
references, [slices](language-basics.md#slices-borrow-a-region), and the
[standard library](standard-library.md).

The compiler checks types and ordinary borrowing inside unsafe code, but cannot
prove that a raw address identifies live initialized memory. An `unsafe` block
marks the place where your code supplies that guarantee. An `unsafe fn` makes
its caller responsible for documented preconditions; its body still needs
explicit unsafe blocks for unchecked operations.

The normative allocation, cast, and aliasing contracts are in
[specification section 14](language-spec-0.1.md#14-raw-memory-validity); layout and
ABI rules are in [section 16](language-spec-0.1.md#16-foreign-interfaces-layout-and-target-attributes).
See the [compiler overview](implementation.md#remaining-design-surface)
for planned facilities that are not yet available.

## Start with an owned value

```dodo test
package main

import "core/ptr"

fn main() {
    value := 40i32
    pointer := ptr.from_mut(&mut value)
    unsafe {
        // value is live, initialized, aligned, and not borrowed elsewhere.
        previous := ptr.read(pointer)
        ptr.write(pointer, previous + 2)
    }
    core.assert_eq(value, 42i32)
}
```

Obtaining the pointer is safe. Reading or writing it requires `unsafe`.
The pointer does not keep `value` alive or extend a checked borrow. For an owned
non-copy payload, `ptr.read` transfers the value without clearing its old bytes;
the caller must prevent a second read or destruction from creating two owners.
This example uses an integer to avoid that additional ownership obligation.

Use checked references whenever they express the operation. A raw pointer is
useful for a foreign boundary or storage implementation, not a way to avoid a
borrow-checking error.

## Struct layout and the C ABI

Struct fields keep declaration order and LLVM target ABI alignment/padding.
`@repr(C)` uses this C-compatible field layout. Native Dodo function ABI is
compiler-internal and unstable. Primitive, raw-pointer and C function pointer
`extern "C"` parameters/results use LLVM's C calling convention. Variadics are
not supported. Imported foreign declarations use the declared final name as
the external symbol; C definitions do the same. Native Dodo functions use
`dodo.<root-package>.<qualified-name>`.

A `@repr(C)` struct can also pass and return by value, as C passes the struct
with the same fields:

```dodo
@repr(C)
struct Size {
    width: u64
    height: u64
    depth: u64
}

unsafe extern "C" fn dispatch(threadgroups: Size, threads: Size)
```

Its fields must be integers, floats, bool, raw pointers, `extern "C" fn`
pointers, other such structs, or nonempty arrays of these, and the struct must have at least one field and no
destructor. The compiler classifies these structs for x86-64 Linux and macOS
(System V), Windows x64, and AArch64 (Linux, macOS and Windows). Other targets,
such as 32-bit Arm, RISC-V and WebAssembly, reject a struct by value; pass a
pointer to it there. Arrays, enums, Options and Results never cross the C ABI
by value. Dodo `extern "C" fn` definitions with a body accept and return C
structs the same way, so C code can call them, and calls through
`extern "C" fn` pointers pass them as C does.

Declare a foreign function without a body, then call it in an unsafe block:

```dodo
package main

unsafe extern "C" fn abs(value: i32) -> i32

fn main() {
    unsafe {
        // C abs is defined for this input; the signed minimum is excluded.
        core.assert_eq(abs(-42), 42i32)
    }
}
```

This example requires a hosted C runtime that supplies `abs`. Dodo declarations
must match the foreign library's actual argument widths, signedness, ABI, and
symbol names. An external declaration does not automatically link its library;
use the [linker options](command-line.md) when extra libraries or objects are
needed. Dodo can also define a function with a body using `extern "C" fn`.

`@link_name("symbol")` binds a foreign declaration to a symbol other than its
own name. Several declarations may bind one symbol with different signatures,
which is how Objective-C calls `objc_msgSend`: each message uses the signature
of its method.

```dodo
@link_name("objc_msgSend")
unsafe extern "C" fn send_id(receiver: *mut u8, selector: *mut u8) -> *mut u8
@link_name("objc_msgSend")
unsafe extern "C" fn send_bool(receiver: *mut u8, selector: *mut u8, flag: bool)
```

Each call passes its arguments by its own declaration's signature, and the
binding author must make sure the callee expects exactly that.

### C function pointers

`extern "C" fn(A) -> R` is the address of code using the C calling convention.
An ordinary `fn(A) -> R` is not: it uses Dodo's own convention, so the two
types never convert into each other. A C function pointer comes from a C
function's name, from C code, or from an unsafe `as` conversion of a raw
pointer or address, for example one returned by `dlsym`. Calling one is unsafe,
like calling the function by name:

```dodo
package main

import "core/ptr"

unsafe extern "C" fn dlopen(path: *const u8, mode: i32) -> *mut u8
unsafe extern "C" fn dlsym(handle: *mut u8, name: *const u8) -> *const u8

fn main() {
    unsafe {
        program := dlopen(0 as *const u8, 2)
        symbol := dlsym(program, ptr.as_ptr(b"abs\0"))
        // A C function pointer is never null: check before converting.
        core.assert(symbol as usize != 0)
        abs := symbol as extern "C" fn(i32) -> i32
        core.assert_eq(abs(-42), 42i32)
    }
}
```

The converted pointer must address code with exactly the written signature.
Dodo does not check for null: a table that stores address zero for an absent
entry, as [`std/gpu/vulkan`](vulkan.md) does for commands a driver lacks, must
test `entry as usize != 0` before calling it. [`std/dylib`](dylib.md) looks
symbols up portably.
Struct fields of C function pointer type describe C method tables, and a
function defined with `extern "C" fn` passes to C as a callback by name:

```dodo
unsafe extern "C" fn qsort(base: *mut u8, count: usize, size: usize,
    compare: extern "C" fn(*const u8, *const u8) -> i32)

extern "C" fn ascending(a: *const u8, b: *const u8) -> i32 {
    unsafe { return *(a as *const i32) - *(b as *const i32) }
}
```

`&str` is not a C string. Its bytes have no guaranteed trailing NUL, and its
representation includes a length. C interfaces that expect a string usually
need an explicitly prepared byte buffer and raw pointer.

## Enum, Result, and Option layout

Enum storage is a zero-based u32 tag plus one shared payload area. Each variant's
fields retain declaration order, and the area accommodates the largest payload
and strictest alignment. Result storage is a bool error tag (false for ok, true
for err) plus shared storage for success or error; a `void` alternative needs no
payload bytes. Option storage is a bool presence tag plus a payload slot. These
rules include target ABI padding and no niche optimizations; they do not define
a C enum or union ABI. Only the active alternative owns resources. Inactive
bytes and padding are not guaranteed to be initialized. Rebuild native objects
together when migrating from the earlier separate payload slots.

## Query layout without guessing

```dodo test
package main

import "core/mem"

@repr(C)
struct Header {
    tag: u8
    length: u32
}

fn main() {
    core.assert_eq(mem.size_of::<u32>(), 4usize)
    core.assert_eq(mem.offset_of::<Header>("tag"), 0usize)
    core.assert(mem.size_of::<Header>() >= 5usize)
    core.assert(mem.align_of::<Header>() >= mem.align_of::<u32>())
}
```

`size_of`, `align_of`, and `offset_of` use the selected compilation target. Padding
means a struct's size need not equal the sum of its field sizes. `offset_of`
requires a literal name of one directly accessible field; it does not accept a
nested field path. None of these queries reads a value or allocates storage.

Do not serialize a struct by assuming that padding or inactive enum payload
bytes are initialized. Use explicit byte encoding such as [bytes](bytes.md) or
[JSON](json.md) for portable data formats.

## Entry points and linking

Hosted executables add a C `main` wrapper around the source `main`. Object,
assembly, bitcode, and IR emission do not add startup code. A native C linker
driver links executables; the driver is executed directly with argument vectors.
Custom targets require caller-supplied startup and linker configuration.

The hosted source entry is a root non-C `main` function with no
parameters, returning `void` or `i32`. `fn main()` means `void` and exits with
status zero; an `i32` return supplies the process exit status. Put fallible
startup in a helper returning a Result and handle it in `main`. Import
`std/env` to access process arguments rather than adding parameters to `main`.

## Implemented core calls

This table supplies the normative core call signatures and restrictions referenced
by the 14 September edition of the 0.1 specification. Calls with an owner argument
also obey its checked dependency rules. Other library calls are versioned in their
respective references.

| Call | Return | Requirements |
| --- | --- | --- |
| `core.drop(value)` | `void` | Consumes a value; cannot discard a Result. |
| `core.assert(condition)` | `void` | `condition: bool`; a false condition panics. |
| `core.assert_eq(left, right)`, `core.assert_ne(left, right)` | `void` | Checked comparison assertions; see [testing](testing.md). |
| `core.wrapping_add(a, b)`, `core.wrapping_sub(a, b)`, `core.wrapping_mul(a, b)` | Same integer type | Safe explicit modulo arithmetic; optional `::<T>`; not constant expressions. |
| `mem.size_of::<T>()` | `usize` | Selected-target ABI size; `void` is zero. |
| `mem.align_of::<T>()` | `usize` | Selected-target ABI alignment; `void` is one. |
| `mem.offset_of::<T>("field")` | `usize` | Direct accessible field of a struct; selected-target ABI offset. |
| `mem.replace(&mut value, replacement)` | Old value | Moves without dropping; disallows checked-borrow/Result-containing values. |
| `mem.swap(&mut a, &mut b)` | `void` | Disjoint initialized values; same type restrictions as `replace`. |
| `mem.uninit::<T>()` | `MaybeUninit<T>` | Opaque storage with T's size/alignment; never implicitly drops T. |
| `mem.init(value)` | `MaybeUninit<T>` | Moves T into opaque storage; cannot hide checked borrows or Results. |
| `mem.assume_init(storage)` | `T` | Unsafe; consumes fully initialized storage; checked-borrow T unsupported. |
| `mem.uninit_as_ptr(&storage)` | `*const T` | Obtains a raw pointer without reading or initializing T. |
| `mem.uninit_as_mut_ptr(&mut storage)` | `*mut T` | Requires an exclusive reference to the storage. |
| `mmio.read8/16/32/64(address)` | Corresponding unsigned integer | Unsafe; `address: usize`. |
| `mmio.write8/16/32/64(address, value)` | `void` | Unsafe; width-matched value. |
| `cpu.fence()` | `void` | Safe; full hardware memory barrier. |
| `cpu.disable_interrupts()` | `bool` | Safe; masks interrupts, returns whether they were enabled. |
| `cpu.restore_interrupts(enabled)` | `void` | Unsafe; re-enables only if `enabled`. |
| `cpu.wait_for_interrupt()` | `void` | Safe; sleeps until an interrupt is pending. |
| `cpu.core_id()` | `u32` | Safe; the executing core's number, 0 on a single-core target. |
| `ptr.read(pointer)` | Pointee value | Unsafe; raw pointer. |
| `ptr.write(pointer, value)` | `void` | Unsafe; mutable raw pointer; overwrites without dropping. |
| `ptr.read_unaligned(pointer)` | Pointee value | Unsafe; load alignment one. |
| `ptr.write_unaligned(pointer, value)` | `void` | Unsafe; store alignment one. |
| `ptr.read_volatile(pointer)` | Pointee value | Unsafe; volatile load. |
| `ptr.write_volatile(pointer, value)` | `void` | Unsafe; volatile store. |
| `ptr.offset(pointer, offset)` | Same pointer type | Unsafe; `offset: isize`, in elements. |
| `ptr.from_ref(&value)` / `ptr.from_mut(&mut value)` | `*const T` / `*mut T` | Safe pointer conversion; dereferencing remains unsafe. |
| `ptr.as_ptr(slice)` / `ptr.as_mut_ptr(slice)` | `*const T` / `*mut T` | Borrowed array or slice; mutable form requires exclusive access. |
| `mem.str_bytes(text)` | `&[u8]` | Safe; preserves the string's checked dependencies. |
| `mem.str_from_utf8(bytes)` | `&str` | Unsafe; bytes must be valid UTF-8, dependencies preserved. |
| `ptr.is_null(pointer)` | `bool` | Safe; does not access storage. |
| `ptr.borrow(pointer, owner)` | `&T` | Unsafe; raw `*const T` or `*mut T`, checked owner reference or slice. |
| `ptr.borrow_mut(pointer, owner)` | `&mut T` | Unsafe; mutable raw pointer and exclusive checked owner. |
| `ptr.borrow_slice(pointer, count, owner)` | `&[T]` | Unsafe; initialized range, count in elements, checked owner. |
| `ptr.borrow_slice_mut(pointer, count, owner)` | `&mut[T]` | Unsafe; uniquely accessible initialized range and exclusive owner. |
| `ptr.copy(source, destination, count)` | `void` | Unsafe; copies `count` T elements; overlapping ranges allowed. |
| `ptr.copy_nonoverlapping(source, destination, count)` | `void` | Unsafe; copied ranges must not overlap. |
| `ptr.write_bytes(destination, byte, count)` | `void` | Unsafe; fills `count * size_of<T>()` bytes with a `u8` pattern. |
| `ptr.drop_in_place(pointer)` | `void` | Unsafe; destroys one initialized T, including its fields; does not free storage. |

The following compiler calls support checked library implementations:

| Call | Return | Requirements |
| --- | --- | --- |
| `mem.storage_type::<T>()` | `void` | Rejects unsupported typed-storage element types; see [container elements](container-elements.md). |
| `ptr.store(pointer, value, &mut owner)` | `void` | Unsafe; matching typed witness, valid destination, and checked dependency deposition. |
| `ptr.take(pointer, &mut owner)` | `T` | Unsafe; removes one initialized value, transferring stored sources. |
| `ptr.relocate(source, destination, count, &mut owner)` | `void` | Unsafe; moves within one owner, allowing overlap; preserves source dependencies. |
| `ptr.view(pointer, &owner)` | `&T` | Unsafe; checked shared typed-storage view. |
| `ptr.view_slice(pointer, count, &owner)` | `&[T]` | Unsafe; checked shared typed-storage slice. |
| `mem.split_at_mut::<slice.SplitMut<T>>(data, mid)` | `slice.SplitMut<T>` | Safe narrow intrinsic; traps if `mid > data.len`; prefer `slice.split_at_mut` for an Option result. |
| `mem.assert_send::<T>()`, `mem.assert_sync::<T>()` | `void` | Compile-time thread capability check; see [threads](threads.md). |
| `mem.callback::<Types...>(function_name)` | `*const u8` | Unsafe; address of a statically selected `unsafe fn(*mut u8) -> void` specialization, for a matching native trampoline. |

Atomic raw-storage intrinsics are described [below](#atomic-storage-intrinsics).
There is no user-defined intrinsic mechanism. Each package must directly import
`core/mem`, `core/ptr`, `core/mmio`, or `core/cpu` for the corresponding calls.

## Initialize opaque storage

`MaybeUninit<T>` lets a library reserve storage without pretending it contains a
valid `T`. It has `T`'s size and alignment but does not implicitly destroy a `T`:

```dodo test
package main

import "core/mem"
import "core/ptr"

fn main() {
    storage := mem.uninit::<i32>()
    unsafe {
        // Write a complete, valid i32 before transferring it out of storage.
        ptr.write(mem.uninit_as_mut_ptr(&mut storage), 42i32)
    }
    value := unsafe { mem.assume_init(storage) }
    core.assert_eq(value, 42i32)
}
```

`mem.init(value)` creates already initialized opaque storage. `mem.assume_init`
consumes the storage and transfers its contained value, but does not check the
bytes at runtime. Calling it before complete initialization violates its unsafe
contract. Destroying a still-initialized `MaybeUninit<T>` does not destroy its
hidden payload; low-level owners must arrange exactly one transfer or explicit
destruction. These operations cannot be used to hide checked borrows or pending
Result obligations.

## Unsafe memory access

### Raw casts and pointer arithmetic

| Conversion or operation | Rule |
| --- | --- |
| `&T` to `*const T` | Safe; same pointee type, no lifetime extension. |
| `&mut T` to `*mut T` or `*const T` | Safe; later accesses must honor the original permissions. |
| Raw pointer to another raw-pointer type | Unsafe; does not validate the resulting pointee or alignment. |
| Integer to pointer, or pointer to integer | Unsafe; an address alone does not prove usable storage. |
| Raw pointer to checked reference | Rejected; use an owner-bound `ptr.borrow*` primitive. |
| `ptr.offset(pointer, offset)` | Unsafe; `offset: isize` counts elements, not bytes. |

On the supported integral-address profile, integer/pointer casts preserve low
bits, discard excess high bits, and zero-extend when widening. These are different
from checked numeric casts. Zero produces a null pointer, and `ptr.is_null`
checks it without dereferencing. Pointer equality compares addresses, not proof
of common allocation identity.

For allocation-derived pointers, an offset must stay within the same allocation
or one past its end, its signed byte displacement must fit `isize`, and address
calculation must not wrap. A one-past pointer cannot be used for a nonempty read
or write. Moving, freeing, or reallocating its source may invalidate the pointer;
reusing the same numerical address does not revive the old storage lifetime.

### Access and owner-bound views

Aliases above require their matching `core/...` import. Raw memory operations
remain subject to the specification's validity and aliasing obligations.
Reading/writing checked-borrow-carrying values through raw pointer intrinsics and
raw-pointer-to-checked-reference casts are rejected in this release. Unsafe
functions still need explicit unsafe blocks for unchecked operations.

Raw copies and byte fills operate on byte ranges with alignment one and do not
establish initialization or ownership by themselves. Their element count times
element size must fit in `usize`, and nonempty source/destination ranges must be
valid for the requested reads/writes. Zero-byte operations accept null pointers,
including nonzero counts of zero-sized elements. A bitwise copy of an owning
value must not result in two live owners being destroyed. `drop_in_place`
requires a valid aligned pointer to an initialized value, and that value must
not subsequently be dropped again without reinitialization.

Raw-pointer conversions do not extend storage lifetimes. The four explicit
unsafe `ptr.borrow*` operations create checked views tied to a supplied owner
reference or slice. The caller must establish that the owner keeps the complete
storage alive, nonnull, correctly aligned, initialized, and valid for the
returned lifetime. The byte extent must fit in `isize`, stay within one live
allocation, and respect all existing aliases. Empty and zero-sized views still
require nonnull aligned pointers. The pointer need not point inside the owner
itself, which permits a buffer or box to anchor a view into its allocation.

The pointer/owner correspondence is an unsafe invariant; the compiler cannot
establish it from their bytes. It does check that returned views retain the
owner's complete dependencies and cannot outlive, move, destroy, or mutably
alias their owner. Mutable views require a mutable raw pointer and exclusive
owner access. Borrowing a container exclusively retains its inherited shared
allocator dependencies as shared. Raw casts still cannot fabricate checked
references, and unsafe code must not invalidate an allocation while a checked
view remains live.

For `ptr.borrow*`, element types containing checked references or Results are
rejected recursively:
the owner loan cannot reconstruct hidden element provenance or Result handling
obligations. The owner's own checked fields remain tracked. Owner expressions
are evaluated after the pointer (and count for slices), even though their
address is not used by the generated view. Safe container methods establish the
unsafe invariant privately and return ordinary checked views.

These are two independent restrictions. A stored reference needs its referent
dependencies transferred on insertion and removal; an owner loan alone only
protects storage. A stored Result needs its pending handling obligation
transferred as well, including on failed insertion and destruction. Matching a
value before replacing it does not handle the replacement. Ordinary assignment
of Result-containing values through fields, indices, or references is rejected
for the same missing handling-state transfer. See the
[container element guide](container-elements.md) for the implemented typed-storage
primitives and their checked mutation/return contracts. Existing raw pointer and
opaque storage primitives retain their restrictions.

`mem.str_from_utf8` requires the complete byte slice to be valid UTF-8. Prefer
safe `text.Text.new(bytes)?.as_str()` after binding the text view. It creates no
allocation and does not extend the source lifetime.

Raw access must respect all live checked borrows. Allocator and storage examples
are in [standard library](standard-library.md).

## MMIO and volatile access

Prefer the typed `hal.Reg<T>` handles in [hardware and embedded](hardware.md);
they lower to exactly these operations. MMIO uses LLVM volatile loads/stores
directly, without fabricating checked references. The compiler rejects widths above the target pointer width; actual
device access legality remains a target/board obligation. Volatile operations
are not atomics or CPU barriers.

LLVM lowering follows the [LLVM Language Reference](https://llvm.org/docs/LangRef.html),
including explicit overflow intrinsics, volatile instructions, and x86 SysV
C-ABI integer extension attributes on declarations and call sites.

## CPU control

`core/cpu` exposes the few processor operations that device code cannot
express with memory accesses. `std/embedded/hal` builds its `Critical` section on them.

| Target | `disable_interrupts` / `restore_interrupts` | `wait_for_interrupt` |
| --- | --- | --- |
| Bare-metal Arm Cortex-M (`thumbv6m`, `thumbv7m`, `thumbv7em`, `thumbv8m*`) | `mrs PRIMASK` + `cpsid i` / `msr PRIMASK` | `wfi` |
| Bare-metal RISC-V machine mode (`riscv32`, `riscv64`) | `csrrci mstatus, 8` / `csrs mstatus` | `wfi` |
| Hosted OS targets (Linux, Windows, macOS, BSDs, WASI) | No-op; returns `false` | No-op |
| Other freestanding targets | Rejected if the call is reachable | Rejected if reachable |

`cpu.core_id()` numbers the cores of a multi-core chip from 0, so that code
such as `std/embedded/executor` can keep separate state for each core. Hosted
targets return 0. RISC-V reads `mhartid`. A Cortex-M has no architectural
core number, so the call asks the chip runtime's `dodo_core_id` function, which
the RP2040 and RP2350 runtimes define (it reads the SIO's CPUID register), and
returns 0 when the linked runtime defines none. Other freestanding targets
reject a reachable call.

`cpu.fence()` is available on every target and lowers to a sequentially
consistent LLVM `fence` (`dmb` on Arm, `fence rw, rw` on RISC-V). Use it to order
ordinary memory writes, such as a DMA buffer, before the volatile access that
starts a device.

Restoring is unsafe because enabling interrupts inside an enclosing critical
section would end that section early; pass back exactly the value returned by
the matching `disable_interrupts`. Masking interrupts never excludes other
threads or cores. Hosted programs have no interrupt mask to change, which is
why the operations are no-ops there and why drivers that use critical sections
can still be tested on a desktop.

## Inline assembly

`asm(...)` emits instructions directly, for the few operations neither Dodo
nor `core/cpu` expresses: special registers, barriers, supervisor calls, or a
hand-tuned loop. It is an expression and requires an `unsafe` block:

```dodo
fn add(a: u64, b: u64) -> u64 {
    // AArch64.
    return unsafe { asm("add {0}, {1}, {2}", out(reg) u64, in(reg) a, in(reg) b) }
}
```

Template strings come first. Several strings are joined with newlines, one
instruction each. Operands follow, then optional clobbers and options:

| Operand | Meaning |
| --- | --- |
| `in(reg) value` | Pass `value` in a general-purpose register the compiler chooses. |
| `out(reg) Type` | The template leaves a `Type` in a register; the expression returns it. |
| `inout(reg) value` | Pass `value` and return the register's final value. |
| `in("r0") value`, `out("x9") u64`, ... | Use exactly this register, as calling conventions such as a supervisor call require. |
| `sym name` | Write the symbol of a function or static into the template, such as a call target. |
| `const value` | Write an integer constant expression into the template, as decimal text. |
| `clobber("r4", "lr")` | Further registers the template overwrites. |
| `options(nomem)` | The template reads and writes no memory the program can see. |
| `options(pure)` | Its only effect is its outputs: it may be removed if they are unused, or merged with an identical one. Needs an output. |

The template refers to operands as `{0}`, `{1}`, ... in the order written, or
`{}` for the next one. `{0:w}` passes an operand modifier to the assembler
(here AArch64's 32-bit register name). Write `{{` and `}}` for literal braces.
The result is `void` without outputs, the output's value with one, and a tuple
with several:

```dodo
fn swap_halves(value: u32) -> (u32, u32) {
    // Thumb.
    return unsafe {
        asm(
            "lsrs {0}, {2}, #16",
            "uxth {1}, {2}",
            out(reg) u32,
            out(reg) u32,
            in(reg) value,
        )
    }
}
```

Operands are integers up to the pointer width, raw pointers, or function
pointers. Inputs are evaluated in order before the template runs; an `out`
register never shares a register with an input, while `inout` reads and
writes the same one.

By default a template may read or write any memory and change the condition
flags: the compiler neither moves memory accesses across it nor removes it.
The `unsafe` block is your promise that the template matches its operands,
clobbers, and options, and that it leaves the stack pointer and every
unlisted register as it found them. Templates use the target assembler's
syntax: unified syntax on Arm and Thumb, AT&T syntax on x86. A template the
assembler rejects is a compile error with the assembler's message.

`sym` and `const` operands are text, not registers. `sym name` takes a
function (including a method or a C function) or a `static`, and writes the
name the linker sees, including the underscore Mach-O adds; the definition is
kept even when nothing else uses it. Assembly can only rely on the calling
convention of `extern "C"` functions. `const` takes any integer constant
expression and writes its value, so `#{0}` with `const SHIFT + 1` becomes
`#3`. Neither takes a modifier.

`std/embedded/cortex_m` shows typical uses: `cpsie i`, `wfe`, `dsb`, and
reading PRIMASK and IPSR.

## Global assembly

`unsafe asm(...)` at the top level of a file is assembly outside any
function: startup code, a function written entirely in assembly, or a table.
Its templates reach the assembler as written, and its operands can only be
`sym` and `const`:

```dodo
// AArch64.
unsafe asm(
    ".text",
    ".globl {0}",
    ".p2align 2",
    "{0}:",
    "    lsl x0, x0, #{1}",
    "    ret",
    sym times_eight,
    const 3,
)

unsafe extern "C" fn times_eight(value: u64) -> u64
```

Code calls what the assembly defines through an `extern "C"` prototype, like
any other foreign function. Select a section before emitting code or data:
which section is in effect at the start is unspecified. Blocks in one file
are emitted in order. `unsafe` is required and no attributes apply; the
assembly must not define a symbol a Dodo declaration also defines.

## Naked functions

`@naked` marks an `extern "C"` function whose body is one
`unsafe { asm(...) }` and which has no prologue or epilogue: no frame, no saved
registers, and no `return`. The assembly implements the C calling convention
itself: it finds its arguments where the convention passes them, leaves the
result in the result register, and returns.

```dodo
// AArch64.
@naked
extern "C" fn add(a: u64, b: u64) -> u64 {
    unsafe {
        asm("add x0, x0, x1", "ret")
    }
}

// Calls a Dodo function by symbol, saving the link register itself.
@naked
extern "C" fn call_bump() -> u64 {
    unsafe {
        asm(
            "stp x29, x30, [sp, #-16]!",
            "bl {0}",
            "ldp x29, x30, [sp], #16",
            "ret",
            sym bump,
        )
    }
}
```

The parameters are not variables, so the body can only refer to them through
the registers and stack slots they arrive in. The assembly takes only `sym`
and `const` operands, with no clobbers or options, and must not fall off its
end. A naked function is never inlined, and calling it is calling a C
function, so it needs an `unsafe` block at the call site. It cannot be
generic, a method, `async`, a test, or an `@interrupt` handler; `@section` and
`@align` work as for other functions. Firmware attributes that go with it,
`@interrupt`, `@section`, and `@align`, are described in
[hardware and embedded](hardware.md#interrupts).

## Atomic storage intrinsics

Prefer the checked `std/sync/atomic` API documented in
[synchronization](synchronization.md). Its implementation uses these unsafe
`core/mem` operations on aligned raw integer pointers:

| Operation | Returned value |
| --- | --- |
| `mem.atomic_load(pointer, order)` | Loaded integer. |
| `mem.atomic_store(pointer, value, order)` | `void`. |
| `mem.atomic_exchange(pointer, value, order)` | Previous integer. |
| `mem.atomic_fetch_add(pointer, amount, order)` | Previous integer; addition wraps at the integer width. |
| `mem.atomic_compare_exchange(pointer, expected, replacement, success, failure)` | Observed old integer; writes only when it equals `expected`. |

All operations require `unsafe`. Modification requires a mutable raw pointer;
load accepts a const pointer. Supported integer widths cannot exceed the target's
supported lock-free width, which this compiler bounds by pointer width. On
ARMv6-M only loads and stores are available; a reachable exchange, fetch-add,
or compare-exchange there is a build error, as is any atomic operation on a
target without atomics ([synchronization](synchronization.md#atomics)). An
optional explicit type argument must match the pointer's integer element type.

Orders are compile-time integers: 0 Relaxed, 1 Acquire, 2 Release, 3 AcqRel,
4 SeqCst. Loads allow 0, 1, 4; stores allow 0, 2, 4. Compare-exchange failure
cannot release or be stronger than success: success 0/2 allows failure 0,
success 1/3 allows failure 0/1, and success 4 allows failure 0/1/4. Invalid orders
are compile errors. The caller must provide valid initialized storage and obey
the atomic storage and concurrency contract; these calls are not a general
exception allowing arbitrary writes through shared references.


## Typed allocated storage

`mem.storage_type::<T>()` checks the supported element category. The unsafe
`ptr.store`, `ptr.take`, `ptr.relocate`, `ptr.view`, and `ptr.view_slice` primitives
require a matching zero-length typed witness in their checked owner. They track
element dependencies separately from storage borrows; they cannot store owned
Results or exclusive-reference elements. Pointer validity, initialized extents,
and exactly-once ownership transfer remain unsafe obligations. See the complete
[contracts and restrictions](container-elements.md#checked-region-invariant).
