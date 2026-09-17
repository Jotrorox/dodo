---
title: "Console I/O and printing"
description: "Print text and values, report errors, and read bounded lines using safe hosted standard streams."
section: "Standard library"
order: 143.5
---

`std/console` reads and writes the process's three standard streams: stdin for
input, stdout for normal output, and stderr for diagnostics. Start with
`println` for a line of text, `printf` for a formatted message, and `read_line`
for bounded input. Each operation can fail, so every Result must be handled.

Console programs run on Linux GNU x86-64 and Windows x64 MSVC/GNU with a hosted
C toolchain. They need no global allocator or application unsafe code. For
memory buffers and custom devices, use the same reader/writer contracts through
[I/O](io.md) and [formatting](formatting.md).

## Print a greeting

```dodo test
package hello
import "std/console"

fn main() {
    console.println("Hello, world!")!
}
```

Save this as `main.dodo` and run `dodo run`. It prints `Hello, world!` followed
by LF and returns exit status zero. Printing returns a Result; postfix `!`
unwraps success and panics on failure. Fallible helper functions can propagate
errors with `?`, or use `match` to recover locally.

The string is `&str`, a borrowed UTF-8 view. Printing does not consume or change
its contents. The returned `usize` is the number of bytes written, not the
number of displayed characters. `println("é")` writes three bytes: two for
the character and one LF. Use a byte writer when the input is arbitrary bytes.

## Choose an output operation

