---
title: "Formatting and console output"
description: "Formatting and console output: a runnable starting point, storage choices, and detailed contracts."
section: "Standard library"
order: 145
---

Use `std/console.println_value` with `std/fmt.signed` to print an integer.
`console` selects the hosted standard output stream; `fmt` supplies value
formatting. No format-string syntax, custom writer, or allocation is needed.
Console access supports Linux GNU x86-64 and Windows x64.

## Quickstart

Save this as `print_value.dodo`:

```dodo test
package print_value
import "std/console"
import "std/fmt"

fn main() -> i32 {
    value := fmt.signed(42)
    match console.println_value(&value) {
        ok(_) => { return 0 },
        err(_) => { return 1 },
    }
}
```

```sh
dodo run print_value.dodo
```

Expected stdout:

```text
42
```

Success exits 0; exit 1 means the output write failed. Check the destination
(e.g. a closed pipe). A failure may have written a prefix, recorded in
`io.Error.transferred`, so do not blindly replay the whole message.
For plain strings start with `console.println("Hello, Dodo!")` and handle its
Result the same way. `fmt.unsigned`, `boolean`, `floating`, and `codepoint` wrap
other common values. Floating defaults use six fractional scientific digits;
choose `Formatter.floating` for explicit precision.

`std/fmt` is portable; `std/console` is hosted. For a portable destination, use
`fmt.println` or `fmt.value_line` with `io.MemoryWriter.new(&mut storage)`.
Start with a fixed byte array. Only choose `std/fmt_alloc` if the result must be
an owned string; give it a buffer from [safe byte-buffer constructors](bytes.md).
`std/float_decimal` is an internal conversion helper, not an application API.

## Console streams

