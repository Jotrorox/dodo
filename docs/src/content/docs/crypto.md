---
title: "Cryptography"
description: "OS random bytes, SHA-1 and SHA-256, HMAC-SHA256, AES-128-CFB8 streams, and RSA PKCS#1 v1.5 keys."
section: "Standard library"
order: 157.5
---

`std/crypto/random` reads the operating system's cryptographically secure random
number generator (CSPRNG). Use it for session tokens, keys, and SipHash map keys.
`std/crypto/openssl` provides SHA-1, SHA-256, HMAC-SHA256, AES-128 in CFB8 mode,
and RSA key pairs with PKCS#1 v1.5 encryption, all from OpenSSL's libcrypto.
`std/crypto` itself holds only the shared error type and two helpers for handling
secrets. It is portable and links nothing.

These are building blocks. Dodo does not implement any cryptographic primitive
itself, and the API offers no protocol-level shortcuts. Use the algorithm a
protocol names, and prefer SHA-256 over SHA-1 wherever you get the choice.

## Quickstart

This hosted example needs **OpenSSL 3.5+ headers and libcrypto**, just like
[TLS](tls.md#quickstart). Save it as `token_start.dodo`:

```dodo test
package token_start
import "std/crypto"
import "std/crypto/random"
import "std/crypto/openssl"

fn main() -> i32 {
    // A per-process secret and a 16-byte session token, both from the OS.
    secret := [0u8; 32]
    token := [0u8; 16]
    match random.fill(&mut secret) {
        ok() => {}, err(_) => { return 1 },
    }
    match random.fill(&mut token) {
        ok() => {}, err(_) => { return 1 },
    }
    // Tag the token, then check a presented copy against the tag.
    tag := match openssl.hmac_sha256(&secret, &token) {
        ok(value) => { value }, err(_) => { return 2 },
    }
    check := match openssl.HmacSha256.new(&secret) {
        ok(value) => { value }, err(_) => { return 2 },
    }
    match check.update(&token) {
        ok() => {}, err(_) => { return 2 },
    }
    match check.verify(&tag) {
        ok(valid) => { assert(valid) }, err(_) => { return 2 },
    }
    crypto.wipe(&mut secret)
    return 0
}
```

```sh
dodo run token_start.dodo
```

It prints nothing and exits 0. Exit 1 means the OS refused to supply random
bytes. Exit 2 means OpenSSL failed. In both cases, stop: there is no weaker
fallback to try.

## Packages

| Import | Contents | Links |
| --- | --- | --- |
| `std/crypto` | `Error`, `ErrorKind`, `equal`, `wipe` | Nothing; portable to freestanding targets. |
| `std/crypto/random` | `fill`, `next_u64`, `below`, `key`, `SystemRandom` | The OS CSPRNG: libc on Linux, Security.framework on macOS, bcrypt on Windows. |
| `std/crypto/openssl` | `Sha1`, `Sha256`, `Md5`, `HmacSha256`, `pbkdf2_hmac_sha256`, `Aes128Cfb8`, `RsaPrivateKey`, `RsaPublicKey` | OpenSSL 3.5+ libcrypto, but not libssl. |

The two hosted packages link separately. A program that needs only random bytes
does not need OpenSSL installed, and importing `std/crypto/openssl` does not
pull in the TLS runtime.

## Errors and secrets

Every fallible call returns `T!crypto.Error`. `Error.kind` is one of:

| Kind | Meaning |
| --- | --- |
| `InvalidInput` | A wrong key, IV, or ciphertext length, an RSA size out of range, or malformed DER. |
| `BufferTooSmall` | The output slice is too short. Nothing was written to it. |
| `AllocationFailed` | OpenSSL could not allocate state for a new value. |
| `Unavailable` | The OS random generator failed. `code` holds the native error. |
| `Backend` | OpenSSL reported a failure. `code` holds its packed error code. |

`Error.code` is numeric only. Formatting an error never prints keys, plaintext,
or digests.

`crypto.equal(left, right)` compares MACs, tokens, and other secrets without
returning early at the first differing byte. Inputs of different lengths compare
unequal immediately: length is not treated as secret. `crypto.wipe(bytes)` zeroes
a buffer with stores the optimizer cannot remove. Wipe shared secrets and
decrypted keys once you no longer need them. OpenSSL copies keys during
construction, so you can wipe your own copy right after creating a value.

## Random bytes

`random.fill(output)` fills the whole slice from the OS generator:

| OS | Source |
| --- | --- |
| Linux | `getrandom(2)` with no flags. It blocks only until the kernel pool is first initialized, and retries on `EINTR`. |
| macOS | `SecRandomCopyBytes(kSecRandomDefault, …)` |
| Windows | `BCryptGenRandom` with `BCRYPT_USE_SYSTEM_PREFERRED_RNG` |

Requests of any size are split below each OS's per-call limit. Every call goes
straight to the OS: there is no user-space generator to seed, fork, or reseed.
On failure the output contents are unspecified, and you must not use them.

* `random.next_u64()` returns a uniform `u64`.
* `random.below(bound)` returns a uniform value in `0..bound` using rejection
  sampling, so it has no modulo bias. Use it to pick characters for tokens from
  an alphabet. `bound == 0` fails with `InvalidInput`.
* `random.key()` returns a random `hash.Key128` for SipHash maps.
* `random.SystemRandom.new()` is a stateless source with the
  [`hash.obtain_key`](hash.md#keyed-hashing-and-caller-supplied-keys) capability.
  It maps OS failure to `hash.KeyError.Unavailable`.

## Hashes and HMAC

`openssl.sha1(data)` and `openssl.sha256(data)` return `[20]u8` and `[32]u8`.
For data that arrives in pieces, create `Sha1.new()` or `Sha256.new()`, call
`update` for each piece, then call `finish`. `finish` returns the digest and
starts a new empty message on the same value. Split points never change the
result.

SHA-1 is not collision resistant. Use it only where a protocol requires it,
such as Minecraft's server hash. `Md5` and `md5(data)` (`[16]u8`) exist for
the same reason, for PostgreSQL's legacy md5 password authentication; MD5 is
broken as a hash.

`HmacSha256.new(key)` accepts keys of any length, including empty keys. Keys
longer than 64 bytes are hashed first, as RFC 2104 specifies. `finish` returns
the 32-byte tag and keeps the key for the next message. `verify(expected)`
finishes the message and compares the tag in constant time. A truncated or
overlong `expected` never matches. `hmac_sha256(key, data)` is the one-shot
form, suited to Velocity modern forwarding and messaging token MACs.

`pbkdf2_hmac_sha256(password, salt, iterations, &mut output)` derives
`output.len` bytes with PBKDF2 (RFC 8018) over HMAC-SHA256, as SCRAM
authentication and many password stores need. `iterations` must be at least 1.

Minecraft prints its server hash as a signed number: the SHA-1 digest read as
one big-endian two's-complement integer, in hex. This helper reproduces the
published examples:

```dodo test
package server_hash
import "std/crypto"
import "std/crypto/openssl"

// A leading '-' and the two's complement when the top bit is set; no
// leading zeros.
fn server_hash(digest: [20]u8, output: &mut[u8]) -> usize {
    bytes := digest
    used := 0usize
    if bytes[0] >= 128 {
        output[0] = 45
        used = 1
        carry := 1u16
        for i in 0usize..20 {
            value := ((bytes[19 - i] ^ 255) as u16) + carry
            bytes[19 - i] = (value & 255) as u8
            carry = value >> 8
        }
    }
    digits := b"0123456789abcdef"
    leading := true
    for i in 0usize..40 {
        nibble := (bytes[i / 2] >> (4 - 4 * (i % 2) as u8)) & 15
        if leading && nibble == 0 && i != 39 {
            continue
        }
        leading = false
        output[used] = digits[nibble as usize]
        used += 1
    }
    return used
}
fn hashes_to(name: &[u8], expected: &[u8]) -> bool {
    digest := match openssl.sha1(name) {
        ok(value) => { value }, err(_) => { return false },
    }
    text := [0u8; 41]
    used := server_hash(digest, &mut text)
    return crypto.equal(&text[..used], expected)
}
fn main() -> i32 {
    // A server feeds its server ID, the shared secret and its public key
    // DER into one Sha1; single names give the published examples.
    assert(hashes_to(b"Notch", b"4ed1f46bbe04bc756bcb17c0c7ce3e4632f06a48"))
    assert(hashes_to(b"jeb_", b"-7c9d5b0044c130109a5d7b5fb5c317c02b4e28c1"))
    assert(hashes_to(b"simon", b"88e16a1019277b15d58faf0541e11910eb756f6"))
    return 0
}
```

## AES-128-CFB8 streams

`Aes128Cfb8.encryptor(key, iv)` and `Aes128Cfb8.decryptor(key, iv)` each take a
16-byte key and a 16-byte IV; any other length fails with `InvalidInput`. CFB8
is a stream mode: each output byte corresponds to one input byte, and each
`update` continues where the previous one stopped. You can split a connection's
bytes at any point, including into empty pieces. A connection needs one
encryptor for outgoing bytes and one decryptor for incoming bytes. Minecraft
uses the 16-byte shared secret as both key and IV.

* `update(input, output)` writes `input.len` bytes to the start of `output` and
  leaves the rest of `output` unchanged. A shorter `output` fails with
  `BufferTooSmall` before any progress is made.
* `update_in_place(data)` transforms a buffer in place, which suits a socket
  read buffer.

CFB8 provides confidentiality only. It does not detect tampering. Add a MAC
unless the protocol you implement already decides otherwise.

## RSA keys

`RsaPrivateKey.generate(bits)` creates a key pair with public exponent 65537.
`bits` must be in `RSA_MIN_BITS..=RSA_MAX_BITS` (1024 to 16384). Generating a
1024-bit key takes milliseconds; a 4096-bit key can take seconds. RSA-1024 is
only as strong as roughly 80-bit symmetric security. Use it only where a
protocol fixes that size, as Minecraft's login does, and generate a fresh key
when the process starts.

* `bits()` and `size()` return the modulus length in bits and in bytes. `size()`
  is also the ciphertext length.
* `public_key_der(output)` writes the public key as DER SubjectPublicKeyInfo
  (RFC 5280), the form Minecraft's encryption request carries, and returns its
  length. `public_key_der_len()` returns that length in advance: 162 bytes for a
  1024-bit key. A short `output` fails with `BufferTooSmall` and is left
  unchanged.
* `decrypt_pkcs1(ciphertext, output)` performs RSAES-PKCS1-v1_5 decryption
  (RFC 8017) and returns the message length. `ciphertext` must be exactly
  `size()` bytes.

OpenSSL applies **implicit rejection** to PKCS#1 v1.5 decryption. A malformed
ciphertext of the right length does not fail: it decrypts to a deterministic
pseudorandom message, so an attacker learns nothing from padding errors.
Always validate what you decrypt. For Minecraft, check that the shared secret is
16 bytes and that the decrypted verify token equals the one you sent, using
`crypto.equal`.

`RsaPublicKey.from_der(der)` parses exactly one DER SubjectPublicKeyInfo that
holds an RSA key in the supported size range. Trailing bytes, other key types,
and truncated input fail with `InvalidInput`. `encrypt_pkcs1(message, output)`
uses fresh random padding, accepts at most `size() - PKCS1_OVERHEAD` message
bytes (117 for a 1024-bit key), and needs `size()` bytes of output. Clients,
test peers, and bots use it for the other side of a key exchange.

Private keys cannot be imported or exported; persist none and generate per run.
Signatures, OAEP, and other key types are not provided.

## Ownership and threads

Each `Sha1`, `Sha256`, `HmacSha256`, `Aes128Cfb8`, `RsaPrivateKey`, and
`RsaPublicKey` value owns its OpenSSL state and frees it when dropped. No call
keeps a borrow of your buffers after it returns. The values are deliberately
neither Send nor Sync: use each one on a single execution lane, and give every
connection its own cipher pair.

## Backend and build

`std/crypto/openssl` needs **OpenSSL 3.5 or later**, the same backend as
[TLS](tls.md#backend-and-build), with headers and libcrypto for the target C
toolchain. Hosted linking compiles the embedded C boundary and adds `-lcrypto`;
on macOS Dodo finds Homebrew's `openssl@3` unless `CPATH` or `LIBRARY_PATH` is
set. `std/crypto/random` needs only the OS: hosted linking adds
`-framework Security` on macOS and `-lbcrypt` on Windows. With `--emit obj`, you
compile `stdlib/std/crypto/runtime.c` or `stdlib/std/crypto/openssl/runtime.c`
yourself and link the matching libraries.

Both hosted packages support the same targets as the other hosted adapters:
x86-64 and AArch64 Linux GNU, macOS, and Windows x64. Importing them for any
other target produces an adapter diagnostic.

## Verification

`cargo test --test crypto_library` builds its fixtures at `-O0` and `-O3` and
runs:

* published vectors: FIPS 180-2 for SHA-1 and SHA-256 (including one million
  `a`s fed in uneven pieces), RFC 4231 for HMAC-SHA256 (including a key longer
  than the block size), and NIST SP 800-38A F.3.7/F.3.8 for AES-128-CFB8;
* entropy checks: distinct outputs, bit balance, 70,000-byte requests, and the
  range and coverage of `below`;
* RSA checks: the DER layout, round trips, the 117-byte message limit, short
  buffers, tampered ciphertexts, malformed DER, and size limits;
* interoperability with the `openssl` command-line tool in both directions. It
  parses Dodo's DER, encrypts a secret to a Dodo key, and decrypts a Dodo
  ciphertext with implicit rejection turned off, so a wrong padding mode fails
  outright rather than decrypting to garbage.

The same test checks that `std/crypto` compiles for WebAssembly and Cortex-M0,
and that the entropy and libcrypto boundaries link independently.

## Complete API reference

For every public type, constant, and function signature, see
[std/crypto](api/std/crypto.md), [std/crypto/random](api/std/crypto/random.md),
and [std/crypto/openssl](api/std/crypto/openssl.md).
