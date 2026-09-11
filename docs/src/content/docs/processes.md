---
title: "Processes"
description: "Direct native process execution, owned child handles, explicit environments and bounded output collection."
section: "Using Dodo"
order: 147
---

`std/process` provides direct executable execution on x86-64 Linux/glibc and
Windows x64. Its selected `std/process/native` adapter uses an explicit C ABI
boundary. Importing it does not import filesystem, environment, threading, or
synchronization packages. The compiler links its small native boundary only
when needed; object-only consumers must also compile and link
`stdlib/std/process/runtime.c` with their target C toolchain.

## Selecting a command

`process.spawn(executable, arguments, environment, directory, options)` returns
`Child!std/platform/error.Error`. The executable is a borrowed
`std/platform/native.NativeString`; arguments and environment are `NativeList`
values. A list contains consecutive NUL-terminated native strings, with no extra
sentinel: an additional final NUL is an actual empty argument. Arguments exclude
`argv[0]`, which is the executable. The directory is
`Option<&native.NativeString>`. All input storage can be released after spawn
returns, including on startup failure.

`Options.new()` inherits all three standard streams and the parent environment.
`Options.capture()` selects null stdin and piped stdout/stderr. Set each stream
to `Stdio.Inherit`, `Stdio.Piped`, or `Stdio.Null` independently. With
`inherit_environment = false`, the supplied list is the complete child
environment, including when empty. With inheritance enabled, that list is
ignored after validation. `std/env` offers snapshot and builder facilities.

The default selects the executable directly. There is no command-string parsing,
shell expansion, wildcard expansion, or implicit shell. Shell execution is an
explicit spawn of `/bin/sh`, `cmd.exe`, or another selected shell, with that
shell's command option and command text supplied as arguments. Shell command
text is interpreted by the selected shell and has its own escaping rules.

Linux `search_path = true` explicitly enables `posix_spawnp` and observes the
parent's `PATH`, even when a different child environment is supplied. Windows
returns typed `Unsupported` for this option; provide the executable path.
Relative executable paths follow native platform resolution: Linux spawn file
actions change directory before execution, while Windows resolves the
application independently of the child's requested working directory. Use an
absolute executable when that distinction matters.

## Native strings and limits

Linux arguments and environment values preserve arbitrary non-NUL bytes;
filenames and arguments need not be UTF-8. Windows preserves native UTF-16,
including unpaired surrogate code units. Native string conversions and their
explicit lossy alternative are provided by `std/platform/native`; no locale
conversion takes place during spawn.

Windows passes the executable separately to `CreateProcessW`. Every argument is
quoted using the Windows C-runtime backslash/quote convention, preserving empty
arguments, embedded quotes, and trailing backslashes. Programs using a custom
command-line parser can interpret that command line differently. Explicit child
environments are sorted with Windows ordinal case-insensitive comparison and
terminated with two NUL units.

The native boundary uses bounded stack scratch, without a global Dodo allocator.
Linux supports at most 4,096 total arguments including `argv[0]`, and 4,096
explicit environment entries. Windows supports a command line below 32,768
UTF-16 units and an explicit environment below 32,768 units. Exceeding an
adapter bound returns `BufferTooSmall`; the OS can impose additional native
limits such as Linux `E2BIG`. Native process creation and Windows handle setup
can allocate OS-managed resources.

## Child and pipe ownership

`Child` uniquely owns the process and each untaken pipe. `take_stdin`,
`take_stdout`, and `take_stderr` return `Option<Pipe>` and transfer that pipe's
ownership exactly once. `Pipe` implements `std/io`'s read/write contracts and
has a recoverable `close` operation; destruction closes once, best effort. A
taken pipe has an independent lifetime. Unix pipe writes suppress only a newly
generated SIGPIPE in the writing thread, so a closed child reader returns a
recoverable native EPIPE error. Linux captured output pipes are nonblocking;
a direct read without available bytes returns an error carrying EAGAIN.

`wait()` waits indefinitely without draining pipes. `wait_timeout(milliseconds)`
returns `TimedOut` while retaining the live child. `try_wait()` returns `none`
while running. The timeout clock is monotonic, polled at approximately one
millisecond intervals; scheduling can delay observation of a deadline.

`ExitStatus.kind` distinguishes `Exited`, Unix `Signalled`, and explicit
`Cancelled`. `code` carries the ordinary exit code or signal number; Windows
32-bit exit-code bits are preserved in the signed `i32` representation.
`success()` requires ordinary exit with code zero. Spawn errors are separate
from every exit status. `terminate()` requests SIGTERM on Linux and uses
`TerminateProcess` with exit code 1 on Windows; it does not wait. `cancel()`
forcibly terminates and reaps an unfinished child and records `Cancelled`.
Cancelling an already observed completed child preserves its status.

Dropping an unfinished `Child` **closes its owned pipes, forcibly terminates the
child, and reaps it**. Completed children release their process resources.
This policy prevents leaked zombies and abandoned subprocesses on every normal
Dodo scope-exit path. It applies to the immediate child; descendants are not
managed as a process group or Windows job. OS scheduling may delay destruction.
Dodo's aborting traps do not unwind, so they do not run these destructors.
Process detach and process-tree management are not supplied.

Linux creates pipes close-on-exec and uses spawn close-from actions to prevent
inheritance of unrelated descriptors. Windows duplicates inherited standard
handles, makes parent pipe endpoints non-inheritable, and passes an explicit
three-handle inheritance whitelist. Every partial startup path releases pipes,
file actions, attribute lists, and process/thread handles before returning.

## Collecting output

`child.collect(stdout_storage, stderr_storage, timeout_ms)` closes owned stdin,
drains both captured streams fairly, and waits for exit. It handles output
larger than either OS pipe capacity without the stdout/stderr ordering deadlock.
There is no scheduler or worker thread. Returned `Output` contains an exit
status and each initialized byte count; output is arbitrary bytes, with no
implicit text decoding. `process.FOREVER` is an explicit unlimited deadline.

The two supplied slice lengths are independent hard size limits. Exactly full
output is accepted after probing for EOF; one additional byte triggers
`BufferTooSmall`. Collection failure reports each retained initialized prefix,
closes the remaining pipes, forcibly terminates and reaps the child. Timeout is
reported as `TimedOut`, and native read/wait failures preserve native error
codes. Taken output pipes are no longer collected. Writing interactive input
must happen before collection or through separately managed taken pipes; this
API does not multiplex simultaneous streaming stdin.

For allocation-dependent collection, import `std/process/alloc`.
`collect_arena(child, stdout_arena, stderr_arena, stdout_limit, stderr_limit,
timeout_ms)` allocates the two limits explicitly and returns owned buffers whose
allocator borrows remain checked. Generic `unsafe collect` accepts two
allocators satisfying `std/bytes_alloc.new`'s documented contract. Allocation
failure leaves the child untouched. Collection failure reaps it and releases
both buffers; use caller-backed collection to retain partial output on failure.
There is no global allocator or hidden growth beyond the chosen limits.

## Verification

Native fixtures execute at `-O0` and `-O3` with controlled C children and temporary
directories. They cover 262,144 bytes on each output stream, per-stream limits,
quotes, spaces, empty arguments, trailing backslashes, native Unix argument
bytes, environment inheritance and replacement, explicit working directories,
stdin ownership and EOF, exit/signal/cancellation/timeout distinctions, missing
executables, and destructor cleanup. The Windows harness compiles and executes
real Windows parents and child programs under Wine using the same contracts.
