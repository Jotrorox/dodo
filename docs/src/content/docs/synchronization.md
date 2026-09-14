---
title: "Synchronization"
description: "Native atomics, guarded locks, condition waits, one-time initialization, barriers, and channels."
section: "Standard library"
order: 156
---

Use `std/sync.Storage.new` and `sync.Mutex.new` to protect a value in caller
storage on supported hosted targets. `std/sync/error` names lock failures.
Start here for local ownership; use `std/sync/allocated` when independent
owners must move into [native tasks](threads.md).

## Quickstart

Save this as `sync_start.dodo`:

```dodo test
package sync_start
import "std/sync"
import "std/sync/error"

fn increment() -> void!error.Error {
    storage := sync.Storage.new()
    counter := sync.Mutex.new(&mut storage, 41usize)?
    guard := counter.lock(1000)?
    {
        value := guard.get_mut()
        *value += 1
    }
    assert_eq(*guard.get(), 42usize)
    // guard unlocks before counter and its storage are destroyed.
    return ok()
}
fn main() -> i32 {
    match increment() {
        ok() => { return 0 },
        err(reason) => {
            if reason == error.Error.TimedOut { return 2 }
            return 1
        },
    }
}
```

```sh
dodo run sync_start.dodo
```

Expected output: none; exit 0 confirms the guarded value became 42. Exit 2
means the lock budget expired: skip the work or retry under an overall deadline.
Exit 1 means initialization or another lock operation failed. Propagate that
failure and release owners; do not access the payload without a guard.
The storage is fixed and caller-owned; no Dodo allocator is selected.

