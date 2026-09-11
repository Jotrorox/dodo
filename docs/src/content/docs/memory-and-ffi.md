---
title: "Memory and foreign calls"
description: "Implemented layouts, C interoperation, pointer intrinsics, and MMIO limits in Dodo 0.1.1."
section: "Language reference"
order: 230
---

This page covers the implemented memory layout and low-level interfaces in
Dodo 0.1.1. See the [compiler overview](implementation.md#remaining-design-surface)
for planned facilities that are not yet available.

## Struct layout and the C ABI

Struct fields keep declaration order and LLVM target ABI alignment/padding.
`@repr(C)` uses this C-compatible field layout. Native Dodo function ABI is
compiler-internal and unstable. Primitive and raw-pointer `extern "C"`
parameters/results use LLVM's C calling convention; C aggregate-by-value ABI
classification and variadics are not supported. Pass pointers to `@repr(C)`
structs instead. Imported foreign declarations use the declared final name as
the external symbol; Dodo functions use `dodo.<root-package>.<qualified-name>`.

## Enum, Result, and Option layout

Enum storage is an explicit tag plus storage for each variant's payload. Result
storage is an error tag plus success/error slots; Option storage is a presence
tag plus a payload slot. There are no niche optimizations or stable enum ABI.
`void` Result success uses an internal placeholder slot.

## Entry points and linking

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

## Unsafe memory access

Aliases above require their matching `core/...` import. Raw memory operations
remain subject to the specification's validity and aliasing obligations.
Reading/writing checked-borrow-carrying values through raw pointer intrinsics and
raw-pointer-to-checked-reference casts are rejected in this release. Unsafe
functions still need explicit unsafe blocks for unchecked operations.

## MMIO and volatile access

MMIO uses LLVM volatile loads/stores directly, without fabricating checked
references. The compiler rejects widths above the target pointer width; actual
device access legality remains a target/board obligation. Volatile operations
are not atomics or CPU barriers.

LLVM lowering follows the [LLVM Language Reference](https://llvm.org/docs/LangRef.html),
including explicit overflow intrinsics, volatile instructions, and x86 SysV
C-ABI integer extension attributes on declarations and call sites.
