---
title: Native threads and checked transfer
description: Native tasks, joining, explicit detachment, and compiler transfer contracts.
section: "Standard library"
order: 155
---

`std/thread` starts an operating-system thread to run a task value's public
`run` method. The task moves into the worker; `join` waits for completion and
moves its result back. Start with caller-owned `Storage` and an explicit join.
This hosted package supports Linux GNU x86-64 and Windows x64.

Read [ownership](ownership.md) before sharing data between threads. Being safe
to move within one thread does not necessarily mean a value can move to another
thread. Borrowed references, raw pointers, and resource owners need additional
transfer guarantees. The simple task below contains only an owned integer.

## Quickstart

Save this as `thread_start.dodo`:

```dodo test
package thread_start
import "std/thread"

pub struct Double {
    value: usize
    pub fn run(self) -> usize { return self.value * 2 }
}
fn calculate() -> usize!thread.ThreadError {
    storage := thread.Storage.new::<Double, usize>()
    worker := thread.spawn(&mut storage, Double { value: 21 })?
    return ok(worker.join())
}
fn main() -> i32 {
    match calculate() {
        ok(value) => { assert_eq(value, 42usize); return 0 },
        err(_) => { return 1 },
    }
}
```

```sh
dodo run thread_start.dodo
```

Expected output: none; exit 0 confirms the worker returned 42. Exit 1 means
thread creation failed: reduce concurrent work or report the resource failure.
The task and `run` method are public for static dispatch from the library.
Storage holds the task, result and handle; the OS still provides a native stack.
Pass owned scalar data as above. Captured checked references and unhandled
Results cannot be hidden in tasks; the full transfer restrictions follow.

## API and contracts

`std/thread` starts native Linux pthreads or Windows CRT threads. Each adapter
is selected from the compilation target independently of filesystem, process,
environment, and synchronization imports. Unsupported hosted targets produce a
build diagnostic. No scheduler or global Dodo allocator is installed.

Tasks are ordinary values with a public `run` method. Dodo statically dispatches
that method; closures are not assumed. Both the task type and its method must
be public so the standard package can call them. Ownership moves into the
worker, and the return value moves back through `join`. For a task without data
to return, use `thread.Unit` instead of `void`.

| Choice | Storage | End-of-scope behavior | Can detach? |
| --- | --- | --- | --- |
| `spawn(&mut storage, task)` | Caller-owned `Storage<T, R>` | Dropping `Join` waits and destroys the unclaimed result. | No |
| `spawn_owned(&allocator, task)` | Explicit native mapping | Dropping `OwnedJoin` also waits. | Yes, by consuming `detach()` |

Create several storage owners and spawn all workers before joining them if they
should overlap. Spawning and immediately joining inside one loop runs tasks
one after another. `join` is a synchronization point, not a cancellation request.

### A task with no return data

Use `thread.Unit` for a task whose completion is the only result. This complete
test still joins the worker, so it never leaves background work behind:

```dodo test
package unit_task
import "std/thread"

pub struct Work {
    pub fn run(self) -> thread.Unit {
        return thread.Unit {}
    }
}

@test
fn joins_a_task_without_data() {
    storage := thread.Storage.new::<Work, thread.Unit>()
    worker := thread.spawn(&mut storage, Work {})!
    completed := worker.join()
    core.drop(completed)
}
```

### Caller-owned storage

`Storage<T, R>` contains caller-provided task, result, and native-handle storage.
The native implementation may allocate operating-system resources such as the
thread stack. `spawn` returns a `Join<T, R>` with an exclusive checked borrow of
the complete storage. Moving, replacing, destroying, or reusing that storage
while the handle is alive is rejected. `join(self)` waits, closes the native
thread handle, and returns the result. Dropping a join handle waits and destroys
the result. Storage can then be reused.

Destruction runs on ordinary returns, `?`, `break`, `continue`, explicit drops,
and normal block exits. Consequently a caller-backed worker cannot outlive its
storage on any supported normal exit path. Checked-reference capture is not
supported: a scoped storage borrow is not a proof that a captured reference is
safe on another thread. Borrowing task APIs will require a separate lifetime
and sharing proof. Caller-backed join handles cannot detach.

### Owned and detached workers

`spawn_owned::<T, R>(&thread.NativeAllocator, task)` explicitly requests an
independent native mapping. Construct the allocation token with
`thread.NativeAllocator.new()`. It does not borrow an arena, access a global
Dodo allocator, or create an implicit process-wide allocation policy. Mapping
allocation and native startup failures return `ThreadError` with `ErrorKind`
and a native code. Unsupported mapping alignment returns an allocation error.
On all failures the task is destroyed exactly once and acquired mappings are
released. The owned handle can move between threads.

`OwnedJoin.join` and automatic destruction have the same waiting behavior as
caller-backed joins. `OwnedJoin.detach(self)` explicitly gives up the result.
The worker keeps running, destroys its result, and releases its mapping. An
atomic handoff covers both completion-before-detach and detach-before-completion
without leaking or releasing storage twice. Process exit does not wait for
detached workers.

