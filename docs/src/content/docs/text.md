---
title: "UTF-8 text and strings"
description: "UTF-8 text and strings: a runnable starting point, storage choices, and detailed contracts."
section: "Standard library"
order: 146
---

Use `std/text.Builder.new` with a byte array to build a short string.
`Text.from_str` wraps a known-valid string literal without copying; `Text.new`
validates bytes received from a file or network. Both and the fixed builder are
portable and require no allocator.

## Quickstart

Save this as `text_start.dodo`:

```dodo test
package text_start
import "std/text"

fn greeting() -> void!text.Error {
    storage := [0u8; 32]
    builder := text.Builder.new(&mut storage)
    hello := text.Text.from_str("Hello, ")
    name := text.Text.from_str("Dodo!")
    builder.append(&hello)?
    builder.append(&name)?
    result := builder.as_text()
    assert_eq(result.as_str(), "Hello, Dodo!")
    return ok()
}
fn main() -> i32 {
    match greeting() {
        ok() => { return 0 },
        err(_) => { return 1 },
    }
}
```

```sh
dodo run text_start.dodo
```

Expected output: none; exit 0 confirms the built string is `Hello, Dodo!`.
Use the [console writer](formatting.md) to display it. The 32-byte array sets
the maximum UTF-8 byte length. Exit 1 means an append failed; increase capacity
or reject oversized input. An unsuccessful append leaves existing text intact.
For `Text.new` failures, reject malformed UTF-8 or explicitly choose replacement
decoding; do not reinterpret arbitrary bytes as valid text.

Start with this fixed builder. When text must grow, start with `std/text_alloc.from_str(&mut arena, value, limit)`
using an `alloc/arena` and an explicit byte limit. The separately
imported `std/text_unicode` adds Unicode whitespace trimming. It does not change
the ASCII-only behavior of `Text.trim_ascii` or provide general normalization.

## Portable text and owned UTF-8

`std/text` imports `core/ascii`, `core/bytes`, `core/mem`, and portable `std/io` error support; it allocates
nothing and requires no locale, OS, allocator, scheduler, or initialization.
`std/text_alloc` is an independent import for explicitly allocated strings. Its
buffer obtains storage through the caller's allocator, with an explicit maximum
capacity and recoverable allocation errors.

### Bytes, scalars, and graphemes

A byte is an octet. A Unicode scalar is a code point in U+0000..U+10FFFF,
excluding U+D800..U+DFFF. A grapheme cluster can contain several scalars: for
example, a letter and a combining accent. `Text.len_bytes()` counts bytes;
`Text.len_scalars()` counts scalars. Neither measures displayed characters,
terminal columns, or grapheme clusters.

The scalar and UTF-8 definitions follow Unicode **16.0.0**. Validation requires
no Unicode tables. This package deliberately provides ASCII trimming and exact
UTF-8 searching; it does not silently apply normalization, locale-sensitive
matching, Unicode whitespace, case folding, or grapheme segmentation. The
independent `std/text_unicode` import adds the Unicode 16.0.0 White_Space
property and trimming; see the supplement below. Larger property tables and
advanced algorithms such as normalization or grapheme segmentation are not
implemented.

### Validation, decoding, and encoding

- `validate(bytes) -> void!Error` rejects overlong encodings, isolated continuation
  bytes, invalid leads, surrogates, values above U+10FFFF, and truncated input.
- `decode(bytes, offset) -> Decoded!Error` decodes one scalar and returns public
  `scalar` and `next` fields. `offset >= bytes.len` is `OutOfBounds`; EOF is
  represented by the iterator's `none`, not a made-up scalar.
- `decode_replacement(bytes, offset) -> Option<Decoded>` explicitly replaces
  each malformed **byte** with U+FFFD and advances by one. A following valid
  sequence is still decoded normally. At or past EOF it returns `none`. This
  policy is deterministic, but is not Unicode maximal-subpart replacement.
- `is_scalar(value)` checks scalar validity. `encoded_len(scalar)` and
  `encode(scalar, destination)` return one through four bytes. Invalid scalars
  and insufficient capacity leave the destination completely unchanged.

`Error` contains public `kind: ErrorKind` and `position: usize`. Positions are
byte offsets, except that an invalid scalar's position is zero and an invalid
scalar-index lookup reports that index. Invalid continuation and scalar-range
restrictions report the offending byte; truncation reports `bytes.len`, where
a required byte is missing. `BufferFull` reports the available destination
length for encoding, or the builder's used length for append. Strict operations
never silently replace invalid input.

