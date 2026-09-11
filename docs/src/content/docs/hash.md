---
title: "Hashing and checksums"
description: "Portable incremental hashing, keyed map policies, and independent checksums."
section: "Using Dodo"
order: 144
---

`std/hash` provides allocation-free byte hashing and statically dispatched map
policies. `std/checksum` is an independent import for accidental-corruption
checks. Neither package imports collections, allocators, an operating system,
or an entropy provider. Both use fixed-size state and O(n) time for n input
bytes; there are no large tables or native-layout reads.

## Algorithms and stable results

| API | Definition | Intended use |
| --- | --- | --- |
| `hash.Fnv1a64`, `hash.fnv1a64` | FNV-1a-64; offset basis 14695981039346656037, prime 1099511628211, unsigned arithmetic modulo 2^64 | Trusted byte keys, stable noncryptographic fingerprints |
| `hash.SipHash24`, `hash.siphash24` | SipHash-2-4, 128-bit key, 64-bit result | Hash-table keys potentially controlled by an adversary |
| `checksum.Crc32`, `checksum.crc32` | CRC-32/ISO-HDLC, reflected polynomial 0xEDB88320, initial and final XOR 0xFFFFFFFF | Accidental corruption detection |
| `checksum.Adler32`, `checksum.adler32` | Adler-32, modulus 65521, initial sums 1 and 0 | Zlib-compatible checksums |

Results are stable across architectures and future releases for the same named
algorithm, exact input bytes, and seed/key. An algorithm change requires a new
name. Return values are integers; serialize them explicitly with the desired
protocol byte order. No algorithm reads padding or a native struct layout.
All algorithm arithmetic is explicitly modular where specified, independently
of Dodo's ordinary checked integer arithmetic.

All state types expose `update(&mut self, &[u8])`, `finish(&self)`, and
`reset(&mut self)`. `finish` returns a snapshot without consuming, modifying,
or resetting state; repeated calls agree, and further updates continue the
original stream. Empty updates have no effect. `reset` restores the initial
seed/key. `Fnv1a64.new()` uses the standard offset basis;
`Fnv1a64.seeded(seed)` replaces it exactly. The seed does not make FNV secure.
The one-shot helpers produce the same result as `new`, `update`, `finish`.
SipHash accepts any stream length and encodes the length modulo 256 in its
final block as specified by the algorithm.

```dodo
package example
import "std/hash"

fn main() -> i32 {
    part1 := [104u8, 101]
    part2 := [108u8, 108, 111]
    state := hash.Fnv1a64.new()
    state.update(&part1)
    state.update(&part2)
    if state.finish() != 0xa430d84680aabd0bu64 { return 1 }
    return 0
}
```

## Keyed hashing and caller-supplied keys

`SipHash24.new(k0, k1)` and `siphash24(data, k0, k1)` interpret the two key
words as the little-endian encodings of key bytes 0..7 and 8..15. The caller
supplies all 128 bits. For maps exposed to untrusted input, obtain an
unpredictable key through an application-selected entropy adapter and retain it
for the map's lifetime. Independent map keys can limit the scope of a leaked
key. These packages do not acquire entropy, create a process-global key, or
silently replace missing entropy with a fixed seed. Fixed keys in examples and
tests serve reproducibility only.

`Key128.new(k0, k1)` holds explicit key words, exposed through `word0` and
`word1`. Its `hasher`, `i32_policy`, and `u64_policy` methods construct keyed
states/policies. `obtain_key(&mut source)` invokes the statically dispatched
capability `source.next_key(&mut self) -> Key128!KeyError` exactly once.
The source type and its `next_key` method must be public for dispatch from the
hash package. `KeyError.Unavailable` propagates unchanged: there is no retry, fallback,
implicit seed, or entropy access. The application implements the source and is
responsible for unpredictable production keys. A source can return a fresh key
per request or apply an explicit application key-management policy. Failure
may advance source state according to that source's contract, but never returns
a usable key. The library provides no deterministic source as a production
default; fixtures/examples define clearly named deterministic test sources.

SipHash is used here as a keyed table hash with a 64-bit output. It is not an
unkeyed collision-resistant digest, password hash, general-purpose message
authentication API, or encryption algorithm. No cryptographic API is provided;
there is no constant-time or key-erasure guarantee. Checksums and FNV provide
no protection against an adversary who can choose or alter the bytes.

## Statically dispatched contracts and value encodings

Generic hashing helpers expect `H.update(&mut self, data: &[u8])`. A complete
incremental state additionally supplies `finish(&self) -> u64` and
`reset(&mut self)`. Calls are monomorphized; there are no traits, closures,
reflection, implicit hashing, or vtables.

`write_u64(state, value)` emits exactly eight little-endian bytes.
`write_i32(state, value)` emits exactly four little-endian two's-complement
bytes, including for negative values. `write_bytes(state, data)` prefixes a
variable-sized field with its length encoded as a little-endian u64, then emits
its bytes. This framing distinguishes, for example, the two fields `ab`,`c`
from `a`,`bc`. User-defined records should hash logical fields explicitly,
including a discriminant where variants require one; never hash their memory
representation. Floating-point keys need an explicit equality policy and
matching treatment of signed zero and NaNs; no default floating policy exists.

Map policies provide `hash(&self, key: &K) -> u64` and
`equal(&self, a: &K, b: &K) -> bool`. Equal keys must hash equally; equality
must be an equivalence relation. Policy state and key equality/hash behavior
must remain stable while keys are stored. `I32 {}` and `U64 {}` use FNV and
ordinary integer equality. `SipI32.new(k0, k1)` and
`SipU64.new(k0, k1)` provide the same encodings with SipHash. Colliding unequal
keys are allowed and handled by collections.

## Attribution and validation

FNV follows Fowler/Noll/Vo's algorithm and the
[IETF FNV specification](https://www.ietf.org/archive/id/draft-eastlake-fnv-25.html).
SipHash follows Jean-Philippe Aumasson and Daniel J. Bernstein's
[SipHash specification](https://www.aumasson.jp/siphash/siphash.pdf); the fixture
contains all 64 official 64-bit
[reference vectors](https://github.com/veorq/SipHash/blob/master/vectors.h),
interpreted as little-endian integers. CRC-32 follows
[RFC 1952](https://www.rfc-editor.org/rfc/rfc1952.html), and Adler-32 follows
[RFC 1950](https://www.rfc-editor.org/rfc/rfc1950.html). These are direct Dodo
implementations of the algorithms, without C or libc dependencies.

`tests/stdlib/hash_checks.dodo` checks published vectors, independent Python
zlib checksum results, every split position in a 513-byte input, zero-length
updates, repeated finalization, reset, seeds, integer/framed encodings, and caller-key-source success/failure
without fallback or retries.
`tests/hash_library.rs` runs it at `-O0` and `-O3` and emits WebAssembly and
Cortex-M0 objects. See `examples/hash.dodo` for a complete program.
