# Indexed JSON parsing: 2026-09-18

`json.parse_indexed` and `json.parse_str_indexed` now accept caller-owned
`&mut[usize]` scratch for duplicate-name detection. The original constant-space
parser remains available. See [API usage and capacity rules](../docs/src/content/docs/json.md#indexed-parsing).

The table compares both entry points in the same modified checkout based on
`7baf51dd2ffa41e9ad3f644df4b4b7be939dce8d`, using Windows 11 x64 (build 26200),
the Intel Core Ultra 5 125U, Dodo 0.1.3 / LLVM 23, native `-O 3`, and Python
3.12.14. Seven measured batches per case target 0.2 seconds after calibration.
Compilation and correctness tests completed before timing.

| Fields | Input bytes | `parse` median µs/op | `parse_indexed` median µs/op | Ratio |
| ---: | ---: | ---: | ---: | ---: |
| 32 | 339 | 50.977 | 3.940 | 13× |
| 256 | 2,958 | 2352.603 | 31.099 | 76× |
| 1,024 | 12,197 | 36220.900 | 167.724 | 216× |

For 1,024 fields, the batch averages ranged from 34.683–37.329 ms for `parse`
and 0.154–0.186 ms for `parse_indexed`. The earlier 33.3 ms figure belongs to
the [historical baseline](RESULTS.md); the ratios above use this run's paired
measurements. These are local observations, not performance guarantees.

Every indexed case uses 5,120 scratch words: capacity for 1,024 live object
members, or 40 KiB on this target. Initial array allocation/initialization is
outside timing; each parse's bucket reset, hashing, collision comparisons,
insertion, and scope cleanup are included. Inputs change on every iteration,
and the runner verifies checksums for every calibration and measured batch.
Raw samples and metadata are in the ignored local file
`benchmark-data/json-indexed.json`.

```sh
cargo build --locked --bin dodo
python scripts/bench_stdlib.py --suite json --samples 7 --seconds 0.2 --linker clang --report benchmark-data/json-indexed.json
```

The index hashes decoded Unicode scalars and compares complete decoded names
within each collision chain. A shared entry stack releases each object's keys
when it closes, preserving ancestor keys and reusing space for sibling objects.
Exhaustion returns `BufferFull` at the new key. Returned values borrow the input
only, and each call resets scratch even after a previous error. Deliberately
colliding keys can still produce quadratic comparisons.

Validation covers escaped-equivalent names, colliding keys and ancestor buckets,
nested scopes, exact/insufficient/empty scratch, reuse after failures, source
lifetimes, unknown fields, depth limits, and exact error parity over all prefixes
and single-byte mutations of a nested Unicode document. The JSON integration,
frontend, and derive suites pass (20 Rust tests), including native fixture
execution at `-O0`/`-O3` and object emission for `wasm32-unknown-unknown` and
`thumbv6m-none-eabi`. All five executable JSON documentation examples pass.
