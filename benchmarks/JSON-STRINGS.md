# Ordinary JSON string fast paths: 2026-09-18

`String.equal` now compares bytes when both strings are unescaped, and
`decoded_len` returns the stored byte count. Unescaped `decode` checks capacity
and copies bytes directly into the caller's buffer. String validation advances
over ordinary ASCII without decoding scalars; escapes and non-ASCII bytes still
use the existing Unicode validator. Escaped equality, surrogate pairs, error
positions, and the no-partial-write guarantee on capacity errors are preserved.

Both parsed strings and `String.from_str` contain valid UTF-8, so byte equality
has the same meaning as scalar equality for unescaped content. A literal
backslash in `from_str` remains decoded content, not a JSON escape.

The comparison uses a saved compiler built immediately before this change and
the rebuilt working checkout based on
`7baf51dd2ffa41e9ad3f644df4b4b7be939dce8d`. Both include indexed validation,
sequential array decoding, and single-pass struct decoding. Environment:
Windows 11 x64 build 26200, Intel Core Ultra 5 125U, Dodo 0.1.3 / LLVM 23,
Python 3.14.7. Runs were sequential at native `-O 3`, with five measured batches
targeting 0.1 seconds after discarded calibration passes.

| String | Operation | Before ns/op | After ns/op | Speedup |
| --- | --- | ---: | ---: | ---: |
| ASCII | Parse | 1908.6 | 276.0 | 6.9× |
| ASCII | Equality | 3345.7 | 148.4 | 22.5× |
| ASCII | Decoded byte length | 2075.6 | 1.6 | — |
| ASCII | Decode/copy | 4670.4 | 101.1 | 46.2× |
| Literal UTF-8 | Parse | 738.4 | 423.9 | 1.7× |
| Literal UTF-8 | Equality | 1302.5 | 93.7 | 13.9× |
| Literal UTF-8 | Decoded byte length | 767.0 | 1.6 | — |
| Literal UTF-8 | Decode/copy | 1999.1 | 66.2 | 30.2× |
| Escaped | Parse | 1330.6 | 880.8 | 1.5× |
| Escaped | Equality | 2243.2 | 2210.6 | 1.0× |
| Escaped | Decoded byte length | 1317.5 | 1253.8 | 1.1× |
| Escaped | Decode/copy | 3240.1 | 3075.2 | 1.1× |

Times are median batch averages. ASCII equality after the change ranged from
144.9–150.4 ns and copying from 99.2–102.5 ns. These are local observations,
not performance thresholds or end-to-end application speedups. The length fast
path does constant work; its 1.6 ns result includes loop, volatile selection,
and checksum overhead, so no isolated-operation speedup is claimed for it.
Small differences in unchanged escaped decoding are not attributed to the fast
paths; escaped parsing also benefits from the ordinary ASCII it encounters.

Inputs contain 353 decoded ASCII bytes, 209 literal UTF-8 bytes, or 273 decoded
bytes represented using escapes (355, 211, and 563 JSON bytes respectively).
Equality alternates matching content and a final-byte mismatch, comparing
against an independently decoded reference. Length, equality, and copy exclude
parsing. Each batch verifies its checksum; decode also verifies every byte of
its final output outside timing. A volatile index selects the string variant,
and parsing changes the final content byte each iteration.

```sh
cargo build --locked --bin dodo
python scripts/bench_stdlib.py --suite json-strings --optimization 3 --samples 5 --seconds 0.1 --linker clang --report benchmark-data/json-strings.json
```

Use `--compiler` to select a saved compiler for comparison. Raw samples and
compiler/source hashes are stored locally in the ignored
`benchmark-data/json-strings-before.json` and `json-strings-after.json` reports.
The single-sample `json-strings-smoke.json` run checks all 12 cases at O0 and
is not a timing baseline.

Validation covers 36 JSON integration/frontend/derive tests, including native
O0/O3 execution and object emission for `wasm32-unknown-unknown` and
`thumbv6m-none-eabi`. The string fixture checks empty and multibyte strings,
prefix and final-byte mismatches, escaped equivalents in both directions,
literal backslashes and controls from `from_str`, exact and oversized buffers,
every insufficient capacity, and output outliving its source. Both parsing
entry points reject malformed UTF-8, invalid escapes, and duplicate decoded
keys with the expected byte positions. Rust/Dodo formatting, Python compilation,
and whitespace checks pass.
