# Network, TLS, HTTP and web verification

Completed on 2026-09-11 in the shared repository. No AGENTS.md was present in
the repository or its ancestors. The existing stdlib, adapters, compiler,
ownership rules, tests and documentation were inspected before implementation.
Independent agents owned networking, TLS and HTTP; the coordinating agent owned
web, shared I/O/compiler changes and integration. Verification was completed
before committing or pushing these changes.

The [machine-readable results](network-stdlib-results.json) record individual
portable and Windows fixture executions. Repeated focused Windows runs are
deduplicated by fixture/optimization when reporting the total below.

## Exact results

| Check | Result |
| --- | --- |
| `cargo test --locked --all-targets` | **394 passed, 0 failed, 0 ignored**, across 35 test executables |
| New networking harness | **6 passed**, O0/O3 native fixtures, exhaustion, independent TCP peer, borrow rejection and target emission |
| New TLS harness | **5 passed**, O0/O3 local credentials, HTTPS interoperability, allocation failure and checked lifetimes |
| New HTTP harness | **3 passed**, O0/O3 protocol/client properties, independent decoding and borrow rejection |
| New web harness | **4 passed**, O0/O3 routing/application/connection fixtures, static boundaries and runnable examples |
| HTTP parser mutation corpus | **29,184 cases**: 57 byte positions × 256 replacement values × O0/O3 |
| `test_portable_stdlib.py` | **136 successful objects**: 34 fixtures × wasm32/Cortex-M0 × O0/O3 |
| Broad existing/portable Wine runner | **84 successful executions**, 42 fixtures × O0/O3 |
| Additional native network Wine fixtures | **4 distinct executions**, TCP/UDP/resolver and backpressure × O0/O3 |
| Windows rooted static files | **2 distinct executions**, actual Windows file/directory symlink fixtures × O0/O3 |
| Windows TLS | **4 distinct executions**, paired client/server engine and HTTPS over TCP × O0/O3 |
| Windows HTTP examples | **4 distinct executions**, Windows client/server against independent Python peers × O0/O3 |
| All distinct Windows fixture/optimization pairs | **98 passed**, no skipped fixtures |
| HTTP/web examples using release compiler | **16 checks passed**, eight cases × O0/O3 |
| Clippy | Passed `--locked --all-targets -- -D warnings` |
| Rust formatting | Passed `cargo fmt --all --check` |
| Dodo formatting | Passed stdlib, examples, portable fixtures and new net/TLS/web fixture directories |
| Release compiler | Passed `cargo build --locked --release --bin dodo` |
| Release linkage check | No shared LLVM dependency; no OpenSSL dependency in compiler executable |
| Python tool tests | **16 passed** |
| Documentation build | **31 pages built** |
| Documentation references | **2106 links/assets/search targets checked** |
| Generated specification and whitespace | `render_spec.py --check` and `git diff --check` passed |

The TLS and HTTP/web interoperability fixtures use controlled loopback endpoints,
fresh local certificates, ephemeral ports, bounded waits and cleanup. They do not
contact public HTTP/TLS services. Resolver failure uses `AI_NUMERICHOST` with an
invalid numeric address, so it does not depend on external DNS connectivity.

## What the fixtures verify

- IPv4/IPv6 parsing and formatting, 2048 generated IPv6 round trips per
  optimization, DNS question/record parsing, compression bounds/cycles and a
  deterministic malformed-packet corpus. Real Linux and Windows IPv4/IPv6 TCP
  connect/listen/accept, partial transfer, refusal, half-close and EOF; UDP empty
  packets, message boundaries, small/zero-capacity receive and truncation.
  Deadline/cancellation progress, backpressure, Linux descriptor exhaustion,
  resolver errors and repeated cleanup are checked.
- OpenSSL verification succeeds for the generated CA and matching hostname and
  fails for untrusted, expired and wrong-host credentials. Tests include ALPN
  negotiation/mismatch, mutual TLS success/missing identity, fragmented handshakes,
  pending writes with modified caller input, bounded staging, allocation failure,
  authenticated shutdown and abrupt truncation. Full HTTP Client → TLS Stream →
  TCP composition interoperates with Python SSL on Linux and Windows/Wine.
- HTTP content lengths, chunking, extensions, trailers, informational responses,
  HEAD, CONNECT, upgrades, pipelining and close-delimited responses; conflicting
  lengths, TE/CL ambiguity, invalid syntax, every parser limit and truncated input.
  Serialization round trips, body backpressure, terminal poisoning, reuse checks,
  origin/trust/proxy separation, redirects/retry decisions and deadline/cancellation
  boundaries use deterministic inputs and fake transport/time values.
- Routing precedence and ambiguity, exact method behavior, percent decoding,
  absolute request targets, wildcard tails, middleware order, caller-backed
  response headers, partial sink progress and no read-ahead while backpressured.
  Rejected requests cannot subsequently dispatch to stale handlers. Rooted files
  reject traversal, alternate streams, links/reparse points and nonregular files.
- The compiler now rejects borrow-carrying writes through **external** mutable
  references as well as local reborrows. Regression cases cover callback receivers,
  referenced slices, indexed borrowed storage, local sources and request context.
  Valid owned aggregate updates and numeric handler state remain accepted.

## Reproduction and dependencies