| You have | Use | Example |
| --- | --- | --- |
| A single value | `print` or `println` | `console.println(42)!` |
| Several values in a sentence | `printf` | `console.printf("{} of {}\n", 2, 5)!` |
| A diagnostic | A stderr wrapper | `console.stderr().println("missing input")!` |
| Raw bytes | `io.write_all` on an output wrapper | See the [file reader](filesystem.md#quickstart). |
| A custom destination | `fmt.print` / `fmt.printf` | See [custom writers](io.md). |

`!` is useful for small demonstrations where failure should stop the program.
In a command-line application, match a write failure or propagate it from a
fallible helper. Writing an error message to stderr can fail too.

## Public API

| Operation | Result and behavior |
| --- | --- |
| `console.stdin()` | `console.Input`, a borrowed standard input wrapper. |
| `console.stdout()`, `console.stderr()` | `console.Output`, a borrowed standard output/error wrapper. |
| `console.print(value)` | `usize!io.Error`, automatic primitive or custom formatting to stdout. |
| `console.println(value)` | Same, followed by one LF, even for empty text. |
| `console.printf("literal", args...)` | Compile-time checked placeholders and heterogeneous arguments. No implicit LF. |
| `Output.print`, `println`, `printf` | The same operations on the selected output, including stderr. |
| `Input.read(&mut[u8])` | `usize!io.Error`, the structural reader contract. |
| `Output.write(&[u8])` | `usize!io.Error`, the structural writer contract; short writes are allowed. |
| `Input.read_line(&mut[u8])` | `io.Line!io.Error`, bounded line input. |
| `io.read_line(&mut reader, &mut[u8])` | The same portable algorithm for any structural reader. |
| `fmt.print(&mut writer, value)`, `fmt.println(&mut writer, value)` | The same operations for any structural writer. |
| `fmt.printf(&mut writer, "literal", args...)` | The same checked formatting for any structural writer. |

Printing completes short writes and retries interruptions. On failure,
`Error.transferred` counts all bytes emitted by that call, including partial
progress and any newline byte. Other errors, including `WouldBlock` and
`BrokenPipe`, return to the caller. Previously emitted bytes remain visible;
replaying the entire message can duplicate them. An empty `print` touches no
stream. Output is unbuffered, so prompts do not require a flush.

## Print values

Pass values directly. No primitive wrappers or widening casts are needed:

```dodo test
package printing
import "std/console"

fn main() {
    console.println(42)!
    console.printf("Answer: {}, enabled: {}\n", 42, true)!
    console.stderr().printf("Status: {}\n", "ready")!
}
```

| Value | Default rendering |
| --- | --- |
| All signed and unsigned integers | Decimal integer; a minus sign for negatives. |
| `bool` | `true` or `false`. |
| `&str` | UTF-8 bytes, unchanged. |
| `f32`, `f64` | Scientific notation with six fraction digits, such as `1.250000e+00`. |
| `fmt.codepoint(u32)` or `printf("{:c}", scalar)` | One UTF-8 scalar; invalid scalars return `InvalidInput`. |

Integer formatting supports the full 64-bit ranges. Floats round to nearest,
ties to even; negative zero retains its sign, infinities print `inf`/`-inf`,
and NaNs print `nan` without payloads. References to printable values also work.
Custom values implement
`pub fn format<W>(&self, output: &mut fmt.Formatter<W>) -> void!io.Error`.
Passing a custom value place borrows it; passing a temporary formats and then
drops it. The borrow checker protects both arguments and the destination.

`print` and `println` do not interpret braces in strings. `printf` requires a
literal: `{}` consumes the next argument; `{{` and `}}` emit literal braces.
The compiler checks syntax, argument count, and each argument's formatting
options. See [formatting options and evaluation](formatting.md#checked-format-strings).

Run `dodo run examples/console_formatting.dodo` from the source checkout to see
text, an integer, a boolean, and a floating value.

## Report an error

Use `errors := console.stderr()` and `errors.println(&reason)` to format
I/O, platform, text, network, TLS, or HTTP errors directly. For example, a closed
stream may print `io: Closed code=9 transferred=0`. The native platform domain
is errno on Linux and GetLastError on Windows; a library-generated code is zero.
Codes are retained numerically, without native message lookups.

Enum errors cannot have methods in the current language. Import the optional
`std/fmt/errors` package and use `errors.allocation(reason)`, `range`, `parse`,
`format`, `synchronization`, `web`, `math`, or `time` to obtain a value implementing
the same formatting contract. For example, `errors.allocation` accepts an
`alloc/error.AllocError` and formats `allocation: Exhausted` when allocation fails.

Built-in error displays contain fixed kind names, codes, byte counts, and
positions. They do not capture credentials, request bodies, paths, environment
contents, source input, or native error-message strings. A custom formatter
controls its own output. Writing an error can itself fail and returns an ordinary
I/O Result. Run `dodo run examples/console_error.dodo` for a complete stderr example.

## Read a bounded line

The following complete program reads one line and writes exactly those bytes
back. Save it as `echo_line.dodo` and run `dodo run echo_line.dodo`, then type a
line and press Enter. It deliberately uses byte I/O, so it also works for
redirected input that is not UTF-8.

```dodo
package echo_line
import "std/console"
import "std/io"

fn main() -> i32 {
    input := console.stdin()
    storage := [0u8; 128]
    line := match input.read_line(&mut storage) {
        ok(value) => { value },
        err(_) => { return 1 },
    }
    output := console.stdout()
    match io.write_all(&mut output, &storage[..line.count]) {
        ok(_) => {},
        err(_) => { return 2 },
    }
    if line.end == io.LineEnd.Full { return 3 }
    return 0
}
```

Exit 3 means the buffer contains only the first piece of a longer line; the
program has not read the remainder. To build a line-processing loop, handle
`Full` by processing that piece and reading again, `Newline` by ending the
current line, and `Eof` by processing any final bytes before stopping.

Allocate a fixed array, obtain `input := console.stdin()`, then call
`input.read_line(&mut storage)`. `io.Line` contains `count: usize` and
`end: io.LineEnd`; the bytes are `storage[..line.count]`.

| `LineEnd` | Meaning |
| --- | --- |
| `Newline` | The prefix ends in LF. LF and any preceding CR are included in `count`. |
| `Eof` | End of input was observed. Zero count means no line; a nonzero count is a final line without LF. |
| `Full` | Storage filled without LF. EOF has not been probed; the next byte remains unread. |

Empty storage returns `Full` with count zero without touching the reader. A line
whose last byte exactly fills the buffer returns `Newline` if that byte is LF;
otherwise it returns `Full`, even if the next read would be EOF. Continue with
more storage to process a long line in pieces. Stopping after `Full` leaves the
remainder available. Nothing is silently discarded or read beyond LF.

Read failures return the initialized prefix length in `Error.transferred`, even
if the prefix ends in LF. An interruption retries after its reported progress;
other failures are recoverable. Reads preserve bytes, including NUL and invalid
UTF-8. Use `text.Text.new(&storage[..count])` to validate a text view, and remove
LF and an optional preceding CR explicitly when desired. That view borrows the
storage, so it must finish before the storage is reused.

`dodo run examples/console_read_line.dodo` prompts for a name, reads into 128
bytes, handles EOF and exhaustion, validates UTF-8, and prints a greeting.
The portable algorithm requests one byte at a time to avoid hidden read-ahead;
`io.BufferedReader` can provide explicit caller-backed buffering when needed.

## Standard-stream ownership and encoding

Dropping `Input`, `Output`, or the underlying platform `BorrowedHandle` performs
no close or flush. The process owns its standard streams. Wrappers have private
handles and expose no close operation; ordinary owning platform handles and files
still close exactly once. Errors from absent or externally closed standard
streams are reported during I/O. The platform adapter also exposes `stdin`,
`stdout`, and `stderr` returning `BorrowedHandle` for low-level composition.

Calls may block. Multiple wrappers share the process streams, without a global
lock or a promise that whole formatted messages are atomic. `fmt.Formatter` and
buffered I/O adapters exclusively borrow the wrapper while active. Borrowing one
wrapper does not reserve every other wrapper for that process stream.

Bytes and LF pass through unchanged on both targets. Redirection to files and
pipes preserves UTF-8 bytes. [Windows terminal `ReadFile`/`WriteFile` operations](https://learn.microsoft.com/en-us/windows/console/console-code-pages)
use the terminal's configured code pages; configure UTF-8 input/output code pages
for non-ASCII terminal text. The library does not alter process-wide terminal
modes or code pages. Windows redirected input is covered separately from terminal
rendering; terminal rendering is not established by cross-compilation checks.

## Complete API reference

For every public type, field, constant, and function signature, see [std/console](api/std/console.md), [std/io](api/std/io.md), [std/fmt](api/std/fmt.md), [std/fmt/errors](api/std/fmt/errors.md).
