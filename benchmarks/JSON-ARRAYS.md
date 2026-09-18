# Sequential JSON array decoding: 2026-09-18

Derived array decoders now consume an `ArrayCursor` instead of calling
`take_element(value, index)` repeatedly. Each extraction starts where the last
one ended. Missing elements and a final `finish()` check enforce the exact
length without a separate counting pass. Primitive arrays use a generated loop;
borrowed, optional, and custom elements keep individual locals with normal
ownership checking and cleanup on failure.

The comparison uses the same fixture with a saved compiler from before the
change and a rebuilt compiler afterward, on the working checkout based on
`7baf51dd2ffa41e9ad3f644df4b4b7be939dce8d`. Both include the existing indexed
JSON changes. Environment: Windows 11 x64 build 26200, Intel Core Ultra 5 125U,
Dodo 0.1.3 / LLVM 23, Python 3.14.7. Seven measured batches per case target
0.2 seconds after calibration. Correctness tests finished before these timings;
the before and after benchmark runs ran sequentially.

| Elements | Input bytes | Before decode µs/op | After decode µs/op | Speedup | After parse µs/op |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 16 | 50 | 1.700 | 0.538 | 3.2× | 0.275 |
| 64 | 194 | 13.378 | 1.731 | 7.7× | 0.759 |
| 256 | 926 | 171.849 | 6.979 | 24.6× | 2.800 |

All table entries are medians at native `-O 3`. For 256 elements, measured batch
averages ranged from 169.434–181.359 µs before and 6.905–7.064 µs after. The
historical 234.033 µs decode / 2.757 µs parse figures came from the original
investigation probe. The ratios above use the current repeatable fixture and
fresh measurements, rather than mixing those historical timings with this run.

The new decoder also completed the benchmark at `-O 0`: 4.244, 14.523, and
59.729 µs per decode for 16, 64, and 256 elements respectively. For the
256-element `-O 3` fixture, the complete executable shrank from 641,024 to
239,104 bytes. Single observed build times were 75.20 seconds before and
2.65 seconds after; these build times are not repeated-sample statistics.

The fixture contains one object field, `values`, decoded into `[N]u32`.
Inputs change each iteration through a volatile read. Checksums use the first
and last decoded values, and every element is checked outside timing. Decoding
includes strict parsing and field lookup. These are local microbenchmark
observations; object lookup and duplicate detection are separate costs.

```sh
cargo build --locked --bin dodo
python scripts/bench_stdlib.py --suite json-arrays --optimization 0 3 --samples 7 --seconds 0.2 --linker clang --report benchmark-data/json-arrays.json
```

Use `--compiler` to select a saved compiler for a before/after comparison.
The fixture is also included in the `json` and `all` suites. Raw samples,
compiler/fixture hashes, and build metadata from this comparison are in the
ignored local files `benchmark-data/json-arrays-before-final.json` and
`benchmark-data/json-arrays-after-final.json`.

The JSON integration, derive, and frontend suites pass (21 Rust tests), with
native fixture execution at O0 and O3 and object emission for
`wasm32-unknown-unknown` and `thumbv6m-none-eabi`. Regressions cover empty,
short, long, and nested arrays; primitive types and numeric bounds; escaped
and borrowed strings; source lifetime rejection; and exactly-once cleanup
after a failed element conversion or length check. Element conversion errors
can now precede a later length mismatch because conversion happens during
sequential consumption.