### Borrowed text

`Text.new(bytes) -> Text!Error from(bytes)` validates a checked immutable byte
slice. `Text.from_str(value)` takes a language string without revalidation.
`as_bytes()` and `as_str()` expose shared views retaining the original storage
dependency. The private data field prevents safe code from inventing unvalidated
text. Mutating or destroying the source while a view is subsequently used is a
compile-time error.

`slice(start, end)` uses exclusive byte endpoints and rejects reversed,
out-of-bounds, or non-scalar-boundary ranges. Empty ranges at valid boundaries
succeed. `is_boundary`, `floor_boundary`, and `ceil_boundary` make byte alignment
explicit; the rounding methods reject positions past the end. `byte_offset`
maps a scalar index to a byte boundary, including one-past-the-end;
`slice_scalars` slices by scalar indices. These operations do not split scalars,
but can split a grapheme cluster. Scalar indexing is linear in the byte length.

`scalars()` returns a stateful `Scalars`; `next()` returns `Option<Scalar>` with
public `value`, `start`, and `end` fields. Repeated calls after EOF return `none`.
No closures, traits, or scheduler are required.

`equal`, `starts_with`, `ends_with`, and `find` take another validated `Text`.
Comparison is exact byte comparison; `find` returns a byte offset, and an empty
needle matches at zero. `trim_ascii` removes ASCII space and bytes 9 through 13
at both ends. `split(delimiter)` rejects empty delimiters and returns a `Split`
whose `next()` preserves leading, adjacent, and trailing empty fields. Both
source and delimiter remain borrowed. Each returned field borrows the splitter,
so finish using it before advancing the splitter again.

### Fixed and allocated builders

`Builder.new(&mut storage)` retains an exclusive checked borrow of caller-owned
bytes. It starts empty, and `capacity()` never changes. `append(&Text)` and
`push(scalar)` validate capacity before writing; failure leaves length and
contents unchanged. `clear()` changes the logical length to zero; it does not
zero old bytes. `as_text()` exposes the initialized prefix. A live view prevents
mutating, moving, or dropping the builder until its last use.

`text_alloc.String<A>.new(buffer)` takes ownership of a
`bytes_alloc.Buffer<A>`, validates its initialized contents, and releases the
buffer on validation failure. Its operations are `len_bytes`, `capacity`,
`limit`, `as_text`, `reserve(additional)`, `append(&Text)`, `append_str(&str)`,
`push(scalar)`, `truncate(byte_length)`, and `clear`. Truncation rejects lengths
past the current end and positions inside a scalar. Append and reservation
errors preserve existing contents. `push` returns `text_alloc.Error`, which
separates scalar errors from allocator errors; allocation-only methods return
`alloc/error.AllocError` directly.

For routine construction with an exclusive arena, use
`text_alloc.empty(&mut arena, limit)?`, `text_alloc.from_str(&mut arena, utf8,
limit)?`, or `text_alloc.from_text(&mut arena, &text, limit)?` directly. The
buffer-taking constructor also accepts buffers from `std/arena_bytes` and
`std/pool_bytes`. The string owns its buffer and keeps its allocator exclusively
borrowed. Growth can move
storage and temporarily needs both allocations alive; arena deallocation does
not reclaim individual old blocks. Shared text views prevent growth at compile
time until their last use. Raw pointers become invalid when storage moves.
Dropping the string releases its allocation exactly once; there is no hidden
global allocator fallback.

### Explicit numeric parsing

`parse_u64(bytes, radix)` and `parse_i64(bytes, radix)` consume the entire input.
Radices 2 through 36 accept ASCII digits and case-insensitive ASCII letters.
An optional leading `+` is supported; only the signed parser accepts `-`.
Whitespace, prefixes such as `0x`, digit separators, and trailing characters
are rejected. `InvalidRadix`, `EmptyNumber`, `InvalidDigit`, and `Overflow` are
recoverable errors. Overflow identifies the first digit that cannot fit.
Both signed boundaries, including -9223372036854775808, are handled exactly.

