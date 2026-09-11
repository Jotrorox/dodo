---
title: "Filesystem and native platform values"
description: "Native paths, files, directories, explicit storage, ownership, and Linux/Windows filesystem guarantees."
section: "Using Dodo"
order: 145
---

`std/fs` provides synchronous, recoverable filesystem operations. It imports
`std/io` and independently selects `std/fs/linux` or `std/fs/windows` through
`std/fs/native`. It does not import processes, environment, threads, a scheduler,
or an allocator. The supported native ABIs are x86_64 Linux GNU and Win64.
Other targets, including Linux musl and non-x86_64 Linux, receive a build
diagnostic rather than silently using an incompatible `stat` layout.
`core`, `alloc`, portable I/O, text, time, and `std/fs/path` remain freestanding.

## Native paths and storage

Import `std/platform/native` for `NativeString`. On Linux,
`NativeString.new(&[u8])` accepts arbitrary filename bytes. On Windows it accepts
`&[u16]`, including unpaired UTF-16 surrogates. Both constructors require one
final NUL and reject interior NULs. `units()` excludes the terminator;
`terminated()` includes it. The value borrows its backing storage, and checked
borrowing prevents mutation or destruction of that storage while the path is
in use. It owns no allocation.

`native.from_utf8(text, byte_scratch, wide_scratch)` provides the same signature
on both targets. Linux uses byte storage and Windows uses wide storage; both
buffers are explicit and both remain conservatively borrowed. Supply sufficient
space for the final NUL. The operation never consults the locale or environment.
`native.to_utf8(path, output, false)` is strict. Passing `true` explicitly
requests replacement of malformed Unix bytes or unpaired Windows surrogates
with U+FFFD. Insufficient conversion space returns `BufferTooSmall`; conversion
may have written a prefix. Native filesystem calls never perform this lossy
conversion implicitly. Windows paths go to the wide Win32 APIs unchanged,
including caller-provided verbatim path prefixes.

`NativeList` is a borrowed sequence of NUL-terminated native strings. An empty
slice is an empty list; a nonempty list must end in NUL. It is useful for
arguments and environment blocks. Its constructor adds no implicit second
terminator and does not apply environment-name policy.

```dodo
package example
import "std/fs"
import "std/platform/native"
import "std/platform/error"

fn inspect() -> u64!error.Error {
    bytes := [0u8; 256]
    wide := [0u16; 256]
    path := native.from_utf8("report Ω.txt", &mut bytes, &mut wide)?
    options := fs.OpenOptions.read_only()
    file := fs.File.open(&path, &options)?
    info := file.metadata()?
    file.close()?
    return ok(info.size)
}
```

## Files and open modes

`OpenOptions.new()` disables every access mode. At least one of `read`, `write`,
or `append` must be enabled. `read_only()` enables reading; `create_new()`
enables writing and exclusive creation. Invalid combinations fail before
calling the OS or changing a file.

| Option | Contract |
| --- | --- |
| `read` | Permit reading from the current cursor. |
| `write` | Permit writes at the cursor; does not create or truncate by itself. |
| `append` | Each native write appends at the then-current end, even after seeking. |
| `truncate` | Empty an existing file when opening; requires `write`. |
| `create` | Create if missing; preserve existing contents unless truncating. |
| `exclusive` | Requires `create`; fail if the path already exists, including a symlink. |
| `follow_symlinks` | Defaults to true. False refuses a final symlink; intermediate links can still be followed. |

`append` with `truncate` is rejected on both adapters. On Windows append access
uses `FILE_APPEND_DATA`, so ordinary writes cannot overwrite after seeking.
Windows append handles may reject length changes or persistence requests that
require broader write access; reopen with ordinary `write` access for those
operations. An individual OS append operation selects EOF atomically on supporting local
filesystems; several writes are not a transaction, and network filesystem
append behavior depends on its implementation. Linux descriptors are created
with `O_CLOEXEC`, and Windows handles are non-inheritable.

