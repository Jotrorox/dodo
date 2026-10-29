---
title: "Compression"
description: "Encode and decode DEFLATE, zlib, and gzip with caller-supplied buffers."
section: "Standard library"
order: 144.5
---

The portable compression packages work with caller-supplied byte slices and do
not require an allocator. Raw DEFLATE, zlib, and gzip use the same compression
format with different framing; select the package matching the data on the wire.

## Complete buffers

`std/compress/deflate`, `std/compress/zlib`, and `std/compress/gzip` provide
`decode(input, output)`. The result reports input bytes consumed and output
bytes written. Each call decodes one stream or gzip member, leaving a following
stream unconsumed. Limit expansion by limiting the output buffer. On error,
the contents of that buffer are unspecified. Errors distinguish malformed or
truncated input and a full output buffer; the wrappers also detect checksum
failures. The zlib decoder rejects preset dictionaries and checks its advertised
window size. The gzip decoder validates optional header fields, header CRC,
data CRC, and size. Decode concatenated gzip members by calling again on the
unconsumed suffix.

Each framing package's `encode` emits stored blocks. For compression, use
`std/compress/deflate_fixed` or `std/compress/deflate_dynamic` for raw streams,
or `encode_fixed` and `encode_dynamic` in the zlib and gzip packages. These use
bounded LZ77 match search. The dynamic encoder uses frequency-ranked balanced
code trees; it does not guarantee the smallest stream. The corresponding
`encode_level` functions (named `encode_fixed_level` and
`encode_dynamic_level` on the wrappers) accept 0 through 9: level 0 emits
stored blocks, and 1 through 9 increase the match-search limit. Levels do not
promise monotonically smaller output. Encoder output contents are unspecified
on failure. The one-shot compressed encoders use roughly 288 KiB of match-search
workspace; callers can provide and reuse a `std/compress/deflate_workspace.Workspace`
with the `encode_with_workspace` variants (or the wrapper variants of those
names). The workspace contains 36,864 `usize` values; plan its storage for
the target.

## Incremental processing

`std/compress/inflate.Decoder`, `std/compress/zlib_stream.Decoder`, and
`std/compress/gzip_stream.Decoder` decode raw, zlib, and gzip data respectively.
Each owns a bounded 32 KiB history and Huffman workspace. Call
`process(input, output)` repeatedly, advancing by the returned `input_consumed`
and `output_written` counts. `NeedInput` requests more input; `NeedOutput`
requests output space; `Done` marks the end of one stream. A `NeedInput` at
end-of-file means the stream is truncated. The decoder consumes no bytes after
its stream, so the caller can process another gzip member or other trailing data.
After an error discard the decoder; `last_input_consumed()` and
`last_output_written()` report progress made in the failing call.

`std/compress/deflate_store.Encoder`, `std/compress/zlib_store.Encoder`, and
`std/compress/gzip_store.Encoder` incrementally produce stored streams.
Call `process(input, output, finish)` with `finish` true when the supplied
input is the final suffix; keep it true and resubmit any unconsumed input until
`Done`. The returned counts and statuses follow the same buffer convention.

`std/compress/io.Reader` adapts a `std/io.Reader` to incremental raw, zlib, or
gzip decoding. Select the `Raw`, `Zlib`, or `Gzip` codec. `read_checked` keeps
I/O, truncation, format, checksum, and unsupported-dictionary errors distinct;
its `transferred` count includes output produced before an error. Its `read`
method also implements the ordinary `std/io.Reader` contract, mapping format
errors to I/O errors. The adapter reads one compressed byte at a time so it
does not consume data after the stream. `std/compress/io.Writer` provides
stored-block streaming output for the same formats with `RawEncoder`,
`ZlibEncoder`, or `GzipEncoder`. Call `finish()` and handle its result to
write the final block and trailer. After a write error, its `transferred` count
is the input accepted by that call; pending compressed bytes are retained for
a retry. `finish()` reports compressed bytes flushed by that call.
