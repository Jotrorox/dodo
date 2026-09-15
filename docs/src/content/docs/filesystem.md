---
title: "Filesystem and native platform values"
description: "Native paths, files, directories, explicit storage, ownership, and Linux/Windows filesystem guarantees."
section: "Standard library"
order: 152
---

Use `std/fs.read_file` to read a small file into a fixed byte array. Import
`std/platform` for reusable path-conversion storage and `std/console` to display
the returned bytes. These are hosted APIs for Linux GNU x86-64 and Windows x64.

## Quickstart

In a fresh example directory, create the fixture with Python 3:

```sh
python3 -c "from pathlib import Path; Path('message.txt').write_bytes(b'Hello, file!\n')"
```

Save this as `read_file.dodo`:

```dodo
package read_file
import "std/fs"
import "std/platform"
import "std/platform/error"
import "std/console"
import "std/io"

fn main() -> i32 {
    workspace := platform.workspace()
    contents := [0u8; 64]
    match fs.read_file("message.txt", &mut contents, &mut workspace) {
        ok(report) => {
            if !report.eof { return 2 }
            output := console.stdout()
            match io.write_all(&mut output, &contents[..report.read]) {
                ok(_) => { return 0 },
                err(_) => { return 3 },
            }
        },
        err(reason) => {
            if reason.cause.kind == error.Kind.NotFound { return 4 }
            errors := console.stderr()
            match errors.println(&reason.cause) {
                ok(_) => {}, err(_) => {},
            }
            return 1
        },
    }
}
```

```sh
dodo run read_file.dodo
```

Expected stdout is `Hello, file!` followed by a newline; success exits 0.
The 64-byte array bounds file contents; `platform.workspace()` supplies a
4096-unit native path buffer. No allocator is required.

Exit 4 means the file is missing: run the setup in the same working directory.
Exit 2 means the file exceeds the destination: enlarge it or stream with
`File.open_utf8` and `io.copy`. Exit 3 means stdout failed. Other file errors
exit 1; inspect `reason.cause.kind` and native `code` (permissions, for example).
`reason.transferred` tells how much of the output array is valid after failure.
Do not treat a partial read as the whole file.

## Whole-file convenience operations

`read_file(path, output, workspace)` opens a fresh cursor and closes it before
returning. `ReadReport.read` is the retained prefix and `eof` distinguishes a
complete file from capacity exhaustion. An exact fit uses a discarded one-byte
EOF probe. This is not a snapshot against concurrent writers.
`File.open_utf8(path, options, workspace)` gives explicit streaming ownership.

`write_file(path, bytes, workspace)` creates or **truncates** the destination,
follows symlinks, and leaves partial/truncated data on failure. It does not sync
or atomically replace. For exclusive creation use `OpenOptions.create_new()`
with `File.open_utf8` and `io.write_all`; for durability handle `File.sync`.
`FileError` preserves native causes and transferred-byte counts.

## API and contracts

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
The Windows fixtures run both natively in Windows CI and under Wine, including
sharing denial, rename/delete with open handles, non-inheritable file handles,
repeated close, and strict/lossy unpaired-surrogate conversion. Rejection tests
cover escaping native paths, mutable aliases, iterator name lifetimes, ignored
Results, and private resource construction.

## Whole-file boundaries and errors

`read_file` reports `io.ReadReport { read, eof }`. The returned byte count always
fits the destination. `eof=false` means more data was observed: do not treat the
retained prefix as a whole file. A one-byte probe distinguishes an exact fit
from exhaustion, including a zero-byte buffer and empty file. The probe byte is
discarded; no unbounded allocation or metadata-size guess is used. Reads retry
interruptions and may block, including on special files and during the probe.
There is no file-operation timeout. `io.read_bounded` exposes this same portable
algorithm for structural readers without importing any hosted module.

On errors, only `output[..reason.transferred]` is retained output. Open/path
conversion failures transfer zero bytes and leave output unchanged. Close errors
preserve the full read count. The first operation error wins over cleanup errors;
owners still attempt deterministic cleanup. File mutation during the read is
observable; completion means observed EOF, not a consistent filesystem snapshot.

`write_file` returns only after writing every input byte and successfully closing.
It creates a missing file and truncates an existing one before writing, even for
empty input. Unix creation mode is 0666 filtered by umask; Windows sharing allows
read/write/delete. It follows symlinks. It does not create parent directories,
replace atomically, or request durable storage. On failure, a created/truncated
file and its written prefix remain; `transferred` counts accepted bytes, including
all bytes if only close failed. Use `File.open_utf8` with explicit options for
exclusive creation, append, or an explicit `sync` request.

The complete [file example](https://github.com/Jotrorox/dodo/blob/main/examples/hosted_files.dodo)
creates/truncates `hosted-example.txt`, reads it through a 64-byte buffer, and
prints it. Run it in a disposable directory to keep its output isolated.