This host used Rust 1.95.0, LLVM 22.1.8, Linux x86_64 GNU, OpenSSL 3.5.8 and
Clang/lld with MinGW headers/import libraries. Missing unversioned development
library links were supplied by the preexisting `/tmp/dodo-link-libs` directory,
as in the earlier stdlib verification. This is an environment workaround, not
a compiler or library dependency change.

```sh
LIBRARY_PATH=/tmp/dodo-link-libs cargo test --locked --all-targets
LIBRARY_PATH=/tmp/dodo-link-libs cargo clippy --locked --all-targets -- -D warnings
cargo fmt --all --check
LIBRARY_PATH=/tmp/dodo-link-libs cargo build --locked --release --bin dodo
python3 scripts/check-linkage.py target/release/dodo
python3 -m unittest discover -s scripts -p 'test_*.py'
python3 scripts/test_portable_stdlib.py
python3 scripts/test_http_web.py --compiler target/release/dodo
```

The installed Wine package lacked its matching data, so the already staged
complete Wine installation was selected explicitly, without changing the system:

```sh
python3 scripts/test_stdlib_windows.py \
  --wine /tmp/dodo-wine-complete-06nr3fy3/usr/bin/wine64 \
  --wineserver /tmp/dodo-wine-complete-06nr3fy3/usr/bin/wineserver64 \
  --mingw-include /tmp/dodo-mingw-headers/root/usr/x86_64-w64-mingw32/sys-root/mingw/include
```

Repeat that command with `--fixture tests/net/native_checks.dodo`,
`--fixture tests/net/backpressure.dodo` and
`--fixture tests/web/windows_static.dodo` for the additional hosted fixtures.
`scripts/test_web_windows.py` accepts the same Wine/header options and runs the
independent HTTP peers. Every runner uses an isolated Wine prefix and cleans up
only that prefix's services.

The Windows TLS backend was cross-built from the official OpenSSL 3.5.8 archive,
verified with SHA-256
`a8f84a39918ec6415ce765d9b429d313ba97b8143169c172e734b9514464f5b2`.
The archive, build tree and generated private credentials are not repository files.
The runner accepts an existing configured target build tree and downloads nothing:

```sh
python3 scripts/test_tls_windows.py \
  --openssl-source /tmp/dodo-tls-openssl/openssl-3.5.8 \
  --wine /tmp/dodo-wine-complete-06nr3fy3/usr/bin/wine64 \
  --wineserver /tmp/dodo-wine-complete-06nr3fy3/usr/bin/wineserver64 \
  --mingw-include /tmp/dodo-mingw-headers/root/usr/x86_64-w64-mingw32/sys-root/mingw/include
```

OpenSSL requires target development headers/libraries and uses Apache-2.0;
[TLS documentation](src/content/docs/tls.md) specifies versions, linkage, providers,
ownership and resource bounds. CI now prepares the pinned maintained backend
instead of relying on Ubuntu's older default package. The changed GitHub Actions
workflow itself has not been executed remotely during this task.

Build documentation with `npm run build` in `docs`, followed by
`python3 scripts/check_docs.py` and `python3 scripts/render_spec.py --check` from
the repository root.

## Remaining limitations

- Hosted adapters currently support x86_64 Linux GNU and Windows MSVC/GNU.
  Freestanding verification emits objects; it does not execute firmware or Wasm.
- OS `getaddrinfo` is synchronous and cannot be cancelled mid-call. The portable
  DNS codec is separate from OS resolver services; a deadline-aware DNS resolver,
  DNSSEC, cache and custom retry/selection policy are not bundled. Named IPv6 zones,
  local-domain sockets and OS-specific socket-option extensions are not supplied.
  Windows reports UDP truncation but cannot recover the original oversized length.
- TLS uses OpenSSL 3.5+, with bounded Dodo staging and certificate input limits.
  OpenSSL also allocates internally and is not subject to a global hard heap cap.
  Entropy comes from the selected OpenSSL/platform provider; there is no arbitrary
  entropy callback. Verification time and trust/credentials are explicit options.
  Revocation, resumption/early-data and DTLS policies are not implemented. Windows
  default roots mean OpenSSL trust paths, not automatic Windows certificate-store
  import.
- HTTP supports HTTP/1.1, not HTTP/1.0, HTTP/2 or HTTP/3. Redirect, retry, proxy
  establishment and transport-pool ownership are explicit orchestration: the client
  validates policies and boundaries but starts no hidden connection, thread or
  automatic body replay. Applications apply server phase/admission limits and
  shutdown through their chosen execution loop. No scheduler or worker pool is
  implicitly installed.
- WebSockets, cookies, multipart, compression and serialization integrations remain
  separate extension work. Routing has no automatic OPTIONS/Allow/virtual-host
  implementation. Static serving supplies secure rooted file opens and streaming
  file handles, without implicit index files, MIME detection or range handling.
- Wine success does not replace native Windows verification of driver/readiness
  behavior under load, reparse/sharing race stress, deployed OpenSSL DLL/provider
  discovery, trust-root configuration or operating-system entropy behavior.
  Optional ASan/UBSan linking was unavailable because host sanitizer runtimes were
  missing; deterministic cleanup/allocation-injection tests passed.

These are recorded limits of the implementation and verification, not claims of
complete RFC coverage, a TLS cryptography audit or a proof of compiler soundness.