`std/sync/atomic` is a separate integer-atomic family: start with
`atomic.Atomic.new(0usize)` and `Ordering.SeqCst` unless you have a proven weaker
ordering protocol. Its actual target restrictions are in [Atomics](#atomics).

## API and contracts

`std/sync` provides caller-backed synchronization. `std/sync/allocated` provides
retained owners that explicitly allocate OS pages and can move between native
[threads](threads.md). `std/sync/atomic` is independent of OS blocking facilities.
Fallible operations return Results that programs must handle.

## Ownership and allocation

Create `sync.Storage.new()` and pass its exclusive borrow to `Mutex.new`,
`RwLock.new`, `Barrier.new`, or `Once.new`. The returned owner keeps storage
alive and immovable until destruction. Channels also borrow uninitialized
`MaybeUninit<T>` slots. These owners do not allocate.

Shared allocated constructors explicitly consume `allocated.PageAllocator.new()`.
For example, `allocated.Mutex.new(allocated.PageAllocator.new(), 0usize)` maps
independent OS pages. `clone()` retains the allocation. Each clone can move into
a task; the last owner destroys its payload and releases the mapping. No global
Dodo allocator is installed. Allocator borrows cannot be hidden inside these
owners. Payload alignment currently cannot exceed 256 bytes; invalid layouts
and allocation failures are recoverable. Reference-count overflow aborts.

Mutex/channel payloads must pass checked cross-thread transfer rules. RwLock
payloads must also pass sharing rules. Raw pointers, references, borrowed
allocators, opaque storage, unhandled Results, and thread-affine destructors do
not qualify by default. Explicit unsafe transfer/sharing implementations have
the obligations described in the [thread guide](threads.md).

## Locks, guards, and waits

`mutex.lock(timeout_ms)` returns a guard. Its `get()` and `get_mut()` views borrow
that guard, preventing unlock, wait, movement, destruction, or lifetime escape
while a view is live. Guards are thread-affine and release locks on normal scope
exit or explicit `core.drop`. Bind a mutable view before assigning through it:

```dodo
guard := lock.lock(1000)?
{
    value := guard.get_mut()
    *value += 1
}
core.drop(guard)
```

`rwlock.read(timeout_ms)` permits concurrent readers; `write(timeout_ms)` returns
an exclusive guard. Waiting writers prevent new readers from barging. Scheduling
is not FIFO or guaranteed starvation-free. Locks are nonrecursive; upgrades and
downgrades are unavailable.

Wait budgets are relative monotonic milliseconds. Zero tries immediate progress;
`sync.forever()` is infinite. `sync.milliseconds(&duration)` rounds a
`std/time.Duration` upward and rejects overflowing budgets. OS descheduling and
lock reacquisition can exceed a timeout; these are not hard real-time deadlines.
`cancel()` permanently cancels the owner and wakes waiters. Existing guards remain
valid and unlock normally.

## Conditions

`mutex.condition()` creates a condition variable permanently bound to that mutex.
`condition.wait(&mut guard, timeout_ms)` releases exclusive ownership, waits,
and reacquires it before returning, including timeout/cancellation. Another
mutex's guard returns `InvalidInput`. `guard.wait(timeout_ms)` is equivalent.
Both interfaces provide `notify_one()` and `notify_all()`.

Check the protected predicate in a loop: spurious/stolen wakeups are allowed.
Condition notifications are separate from lock-acquisition wakeups; notification
does not select a particular condition waiter. Change predicates while holding
the mutex. The rules follow the
[POSIX condition API](https://man7.org/linux/man-pages/man3/pthread_cond_wait.3.html)
and [Windows condition API](https://learn.microsoft.com/en-us/windows/win32/sync/condition-variables).

## Once and barriers

`Once.new::<T>(storage)` and its allocated counterpart serialize
`get_or_init(&mut initializer, timeout_ms)`. The structural method
`initialize(&mut self) -> T!error.Error` supplies initialization; closures are
not assumed. Failure leaves the value retryable. A successful initializer runs
once. Its `OnceGuard<T>` returns read-only `&Option<T>` containing `some(value)`;
callers cannot reset it. Initializers must avoid recursively acquiring the same
Once.

`Barrier.new(storage, parties)` rejects zero. Each generation releases after that
many participants arrive, then resets. Timeout permanently breaks the barrier
and wakes other participants with `Cancelled`. Cancellation also breaks future
waits, preventing abandoned participants from contaminating another generation.

## Channels

`Channel.new::<T>(storage, slots)` creates a bounded caller-backed ring.
`allocated.Channel.new::<T>(allocated.PageAllocator.new(), capacity)` explicitly
allocates a clonable bounded ring. Neither grows implicitly; unbounded/growing
channels are not implemented.

`send(value, timeout_ms)` moves ownership. Failure destroys the value once and
returns its reason. `receive(timeout_ms)` returns `some(value)`, or `none` when
closed and empty. `close()` rejects new sends and wakes waiters; queued messages
remain drainable. Cancellation also wakes waiters while allowing buffered
messages to drain; an empty cancelled receive returns `Cancelled`. Zero-time
operations return `TimedOut` for a full/empty live ring.

Closure is explicit. Owners are symmetric handles, not separately counted
sender/receiver endpoints; dropping one clone does not close the channel. The
last owner drains and destroys unread messages outside native locks before
releasing storage. Programs must close/cancel when their protocol needs blocked
peers to exit.

## Atomics

`atomic.Atomic.new(value)` stores an inline integer. `allocated.Atomic` supplies
a retained stable allocation for sharing the same atomic across move tasks.
Both expose `load`, `store`, `exchange`, `fetch_add`, and strong
`compare_exchange`, with explicit `atomic.Ordering`: Relaxed, Acquire, Release,
AcqRel, or SeqCst. `fetch_add` wraps. Compare-exchange returns
`{ previous, exchanged }`; failure does not modify the value.

Loads accept Relaxed/Acquire/SeqCst; stores accept Relaxed/Release/SeqCst.
Compare-exchange failure ordering cannot release or exceed success ordering.
Invalid dynamic combinations return `InvalidOrdering`. Unsafe `mem.atomic_*`
intrinsics require literal ordering codes 0–4 and diagnose invalid combinations.
Pointers must be aligned, live, and accessed through a compatible atomic protocol.
Volatile/MMIO operations are not synchronization.

The backend supports native integer atomic instructions on x86-64 and AArch64,
widths 8/16/32/64 no wider than the target pointer. It inserts no OS/libatomic
fallback. Hosted x64 tests exercise the operations; other accepted architectures
are compilation capabilities, not claimed hardware verification. Unsupported
targets, including Cortex-M0 and non-atomic WebAssembly, receive a build
diagnostic. Importing unused atomic declarations requires no OS adapter.
Payload operations are lock-free for those supported native widths. Retaining
and destroying shared owners use native locks; constructors and blocking APIs
have no lock-free guarantee.

## Destruction and limits

Dodo traps abort without unwinding. Poisoning is therefore not meaningful: no
surviving Dodo caller can recover a poisoned guard after a trap. Impossible native
ownership/destruction failures abort rather than release storage still in use.
The library supplies no scheduler, asynchronous cancellation, process-shared
locks, robust mutexes, priority inheritance, or Dodo thread-local storage API.
