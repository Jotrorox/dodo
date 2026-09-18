# Single-pass JSON struct decoding: 2026-09-18

Derived struct decoders now consume an `ObjectCursor` once. Generated balanced
decision trees dispatch hashes of decoded Unicode field names, with exact name
comparisons inside each hash bucket. An optional raw value per schema field
tracks presence. After traversal, conversion runs in declaration order, keeping
unknown-field error precedence, custom codec order, borrowing, and failure
cleanup. Missing optional fields and explicit null still decode to `none`.

Dispatch takes logarithmic hash comparisons per member, plus exact comparisons
within a collision bucket. Temporary field storage is proportional to the schema
size. This removes repeated object traversal during conversion; ordinary parsing
still has its separate quadratic duplicate-name check.

The comparison uses a saved compiler from immediately before this change and
the rebuilt compiler on the working checkout based on
`7baf51dd2ffa41e9ad3f644df4b4b7be939dce8d`. Both include the existing indexed
validation and sequential array changes. Environment: Windows 11 x64 build
26200, Dodo 0.1.3 / LLVM 23, Python 3.14.7. Five measured batches target 0.1
seconds after calibration, at native `-O 3`. Runs were sequential.

| Fields | Indexed decode before µs/op | Indexed decode after µs/op | Speedup | Ordinary decode before µs/op | Ordinary decode after µs/op |
| ---: | ---: | ---: | ---: | ---: | ---: |
| 16 | 8.548 | 3.451 | 2.5× | 13.902 | 10.135 |
| 64 | 101.563 | 11.381 | 8.9× | 199.732 | 129.473 |

Values are medians. Indexed decoding includes `parse_indexed` (including reset
of 1,280 scratch words) and `from_value`; ordinary decoding uses `decode` and
includes constant-space duplicate validation. Indexed parsing alone measured
1.324 → 1.774 µs for 16 fields and 5.323 → 5.390 µs for 64 fields. These are
local observations with machine variability, not isolated conversion timings.
For 64 fields, indexed decode batches ranged from 99.644–104.914 µs before and
11.188–13.951 µs after.

The fixture supplies fields in reverse declaration order. A volatile loop index
changes the last input digit on every iteration. Each timed batch checks a
checksum, and every decoded field is verified outside the timer. Source and
compiler hashes, raw samples, and build metadata are retained locally in the
ignored `benchmark-data/json-structs-before.json` and
`benchmark-data/json-structs-after.json` reports. The old compiler's 256-field
build exceeded the runner's 180-second compilation limit, so no speedup is
reported for that width. The default struct suite measures 16 and 64 fields;
`--struct-sizes` selects other widths up to 256.

```sh
cargo build --locked --bin dodo
python scripts/bench_stdlib.py --suite json-structs --struct-sizes 16 64 --optimization 3 --samples 5 --seconds 0.1 --linker clang --report benchmark-data/json-structs.json
```

Use `--compiler` to select a saved compiler for a before/after comparison.

Validation covers 23 Rust JSON integration/frontend/derive tests, including
native execution at O0/O3 and object emission for `wasm32-unknown-unknown` and
`thumbv6m-none-eabi`. Regressions exercise 70 fields, arbitrary member order,
missing fields across the schema, optional/null fields, empty and escaped names,
Unicode and surrogate pairs, deliberate hash collisions, unknown and duplicate
fields, exact error positions, source lifetimes, and exactly-once owner cleanup.
Clippy and formatting checks also pass.
