# Standard-library onboarding review

The standard-library overview now leads with everyday tasks and links each
public family to a runnable guide. It inventories all 113 source packages,
compiler intrinsic packages, and virtual native adapters. Application entry
points are distinguished from implementation helpers. The work uses the current
checkout's console, native workspace, file, environment, command, shared-text,
clock, hosted HTTP/HTTPS, and web convenience APIs.

This documentation snapshot depends on the concurrent console, hosted API,
hosted HTTP/HTTPS, and reference-container implementation tasks. The documentation
PR remains a draft until those APIs and their example/setup files land. The
executable validation below used the combined working checkout; it does not
claim that these examples compile against the PR's current `main` base alone.

## Pages and tooling changed

New module guides:

- [Core utilities](src/content/docs/core.md)
- [Allocation and boxes](src/content/docs/allocation.md)
- [Binary bytes and buffers](src/content/docs/bytes.md)
- [Console I/O and printing](src/content/docs/console.md)
- [Byte I/O](src/content/docs/io.md)
- [Formatting](src/content/docs/formatting.md)
- [UTF-8 text and strings](src/content/docs/text.md)

Revised module and overview pages:

- [Standard-library overview and inventory](src/content/docs/standard-library.md)
- [Collections](src/content/docs/collections.md)
- [Container element safety](src/content/docs/container-elements.md)
- [Environment](src/content/docs/environment.md)
- [Filesystem](src/content/docs/filesystem.md)
- [First program](src/content/docs/first-program.md)
- [Hashing and checksums](src/content/docs/hash.md)
- [HTTP](src/content/docs/http.md)
- [Hosted HTTP and HTTPS](src/content/docs/hosted-http.md)
- [Mathematics](src/content/docs/math.md)
- [Memory and foreign calls](src/content/docs/memory-and-ffi.md)
- [Networking](src/content/docs/networking.md)
- [Ownership](src/content/docs/ownership.md)
- [Platform adapters](src/content/docs/platform.md)
- [Processes](src/content/docs/processes.md)
- [Synchronization](src/content/docs/synchronization.md)
- [Threads](src/content/docs/threads.md)
- [Time and clocks](src/content/docs/time.md)
- [TLS](src/content/docs/tls.md)
- [Web applications](src/content/docs/web.md)

The [homepage](src/content/docs/index.md) links the everyday tasks. The
[contributor guide](src/content/docs/contributing.md) documents the quickstart
structure, executable fences, local setup, inventory maintenance, and validation.
The concurrently added console guide is linked in the inventory and dedicated
**Standard library** sidebar group. Related first-program, ownership and
container-reference updates are included so the guides agree about the current
APIs and restrictions. Concurrent compiler, library, example and README edits
remain untouched in the shared working checkout.

[Navigation ordering](src/lib/docs.ts) and the [content schema](src/content.config.ts)
include the new group. The existing [documentation checker](../scripts/check_docs.py)
now checks complete, consistent navigation, duplicate and current-page links,
and source-package inventory coverage in addition to links, anchors, assets and
search. A deliberately broken sidebar target in an isolated build copy was
rejected by the checker.

## Stale statements and missing guidance corrected

- HTTP is implemented: the inventory now includes portable HTTP/1.1 and web
  routing, sockets/DNS, TLS, hosted URL clients, HTTPS, web serving and static files.
- Console output, OS clocks, UTF-8 file/environment/command conveniences, and
  shared owned strings are documented using the APIs in this checkout.
- Owned containers support shared references and shared-arena strings. Result,
  exclusive-reference, fixed-container and mutable-view restrictions remain
  explicit; ordinary raw/opaque-memory restrictions are preserved.
- Hosted support means x86-64 Linux GNU and Windows x64 MSVC/GNU. Portable
  WebAssembly/Cortex-M0 object emission is distinguished from hardware execution;
  atomics have their own target restrictions.
- The Python HTTP peer explicitly serves HTTP/1.1; its default HTTP/1.0 is rejected
  by Dodo. Fixed-port reuse failures, missing files, absent variables, full buffers,
  child failure, HTTP status handling and TLS verification recovery are explained.