`parse_u32_decimal(bytes)` is a strict decimal helper for fixed-width protocol
and calendar fields. It consumes the entire input, accepts only ASCII `0`–`9`,
and rejects signs, whitespace, and separators. Its `ParseError` distinguishes
`Empty`, `InvalidDigit`, and `Overflow`; leading zeroes are permitted. It reuses
the integer parser with the `u32` range limit. The [time parser](time.md#parsing-and-formatting)
uses it after validating field positions and punctuation.

`parse_f64(bytes)` accepts finite decimal syntax:
`[+-]?(digits(.digits?)?|.digits)([eE][+-]?digits)?`.
It rejects whitespace, hexadecimal notation, NaN, infinity, separators, and
partial input. Decimal overflow is `Overflow`; underflow rounds to a subnormal
or signed zero. Negative zero is preserved. Conversion rounds the exact decimal
rational to binary64 using round-to-nearest, ties-to-even, including subnormal
values and halfway cases. This is a portable Dodo fixed-big-integer algorithm;
it does not call libc, depend on the floating-point locale, or accumulate an
approximate decimal significand in a float.

`MAX_FLOAT_DIGITS` is **768**: all mantissa digits, including leading and trailing
zeros, count. A longer mantissa returns `TooManyDigits` at the first excess
byte. Exponent text can be arbitrarily long; its magnitude is internally
saturated only after classification becomes unambiguous. Conversion uses fixed
4096-bit integer arrays on the stack and can require several kilobytes of stack
space. This favors simple, auditable correctness over specialized fast-path
performance. It is suitable for freestanding builds, but small embedded stacks
must budget for the parser. There is currently no direct `f32`, grapheme,
normalization, Unicode case conversion, or arbitrary-precision numeric API.

### Verification

`tests/stdlib/std_text.dodo` and `std_text_alloc.dodo` execute at `-O0` and `-O3`
and compile to WebAssembly and Cortex-M0 objects. They exercise scalar limits,
malformed UTF-8 byte positions, replacement policy, encoding exhaustion, text
boundaries/splitting, transactional builder failure, integer extremes, precise
floating-point ties/subnormals, growing allocations, failure preservation, and
allocation cleanup. `tests/std_text.rs` also generates 263 decimal/reference
comparisons (including values near subnormal and finite limits), executes them
at both optimization levels, and rejects nine invalid borrow/Result programs.


### Optional Unicode whitespace supplement

`std/text_unicode` is independently imported; `std/text` does not depend on it.
`UNICODE_VERSION` is `"16.0.0"`. `is_whitespace(scalar)` implements all 25
scalars in the `White_Space` property from the primary
[Unicode 16.0.0 PropList](https://www.unicode.org/Public/16.0.0/ucd/PropList.txt).
Invalid scalars return false. The data is distributed under Unicode License V3,
with the full copyright and permission notice in `stdlib/std/LICENSE.unicode`.

`trim(input: &text.Text) -> text.Text from(input)` removes Unicode whitespace
scalars at both ends, keeps interior bytes exactly, and preserves the source
borrow. Empty or entirely whitespace input produces an empty view. It scans
the input once using fixed stack space, allocates nothing, and cannot fail for
validated `Text`. Unlike ASCII trimming, this includes U+0085, U+00A0, U+1680,
U+2000..U+200A, U+2028, U+2029, U+202F, U+205F, and U+3000. U+180E, U+200B, and
U+FEFF are not stripped. This property does not promise grapheme boundaries,
normalization, locale behavior, or display-width handling.

`tests/stdlib/std_text_unicode.dodo` checks the property over all Unicode code
points, tests all 25 whitespace values, and exercises trimming with multibyte
scalars and combining marks. It executes at `-O0`/`-O3` and emits WebAssembly
and Cortex-M0 objects. Additional rejection cases ensure trimmed views cannot
escape source storage or survive invalidating mutation.


## Shared owned strings

For several strings or containers sharing one arena, use
`std/text_shared.from_str(arena.handle(), value, limit)` with an
`alloc/shared_arena.SharedArena`. It returns an owned UTF-8 string retaining the
shared allocator capability. `new` constructs an empty string and `from_text`
copies a validated view. `append_str`, `append`, `push`, `reserve`, `truncate`, `clear`,
`as_str` and `as_text` preserve UTF-8 validity. The byte limit bounds logical
length; vector capacity can reserve extra bytes. Arena growth retains old bump
allocations until reset, so plan total arena space as well as string limits.

`text_shared.Key` supplies lexicographic byte ordering and deterministic FNV
hashing for owned string keys. Use it for trusted keys; it does not acquire an
unpredictable hash seed. Reference/Result element acceptance still depends on
the [current container rules](container-elements.md); an owned string constructor
does not waive a container's element restrictions.