`File.read`, `write`, and absolute `seek` satisfy the structural `std/io`
contracts. Empty reads/writes succeed with zero without accessing a device;
nonempty reads returning zero mean EOF. Short operations are valid. Use
`io.read_exact`, `read_up_to`, `write_all`, and `copy` for completion behavior.
Linux writes suppress only their own thread-generated SIGPIPE, preserving the
caller's signal mask and process signal disposition, so a closed pipe reports
recoverable EPIPE. Native I/O errors retain their native code in `io.Error.code`; interruption is
represented as `io.ErrorKind.Interrupted`. Seeking beyond EOF does not grow a
file. `set_len` changes length without changing the cursor; extending a regular
file exposes zero bytes, subject to native filesystem behavior.

Files have no userspace buffer, so `flush` is a successful no-op. Flush an
`io.BufferedWriter` separately before `File.sync`. `sync` calls `fsync` or
`FlushFileBuffers`; it requests persistence according to the OS/device and does
not independently persist the parent directory entry. Linux directory handles
can be synced. Windows directory persistence is not offered as a capability.
No operation implicitly syncs on drop.

`File` owns one `native.Handle`. Moving transfers ownership. `close()`
invalidates the owner before the native call and reports errors; it is safe to
call again. Destruction closes once and discards unreportable close errors.
Linux `close(EINTR)` is never retried, because the descriptor may already be
closed and its number reused. Call `close()` when a close error matters. The
unsafe `File.from_raw` and `native.Handle.from_raw` constructors require unique
ownership of a valid synchronous native resource; `raw_handle()`/`raw()` do not
transfer ownership. Traps abort the process and do not run destructors.

## Metadata, links, directories, and permissions

`fs.metadata` follows the final symlink; `symlink_metadata` inspects the final
link itself. `File.metadata` describes the open object. Results use
`std/fs/types.Metadata`, with kind, size, Unix-epoch modification seconds and
nanoseconds, a read-only summary, native file identity, and link count. The
`unix_metadata` flag explicitly marks whether mode/uid/gid are available;
Windows returns zero for those Unix fields. Windows reparse entries are
classified as `SymbolicLink`; not every reparse tag is an ordinary symlink.
Metadata is an observation and is not an authorization or race-free identity
check for a later path-based operation.

`Directory.open(path, byte_scratch, wide_scratch)` opens an iterator. Windows
uses caller wide scratch to build its search pattern. `next` copies a name,
including its native terminator, into caller storage and returns
`Option<NativeString>`. It does not retain that storage. The returned name is a
single component, not a joined path. A `BufferTooSmall` result preserves the
pending entry for retry. Dot entries are skipped; enumeration order and the
visibility of concurrent modifications are unspecified. End of iteration is
stable until close. The native iterator owns its native directory resource;
Linux libc also owns its internal directory buffer, with allocation failure
reported through errno. No Dodo collection or global Dodo allocator is created.

`create_directory` creates one directory and fails when it already exists;
`remove_directory` removes one empty directory. Neither recursively traverses.
`remove_file` unlinks a file or a file symlink, leaving the target intact.
Windows directory symlinks are removed with `remove_directory`.

`symlink(target, path, directory)` stores the native target. Linux ignores the
directory hint; Windows requires it for directory links and may require the
appropriate privilege/developer-mode support. Runtime failure is a typed native
error. `read_link` reads the stored target without following it. Linux requires
byte result space including a NUL. Windows requires 16 KiB of caller byte
scratch for reparse data and wide result space; unsupported reparse tags return
`Unsupported`. Its result prefers the print name and otherwise preserves the
native substitute name. There is no implicit privilege escalation or fallback.

`set_readonly` maps to the platform's permission model. Linux follows links,
clears every write bit when making a file read-only, and restores only owner
write permission when making it writable. Windows changes the file attribute;
it does not implement ACL editing, and read-only directories are not an access
control boundary. Do not use this convenience function to preserve a precise
Unix mode or to make security claims about Windows permissions.