### Completion, yielding, and cancellation

`is_finished(&self)` provides an acquire observation of result publication;
`join` still performs the native wait and handle closure. `yield_now()` is a
scheduling hint. `sleep_ms(u32)` requests a finite delay, subject to
operating-system timer resolution and scheduling. Interrupted Linux waits
resume their remaining duration. Neither
operation supplies fairness or realtime guarantees. Applications can use
`is_finished` with a bounded deadline or transfer a synchronized cancellation
token in the task. There is no forcible thread termination or implicit
cancellation. Dropping a worker handle can therefore wait indefinitely if its
task never completes.

## What can cross a thread boundary

Compiler thread capability checks are separate from borrowing checks.
`core.mem.assert_send::<T>()` checks ownership transfer;
`core.mem.assert_sync::<T>()` checks shared concurrent access. They emit no
runtime code. Scalars, arrays, options, and ordinary aggregates are checked
recursively. References, slices, strings, raw pointers, opaque storage, and
types with destructors are rejected by default. This excludes arena-backed
allocations, single-threaded shared arenas, raw-pointer-bearing containers, and
thread-affine resource owners. Shared synchronization owners provide explicit
contracts in their implementation and validate their generic payloads at
construction.

`@unsafe_send` and `@unsafe_sync` on a struct are **unsafe implementation
contracts**, not compiler proofs. The author must guarantee that moving the
complete owner, including destruction, or concurrently calling shared methods
is safe. Annotated raw-pointer owners must preserve allocation lifetime,
exclusivity, synchronization, and exactly-once release. Private fields and
checked constructors prevent callers from bypassing those guarantees. Generic
owners must enforce all required payload capabilities in every constructor.
The attributes cannot erase a checked borrow contained in the struct.
These contracts should remain confined to audited ownership adapters.

Task and result storage also passes through the checked `mem.init` boundary.
Values containing checked borrows or `Result` obligations cannot enter opaque
thread storage. Tasks must handle a `Result` before producing a plain result
value or an explicit non-Result status enum. Silently dropping an unhandled
`Result` through a join handle is rejected. This restriction also applies to
nested fields.

## Native callbacks and abnormal termination

The compiler's unsafe `mem.callback::<Types...>(function_name)` specializes a
statically named function and supplies its native address. It requires the
exact signature `unsafe fn(*mut u8) -> void`, an unsafe block, and normal package
visibility. It neither executes the function nor creates a closure. Runtime
adapters supply the actual pthread / Windows callback ABI trampoline and use
target headers for native handles and layouts. Layout bounds are checked by C
static assertions. Ordinary Dodo callers cannot call arbitrary raw function
pointers safely.

Dodo traps abort the whole process; they do not unwind a single worker. There
is no thread panic result or lock poisoning recovery. Successful joining orders
worker writes before result access. Unexpected native join failures abort
because returning while a worker could still access borrowed storage would
violate the lifetime guarantee. Safe ownership excludes joining oneself or
joining an already-consumed handle. Linux workers disable POSIX cancellation.
Normal native thread return runs supported native runtime TLS destructors;
Dodo has no language-level thread-local declaration/destructor facility yet.

## Atomic primitives

For ordinary application code, start with [synchronization](synchronization.md)
for guards, channels, and checked atomic operations. The following raw
intrinsics are primarily for implementing ownership and synchronization adapters.

Raw integer atomics are independent of OS blocking adapters. The compiler
currently supports 8/16/32/64-bit integer operations up to pointer width on x86-64
and AArch64 targets, and rejects other targets with a build diagnostic. No
`libatomic`, allocator, or OS fallback is inserted. Naturally aligned operations
are lock-free; AArch64 read/modify/write operations may use retry loops, so they
are not promised wait-free. Unsupported target widths are rejected. Raw
callers must provide live, naturally aligned storage and must not race atomic
accesses with non-atomic accesses.

Order codes are `0` relaxed, `1` acquire, `2` release, `3` acquire/release, and `4`
sequentially consistent. Intrinsic order arguments must be compile-time
constants. Loads reject release/acquire-release; stores reject
acquire/acquire-release. Compare-exchange failure order cannot release and
cannot be stronger than success order. Compare-exchange returns the observed
previous value and is strong: equality with the expected value denotes
success. Arithmetic fetch-add wraps at the integer width, as required by the
atomic operation. The checked `std/sync/atomic` facade exposes typed order
values and typed errors for invalid runtime order combinations.

The native ABI and ordering rules follow the
[LLVM atomic memory model](https://llvm.org/docs/Atomics.html),
[POSIX pthread join contract](https://pubs.opengroup.org/onlinepubs/9799919799/functions/pthread_join.html),
and [Windows CRT thread creation contract](https://learn.microsoft.com/en-us/cpp/c-runtime-library/reference/beginthread-beginthreadex).

## Complete API reference

For every public type, field, constant, and function signature in the thread
package, see [std/thread](api/std/thread.md). Compiler-provided `core.mem`
intrinsics are covered in [memory and FFI](memory-and-ffi.md).