- References to incremental client/server behavior now point to the preserved
  polling examples. The newly added hosted guide's incorrect `/docs/...` links
  were changed to site-relative Markdown links.

Detailed ownership, allocation, invalidation, byte/text, numerical and protocol
contracts follow the quickstarts. The large overview's core/allocation/I/O/text
contracts were relocated into the new guides, with old overview section anchors
retained as signposts. Each quickstart explains imports and storage on first use,
provides the run command, and states its stdout or what a silent exit 0 proves.
Local fixtures and peers require no public Internet service.

## Validation

Validation ran on x86-64 Linux GNU with the compiler built from the changing
checkout. OpenSSL-backed checks used matching OpenSSL 3.5.8 headers/libraries.

| Check | Result |
| --- | --- |
| `npm ci --prefix docs` and `npm run build --prefix docs` | Production site built successfully. |
| `python3 scripts/render_spec.py --check` | Generated specification downloads match. |
| `python3 scripts/check_docs.py` | 43 pages; navigation, all 113 source-module inventory entries, and 3,601 internal links/assets/search targets checked. |
| Repository source/example links | Every linked GitHub `blob/main` / `tree/main` path exists in this checkout. |
| `dodo test docs --doc`, O0 and O3 | 28 passed and one intentional ignored demonstration at each level. |
| `dodo test examples`, O0 and O3 | 23 passed and one intentional ignored demonstration at each level. |
| Copied Markdown quickstarts with local setup, O0 and O3 | 32 checks passed: file reads and missing/oversized files; absent/present/empty/oversized environment values; child execution and missing child; TCP connection and refusal; HTTP fetch and body limit; web 200/404; HTTPS with generated trust and the exact documented Python TLS peer. |
| Hosted guide and generated HTTPS programs, O0 and O3 | Copied HTTP client/server and `scripts/local_https.py` HTTPS client/server pairs passed at both levels. |
| `python3 scripts/test_http_web.py` | All 16 example/interoperability checks passed at O0/O3, including HEAD, chunks, malformed framing and disconnects. |
| `scripts/test_portable_stdlib.py` plus one direct retry | All 168 object combinations passed: 42 fixtures × WebAssembly/Cortex-M0 × O0/O3. |
| `cargo test --locked --test tls_library` | All five tests passed, including verified engine behavior, local Python TLS interoperability, allocation cleanup and transport borrowing. |
| `git diff --check -- docs scripts/check_docs.py` | No whitespace errors. |

Peer checks extract the documentation's Dodo fences rather than maintaining
separate copies. Like the repository HTTP/web runner, automated runs substitute
fresh loopback ports to avoid TIME_WAIT conflicts. The local setup commands and
expected outputs are included in the guides. Files, processes and TLS credentials
were created in temporary directories and removed after checks.

The full portable runner reached its 120-second per-command timeout while
compiling the large numerical reference fixture for Cortex-M0 at O3 during
concurrent tests. That exact object succeeded on a direct retry; the existing
runner checked all remaining fixtures, and the combined results cover every
current fixture/target/optimization combination. Object format was checked too.
Windows support is documented from the adapters and repository evidence; these
onboarding validations do not claim a new native-Windows execution run.

Before publishing the draft PR, the isolated documentation branch also passed a
fresh dependency install, production website build, specification-download check,
and documentation checker (43 pages and 3,601 links/assets/search targets).
Its Markdown examples passed again at O0 and O3 using the combined checkout's
compiler: 28 passed and one intentional ignored example at each level.

## Remaining implementation work

The requested everyday tasks now have runnable starting paths. Remaining limits
that documentation cannot remove include:

- Additional hosted OS/ABI adapters, especially macOS, AArch64 Linux and musl.
- Interruptible system DNS and preemption of synchronous callbacks/writers;
  hosted timeouts remain cooperative and are not a universal end-to-end deadline.
- Explicit listener address-reuse configuration for predictable rapid restarts.
- Result and exclusive-reference container elements, broader fixed-container
  element support, and mutable views of stored references.
- HTTP/2, HTTP/3/QUIC, WebSockets, JSON/TOML integration, general device drivers,
  and timezone/scheduler facilities beyond the current explicit providers.

No global allocator, automatic unbounded growth or implicit execution policy was
introduced to hide these limits.