`std/fs/unix` separately exposes `open(..., mode)`, `set_mode`, and
`create_directory(..., mode)`. Modes are bounded to `07777`, and creation modes
are filtered through the process umask. `std/fs/windows_ext.open` exposes
`SHARE_READ`, `SHARE_WRITE`, and `SHARE_DELETE`; ordinary opens share all three.
Opening an incompatible extension for the selected target produces a build
diagnostic. Sharing rights govern other opens/removals, independently of this
handle's read/write access rights.

## Rename, copying, and path resolution

`rename(old, new, replace)` uses the native rename operation. Linux uses
`renameat2(RENAME_NOREPLACE)` when replacement is forbidden, which may report
`Unsupported` on older kernels/filesystems. Windows uses `MoveFileExW` without
`MOVEFILE_COPY_ALLOWED`. A cross-filesystem move returns `CrossDevice`; it never
becomes a copy followed by deletion. A failed operation does not trigger an
alternate destructive operation. Replacement on Linux is an atomic namespace
change on supporting filesystems. Windows applies its native sharing and
filesystem restrictions; no broader transaction or crash-persistence guarantee
is made. A successful rename does not itself sync either parent directory.

`copy_new(source, destination, scratch, limit)` copies regular-file contents
through caller storage and creates the destination exclusively. Existing
contents are never truncated, including when source and destination name the
same file. The source is opened and checked before creating the destination.
A zero scratch buffer with a nonzero limit is rejected before creation.
The result is `io.CopyReport`; `eof=false` means the hard byte cap was reached,
not that the whole source was copied. Permission bits, ownership, times, and
link identity are not copied. Successful transfer also checks explicit closure of both files. Failure reports
consumed/written byte counts;
a partial new destination remains for explicit inspection or removal. There
is no racy implicit unlink, replacement, or rollback. Copying does not guarantee
a consistent snapshot against concurrent source mutation and does not sync.

`canonicalize` resolves an existing path through the filesystem, following
symlinks. Linux requires at least 4096 bytes of scratch for nonallocating
`realpath`. Windows uses `GetFinalPathNameByHandleW` and returns its native
normalized final name, commonly including a verbatim prefix. The result is a
snapshot, not a stable capability that prevents subsequent replacement.

`std/fs/path.lexical_unix` and `lexical_windows` perform a separate, conservative
lexical operation using no OS calls. They collapse redundant separators and
`.` components, preserve all `..` components, and retain Unix double-root and
Windows drive-relative/UNC forms. They never claim that two names identify the
same object. Windows device/verbatim prefixes return `Unsupported` because
their rules differ. Input excludes a NUL; output requires input length plus
one units, includes a final NUL, and the returned count excludes it. Empty
input and interior NULs are invalid. Validation/capacity failure leaves output
untouched. This package cross-compiles to freestanding targets.

## Errors and current scope

`std/platform/error.Error` combines a portable `Kind` with an unmodified native
`code`: errno on Linux, a Win32 error number on Windows, and zero for a library
validation error. `capabilities()` reports symbolic-link API availability,
Unix permissions, Windows sharing modes, and directory sync independently.
Availability does not promise that a particular filesystem or account permits
an operation. Results must be matched, propagated, or otherwise explicitly
handled under the ordinary language rules.

The initial adapters are synchronous. They do not provide recursive tree
operations, filesystem watching, ACL editing, transactional copying, relative
handle-based traversal, or async I/O. Checked references protect Dodo storage
and owner lifetimes; they do not eliminate races caused by other processes
changing a filesystem namespace. File data allocations remain caller-selected;
native OS resources and libc directory state retain their native resource
allocation behavior.

Validation includes native fixtures at `-O0` and `-O3`, a 64-combination
open-mode matrix for both existing and missing files, Unicode/space paths,
Unix non-UTF-8 names, symlinks, permissions, directory-buffer retries, copying
limits, cross-device failure preservation, and descriptor cleanup on failure.
The Windows harness links and executes real PE fixtures under Wine, including
sharing denial and strict/lossy unpaired-surrogate conversion. Rejection tests
cover escaping native paths, mutable aliases, iterator name lifetimes, ignored
Results, and private resource construction.
