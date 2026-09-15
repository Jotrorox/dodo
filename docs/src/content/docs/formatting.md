---
title: "Formatting and console output"
description: "Formatting and console output: a runnable starting point, storage choices, and detailed contracts."
section: "Standard library"
order: 145
---

Use `console.print(value)`, `console.println(value)`, and
`console.printf("literal", args...)` for everyday output. All return
`usize!io.Error`, including the written byte count. `console` supports Linux GNU
x86-64 and Windows x64; `fmt` works with any structural writer on all targets.

## Quickstart

```dodo test
package printing
import "std/console"

fn main() {
    console.printf("Answer: {}, enabled: {}\n", 42, true)!
}
```

Expected stdout is `Answer: 42, enabled: true` followed by LF. Postfix `!`
unwraps success and panics on failure. Use `?` to propagate errors or `match` to
recover. An error may have written a prefix, recorded in `io.Error.transferred`.

`print` renders one value; `println` adds one LF. Strings pass through unchanged,
including braces and NUL. Integers of every size render in decimal, booleans as
`true`/`false`, and `f32`/`f64` in scientific notation with six fraction digits.
References to these values also work. Custom values use the existing public
`format` method contract described below.

For a portable destination, use `fmt.print(&mut writer, value)`,
`fmt.println(&mut writer, value)`, or `fmt.printf(&mut writer, "literal", args...)`.
Start with `io.MemoryWriter.new(&mut storage)` over a fixed byte array. Choose
`std/fmt_alloc` when the result must be an owned string, supplying a buffer from
[safe byte-buffer constructors](bytes.md). `std/float_decimal` is an internal
conversion helper.

## Checked format strings

`printf` accepts a **string literal**, followed by zero or more heterogeneous
arguments. `{}` consumes the next argument. `{{` and `}}` emit literal braces;
for example, `printf("{{{}}}", 42)` emits `{42}`. Normal string escapes are decoded
first. An empty format emits nothing. LF is explicit: `printf` never adds it.
The compiler rejects malformed braces, missing or surplus arguments, unsupported
options, and types incompatible with their placeholders. Variables and constants
containing format strings are not accepted; print dynamic text with `print`.

The initial placeholder grammar is:

```text
{} or {:[[fill]align][sign][0][width][.precision][type]}
```

| Option | Meaning and compatible values |
| --- | --- |
| `align` | `<` left, `>` right, `^` center. All primitive displays. Default: right. |
| `fill` | One ASCII byte before alignment, excluding braces. Default: space. |
| `sign` | `+` always, space for positive, `-` negative only (default). Numbers only. |
| `0` | Zero padding after the sign. Numbers only; cannot combine with fill/alignment. |
| `width` | Decimal minimum byte width, 0 through 4294967295. Never truncates. |
| `.precision` | 0 through 324 fraction digits, floats only. Default: 6. |
| `d`, `b`, `o`, `x`, `X` | Integer decimal, binary, octal, lower/uppercase hexadecimal. No radix prefix. |
| `f`, `F`, `e`, `E` | Float fixed or scientific; uppercase also changes infinity/NaN spelling. |
| `s` | String display. |
| `c` | A `u32` Unicode scalar. Width/fill/alignment allowed; invalid scalars return `InvalidInput`. |

An omitted type uses automatic display. For a float, precision alone selects
fixed notation (`{:.2}`); otherwise automatic display uses scientific notation.
Booleans support width/fill/alignment with no type letter. Custom values accept
only `{}` (or the equivalent `{:}`), since their existing `format` method takes
no options. `fmt.codepoint(u32)` remains available for scalar display with
`print`/`println`. There are no named/numbered fields, argument reuse, dynamic
width/precision, debug displays, radix prefixes, or general variadic functions.

Examples: `{:08x}` produces eight lowercase hexadecimal digits; `{:+.2f}` adds
a sign and two decimal places; `{:*^8s}` centers text between asterisks.
Widths count **bytes**, including UTF-8, rather than terminal columns. Center
alignment puts an odd extra padding byte on the right.

### Evaluation, borrowing, and failures

The receiver/writer and arguments evaluate exactly once, left to right, before
any output. This includes arguments after a field whose write later fails.
A failure while evaluating an argument prevents all output from this call.
Custom value places are shared-borrowed automatically, so they remain usable
and cannot be mutated by later arguments while borrowed. Owned temporaries are
formatted and dropped normally, including on an I/O failure. Explicit references
retain normal checked borrowing. Output exclusively borrows the writer for the
call. A temporary `console.stdout()`/`stderr()` wrapper also supports methods.

The compiler expands each call into an ordinary function with a fixed typed
signature and one `Formatter`. All generated code passes through the normal
type, ownership, and borrow checks. There is no C variadic call, erased argument
array, runtime format parser, or heap allocation. The library's reserved
`@compiler(print)`, `@compiler(println)`, and `@compiler(printf)` prototypes
identify these entry points; they are not application extension attributes.
Printing entry points cannot be taken as callbacks; wrap a concrete call in an
ordinary function when a callback is needed.

Short writes complete and interruptions retry. Other I/O failures return
immediately; subsequent fields are not formatted. `Error.transferred` includes
all earlier fields, literal text, padding, and partial progress in the failing
write. No implicit flush, rollback, or allocation occurs. A custom formatter
must propagate the supplied formatter's errors to preserve its cumulative count.

## Console streams

`console.stdin()`, `stdout()`, and `stderr()` return unbuffered borrowed wrappers.
They do not close the process stream on drop, allocate, or normalize newlines.
Reads and writes may block. Independent wrappers can interleave output; a
sequence of formatting calls is not an atomic log record. Foreign closure or
replacement of a standard handle is observed as normal I/O failure.
`Input.read_line` uses the bounded [line-reader behavior](io.md#bounded-and-line-oriented-input).
Output also provides `print`, `println`, and `printf`.
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
`fmt.print(&mut sink, &value)` specializes that method and returns the written
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
any structural writer and a printable value; `fmt.printf` checks literal formats.
Each helper reports cumulative progress for its whole call, including a failing
newline write, completes short writes, and retries interruptions.
`Formatter.decimal(i64)` and `natural(u64)` use default decimal
formatting and support structural library error formatters without an import cycle.

I/O, native platform, text, network, TLS, and HTTP errors implement `format`
directly. Optional `std/fmt/errors` supplies displays for enum errors with
`allocation`, `range`, `parse`, `format`, `synchronization`, `web`, `math`, and
`time`. Every display works with `fmt.print`, `fmt.println`, `fmt.printf`, and console
printing. Built-in error output includes fixed kinds and numeric metadata only.
It does not capture sensitive input or native error-message strings.
