---
title: "Compression"
description: "Encode and decode DEFLATE, zlib, and gzip, and decode Brotli, with caller-supplied buffers."
section: "Standard library"
order: 144.5
---

The portable compression packages read and write caller-supplied byte slices
and never allocate. Raw DEFLATE (RFC 1951), zlib (RFC 1950), and gzip
(RFC 1952) share one compressed format with different framing; import the
package matching the data on the wire:

| Package | Framing | Check value |
| --- | --- | --- |
| `std/compress/deflate` | none | none |
| `std/compress/zlib` | 2-byte header | Adler-32 |
| `std/compress/gzip` | 10-byte or longer header | CRC-32 and size |
| `std/compress/io` | `std/io` Reader and Writer adapters for all three | |

All three packages use `deflate.Error`, `deflate.Status`, and
`deflate.Progress`, so code written for one framing works with the others.
[`std/compress/brotli`](#brotli) decodes the separate Brotli format.

## Complete buffers

`decode(input, output)` decodes one stream, or one gzip member, and reports
the input bytes consumed and output bytes written. Data after the stream is
left unconsumed; decode concatenated gzip members by calling again on the
remaining input. The output buffer limits expansion: a stream that needs more
space fails with `OutputFull`. Other errors are `Truncated`, `InvalidData`,
and, for zlib and gzip, `ChecksumMismatch`. The zlib decoder rejects streams
that need a preset dictionary with `UnsupportedDictionary` and enforces the
window size the header declares. The gzip decoder validates the optional
header fields and the header CRC. After an error the contents of the output
buffer are unspecified.

`encode(input, output, level)` compresses at level 0 through 9:
`deflate.DEFAULT_LEVEL` is 6, level 0 stores data uncompressed, and 9 searches
longest. Any other level fails with `InvalidArgument`. The encoder uses lazy
LZ77 matching and optimal length-limited Huffman codes, and compares stored,
fixed-code, and dynamic-code forms of each block, keeping the smallest. Output
is never larger than the stored form, so `max_encoded_len(input.len)` bytes
from the same package always suffice.

```dodo test
package main

import "std/compress/gzip"

fn main() {
    text := b"hello hello hello hello"
    packed := [0u8; 64]
    size := gzip.encode(text, &mut packed, 6)!
    restored := [0u8; 64]
    result := gzip.decode(&packed[..size], &mut restored)!
    core.assert_eq(result.output_written, text.len)
}
```

The match finder needs `deflate.WORKSPACE_LEN` `u32` words (256 KiB) of
workspace. `encode` places it on the stack; `encode_with_workspace` takes a
caller-owned `&mut [u32]`, which can live in static storage and be reused for
any number of calls.

## Gzip metadata

`gzip.read_header(input)` parses the header at the start of a member and
returns its `Header` with the header's `length`. The fields are the
modification time `mtime` (seconds since 1970, 0 when unknown), the writing
operating system `os` (3 for Unix, 255 unknown), the `text` flag, and the
`extra`, `name`, and `comment` byte strings, which borrow from the input and
are empty when absent. `Truncated` means the header continues past the
input, so incremental readers can call it on a growing prefix.

`gzip.encode_with_header(input, output, level, header, workspace)` writes a
member with that metadata, given as a `gzip.Header` literal; use 0, 255,
and empty byte strings for unknown fields. A name or comment containing a zero
byte, or an extra field over 65,535 bytes, fails with `InvalidArgument`. The
header needs `gzip.header_len(header)` bytes, of which `max_encoded_len`
already counts 10. Other gzip encoders, including the incremental one, write
a minimal header with no metadata.

## Incremental processing

`Decoder` in each package decodes a stream through buffers of any size. Call
`process(input, output)` repeatedly, advancing by the returned
`input_consumed` and `output_written` counts. `NeedInput` asks for more input,
`NeedOutput` for more output space, and `Done` marks the end of the stream.
`NeedInput` at the end of the data means the stream is truncated. A decoder
never consumes bytes past its stream, and `reset()` prepares it for the next
one, such as another gzip member. Errors are final: further calls fail until
`reset()`. The decoder holds a 32 KiB history window.

`Encoder.new(level, buffer, workspace)` creates an incremental compressor. It
borrows a `buffer` of at least `deflate.ENCODER_BUFFER_LEN` bytes (97 KiB) and a
workspace of `deflate.WORKSPACE_LEN` words, so the encoder itself stays small. Call `process(input, output, finish)`, passing `finish` true once
`input` holds the last bytes; keep passing it, resubmitting unconsumed input,
until `Done`. The encoder buffers up to 64 KiB of input and emits whole
blocks, so output lags input until `finish`.

## Streams

`std/compress/io.Reader` decompresses from a `std/io` reader. It borrows the
source, a decoder, and a storage buffer for compressed bytes:

```dodo test
package main

import "std/compress/gzip"
import "std/compress/io" as cio
import "std/io"

fn main() {
    packed := [0u8; 64]
    size := gzip.encode(b"hello", &mut packed, 6)!
    source := io.MemoryReader.new(&packed[..size])
    decoder := gzip.Decoder.new()
    storage := [0u8; 512]
    reader := cio.Reader.new(&mut source, &mut decoder, &mut storage)!
    text := [0u8; 16]
    count := reader.read_checked(&mut text)!
    core.assert_eq(count, 5usize)
}
```

Bytes read past the end of the stream stay available through `buffered()`; a
one-byte storage never reads past it. `next_stream()` resets the decoder for
a following stream, such as the next member of a concatenated gzip file, and
returns false at the end of the source. `read_checked` keeps source failures
(`Io`, with the source error in `cause`), truncation (`Truncated`), and
malformed data (`Format`, with the `deflate.Error` in `format`) apart; its
`transferred` count includes output produced before the error. `read`
implements the ordinary `std/io` Reader contract, reporting format errors as
`InvalidInput` and truncation as `UnexpectedEof`. A source error that arrives
with data is reported after that data is decoded.

`std/compress/io.Writer` compresses everything written to it into one stream
on a `std/io` writer, using any of the three `Encoder` types. `write` accepts
all of its input unless the sink fails; the error then reports the input
accepted. Call `finish()` and handle its result to write the final block and
trailer. After a sink error the unwritten compressed bytes are kept, so
`finish()` can be retried; it reports the compressed bytes written by that
call.

## Brotli

`std/compress/brotli` decompresses Brotli (RFC 7932) streams, as used by
HTTP `Content-Encoding: br` and JPEG XL `brob` boxes. It decodes every
meta-block kind, block types and counts, all literal context modes,
distance codes and context maps, and the static dictionary with its 121
word transforms, for window sizes 10 to 24. Large-window Brotli, a
non-standard extension, fails with `Unsupported`. There is no encoder.

`decompress(input, output)` decodes one complete stream and returns its
length. The output buffer is also the sliding window, so nothing else is
needed except a 16 KiB stack workspace for the prefix code tables and
context maps (`STACK_WORKSPACE_LEN`, 8,192 16-bit words; about 30 KiB of
stack in all unoptimized, 23 KiB optimized). That covers the streams the
reference encoder writes for inputs of up to 128 KiB at any quality; a
stream needing more fails with `WorkspaceTooSmall`.
`decompress_with_workspace(input, output, workspace)` takes a caller-owned
`&mut [u16]` instead, from static storage or an allocator, and needs only
about 7 KiB of stack (14 KiB unoptimized).

```dodo test
package main

import "std/compress/brotli"

fn main() {
    // "hello hello hello", compressed by `brotli -q 11`.
    packed := [0x1fu8, 0x10, 0x00, 0xf8, 0x8d, 0x94, 0x6e, 0xde, 0x44, 0x55, 0x86, 0x96, 0x6c, 0x20, 0x6f, 0x01, 0x4f, 0x1c, 0x60, 0x1c]
    text := [0u8; 64]
    size := brotli.decompress(&packed, &mut text)!
    core.assert_eq(size, 17usize)
}
```

How much workspace a stream needs depends on the prefix codes each
meta-block declares, and the workspace is reused from one meta-block to the
next. `workspace_size(input)` returns the exact number of words by reading
the stream's headers and building their tables in `STACK_WORKSPACE_LEN`
words of stack, without producing output: under 6,000 for reference-encoder
streams of 64 KiB inputs, about 12,000 for 300 KiB and up to 80,000 for
several megabytes. It is exact whenever the last compressed meta-block is
the only one, or the earlier ones (such as the series that qualities 0 and 1
write) fit in its stack workspace and choose literal codes without context
modeling, which lets it read past them without the window. For other
streams it returns `WORKSPACE_LEN`, the bound for any valid stream:
696,108 words (1.3 MiB), for a meta-block with 256 literal, 256
insert-and-copy and 256 distance codes at their largest table sizes. Such a
stream is legal (`tests/compress/brotli/brotli_wide.py` writes one that
needs about 246,000 words), and any table-driven decoder needs at least one
word per code symbol, over 380,000, for the largest, so the bound stays. After decoding, or after a
`WorkspaceTooSmall` error, `Decoder.workspace_needed()` reports the exact
need of the meta-blocks read so far, including the one that did not fit.

```dodo test
package main

import "std/compress/brotli"

fn main() {
    // "hello hello hello", compressed by `brotli -q 11`.
    packed := [0x1fu8, 0x10, 0x00, 0xf8, 0x8d, 0x94, 0x6e, 0xde, 0x44, 0x55, 0x86, 0x96, 0x6c, 0x20, 0x6f, 0x01, 0x4f, 0x1c, 0x60, 0x1c]
    words := [0u16; 4096]
    need := brotli.workspace_size(&packed)!
    core.assert(need <= words.len)
    text := [0u8; 64]
    size := brotli.decompress_with_workspace(&packed, &mut text, &mut words[..need])!
    core.assert_eq(size, 17usize)
}
```

Errors are `Truncated`, `InvalidData`, `OutputFull` (the output is too
short), `WorkspaceTooSmall`, `WindowTooSmall` and `Unsupported`. Data after
the end of the stream is `InvalidData`, as with the `brotli` tool. Malformed
input never traps, and decoding time is bounded by the output size: a
command that would produce nothing (an empty transformed dictionary word) is
rejected.

To decompress without holding the whole result, `Decoder.new(input, window,
workspace)` decodes through a caller ring buffer of at least
`window_len(input)` bytes: the stream's sliding window (up to
`MAX_WINDOW_LEN`, 16 MiB), or less when a single meta-block is shorter.
`read(output)` copies the next bytes into a buffer of any size and returns 0
at the end of the stream. Unlike the DEFLATE decoders, `Decoder` needs the
whole compressed input up front. It ignores bytes after the stream, and
`input_consumed()` reports where the stream ended. Errors are final.
`decompressed_size(input, window, workspace)` decodes through such a window,
discarding the data, to size an output buffer.

`scripts/generate_brotli_tables.py` generates the static dictionary, checked
against RFC 7932's length and CRC-32 and against google/brotli's
`dictionary.bin`, and the transform and context tables, parsed from the RFC.