`console.stdin()`, `stdout()`, and `stderr()` return unbuffered borrowed wrappers.
They do not close the process stream on drop, allocate, or normalize newlines.
Reads and writes may block. Independent wrappers can interleave output; a
sequence of formatting calls is not an atomic log record. Foreign closure or
replacement of a standard handle is observed as normal I/O failure.
`Input.read_line` uses the bounded [line-reader behavior](io.md#bounded-and-line-oriented-input).
Output also provides `print`, `println`, `print_value`, and `println_value`.
Explicitly buffer with `io.BufferedWriter` when needed and handle `flush`.

## Byte formatting

`std/fmt` writes to any `std/io` writer using ordinary generic methods. Create
`fmt.Formatter.new::<WriterType>(&mut writer)`, then call `string`,
`padded_string`, `boolean`, `codepoint`, `unsigned`, `signed`, or `floating`.
Every method returns `void!io.Error`. `written()` counts bytes emitted by that
formatter. Failure includes all prior successful output and the failing write's
partial progress in `Error.transferred`; bytes already written remain visible.
An empty string succeeds without calling the sink. Formatting never flushes.
A `MemoryWriter` supplies caller-provided fixed storage; it reports `BufferFull`
with the accepted prefix when exhausted.

`fmt.defaults()` creates options with decimal radix, no minimum width, ASCII
space fill, right alignment, minus signs only, lowercase digits, and no zero
padding. Public options select radix 2 through 36, uppercase digits, minimum
width, `Alignment.Left`/`Right`/`Center`, and `Sign.NegativeOnly`/`Always`/`Space`.
Center alignment places an odd extra padding byte on the right. Numeric
`zero_pad` emits the sign first, then zeroes, then digits; it takes precedence
over alignment. Width measures **bytes**, not Unicode scalars, grapheme clusters,
or terminal columns. `fill` must be ASCII. Padding never truncates content.
Integer functions support the complete `u64`/`i64` range, including `i64`'s
minimum; smaller types can be explicitly widened. Radix prefixes are explicit
strings supplied by the caller. Code points must be Unicode scalar values and
are encoded using `std/text`; invalid scalar values fail before output.

`floating(value, precision, style, &options)` formats binary64 with mandatory
precision from 0 through 324. `FloatStyle.Fixed` uses that many digits after the
decimal point. `Scientific` uses one leading digit, the specified fraction, and
an exponent with a sign and at least two digits. Rounding is nearest, ties to
even, including subnormal values and cases that carry into the next exponent.
Precision zero omits the decimal point. Negative zero remains negative.
Infinity prints `inf`/`-inf`; NaN payloads and NaN signs are canonicalized to
`nan`. The uppercase option changes those spellings and the exponent marker.
A positive-sign option also applies to NaN and infinity. Binary32 values can be
exactly widened to binary64; formatting then rounds that exact value. There is
no shortest-roundtrip, `%g`, hexadecimal-float, locale, or dynamic format-string
API in this release.

Floating conversion decodes the binary64 significand and exponent and constructs
an exact decimal integer using powers of two or the identity
`m / 2^k = (m * 5^k) / 10^k`. This independent implementation uses integer
arithmetic to round the coefficient once, with ties to even, regardless of the
floating-point rounding mode. No C code or libc is linked. The helper uses a
768-byte decimal digit array and a 640-byte output buffer, plus ordinary scalar
locals and call frames; integer and string formatting do not use this floating
scratch. The algorithm favors correctness and bounded storage over
shortest-output speed. All target memory and soft-float helper requirements are
those of the normal compiler backend.

Customization is static and explicit. A public value type defines
`pub fn format<W>(&self, output: &mut fmt.Formatter<W>) -> void!io.Error`;
`fmt.value(&mut sink, &value)` specializes that method and returns the written
byte count. The method may compose all standard formatting operations and
propagate failures with `?`. It requires no traits, closures, reflection,
vtables, scheduler, or allocation. See `examples/formatting.dodo`.

`write_u32_padded(destination, value, width)` is an allocation-free convenience
for decimal output into a caller-owned byte slice, built on the same formatter
and memory-writer contracts. It writes at least `width` digits, adding leading
zeroes without truncating the value, and returns the written byte count. It
leaves the complete destination unchanged if it returns
`FormatError.BufferTooSmall`, and preserves all bytes beyond successful output.
No NUL terminator is added. [Time formatting](time.md#parsing-and-formatting)
uses this helper for fixed-width calendar fields.

`std/fmt_alloc` is imported independently. `fmt_alloc.string(buffer, &value)`
consumes a `std/bytes_alloc.Buffer`, clears its old contents, formats with the
buffer's explicit allocator and growth limit, and returns a validated
`text_alloc.String`. Use safe `arena_bytes`/`pool_bytes` constructors for the
buffer. `Error` distinguishes allocation, formatting, and UTF-8 validation;
custom formatters emitting malformed byte sequences cannot construct an invalid
string. Failure releases owned buffer storage deterministically. Formatter
errors preserve the byte count before failure, even though that failed buffer
is then destroyed. `fmt_alloc.allocate` also accepts a custom allocator, initial
capacity, and limit directly; it is unsafe because a generic allocator must
honor the allocation contract. Existing-sink formatting never imports these
allocation adapters.



## Console convenience and printable errors

See [console I/O](console.md) for the complete hosted API, formatting defaults,
line-reading results, and stream ownership. `fmt.print` and `fmt.println` take
any structural writer and a string; `fmt.value_line` adds LF to `fmt.value`.
Each helper reports cumulative progress for its whole call, including a failing
newline write. `Formatter.decimal(i64)` and `natural(u64)` use default decimal
formatting and support structural library error formatters without an import cycle.

I/O, native platform, text, network, TLS, and HTTP errors implement `format`
directly. Optional `std/fmt/errors` supplies displays for enum errors with
`allocation`, `range`, `parse`, `format`, `synchronization`, `web`, `math`, and
`time`. Every display works with `fmt.value`, `fmt.value_line`, and console value
printing. Built-in error output includes fixed kinds and numeric metadata only.
