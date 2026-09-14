---
title: "Hosted HTTP and HTTPS"
description: "Small bounded clients and serial web servers with library-owned I/O loops."
section: "Using Dodo"
order: 154
---

Use `std/http/hosted` for a hosted HTTP client, `std/http/https` for verified
HTTP/HTTPS, and `std/web/hosted` for a serial web server. These modules own the
ordinary resolution, connect, readiness, framing, body transfer, and cleanup
loops. The portable `std/http`, `std/http/client`, `std/http/connection`,
`std/web`, and `std/web/server` APIs remain independently usable.

## Quickstart

This GET streams to the existing console writer. Non-2xx statuses are valid
HTTP responses; the application decides whether they are successful. Save it as
`get_example.dodo`. `hosted.WORKSPACE_BYTES` is fixed caller-owned protocol
storage; `console` chooses the body sink and `hosting` names the error type.
The hosted APIs support Linux GNU x86-64 and Windows x64 MSVC/GNU.

```dodo
package get_example
import "std/console"
import "std/http/hosted"
import "std/http/hosting"

fn run() -> i32!hosting.Error {
    workspace := [0u8; hosted.WORKSPACE_BYTES]
    client := hosted.Client.new(&mut workspace, hosted.Config.defaults())?
    output := console.stdout()
    response := client.get_to(b"http://localhost:8080/", &mut output)?
    if response.status != 200 { return ok(2) }
    return ok(0)
}
fn main() -> i32 {
    match run() {
        ok(code) => { return code }
        err(reason) => {
            errors := console.stderr()
            match errors.println_value(&reason) { ok(_) => {} err(_) => {} }
            return 1
        }
    }
}
```

Save this companion as `server_example.dodo`; it needs no files, credentials,
external libraries beyond the target C runtime, or Internet service. The route
and text literal are borrowed; the three arrays bound protocol and body storage:

```dodo
package server_example
import "std/console"
import "std/web"
import "std/web/hosted"

fn main() -> i32 {
    routes := [web.Route { method: b"GET", pattern: b"/", id: 0 }]
    handler := hosted.Text.new(b"Hello, Dodo!\n")
    workspace := [0u8; hosted.WORKSPACE_BYTES]
    request := [0u8; 4096]
    response := [0u8; 4096]
    match hosted.serve(b"127.0.0.1:8080", &routes, &mut handler,
        &mut workspace, &mut request, &mut response, hosted.Config.defaults()) {
        ok(report) => { if report.failed != 0 { return 2 }; return 0 }
        err(reason) => {
            errors := console.stderr()
            match errors.println_value(&reason) { ok(_) => {} err(_) => {} }
            return 1
        }
    }
}
```

In terminal 1, run the copied server:

```sh
dodo run server_example.dodo
```

It waits silently. In terminal 2, run the copied client:

```sh
dodo run get_example.dodo
```

Expected client stdout is `Hello, Dodo!` followed by a newline, with exit 0.
The server keeps accepting until stopped with Ctrl+C. If the client races server
compilation and reports connection refused, retry when the server is running.
If bind fails, check for another listener or a recently closed port; use a fresh
port in both examples if necessary. A failed request exits 1 and prints its
error on stderr; check the cause and delivered-prefix count before retrying.
A completed non-200 response exits 2. The server reports individual failed
connections in its `Report`, independently of startup errors.

For the underlying protocol APIs, see [HTTP](http.md) and [web serving](web.md). `Config.max_connections` sets a finite number of
accepted connections (including rejected requests). Repository versions are
[http_client.dodo](https://github.com/Jotrorox/dodo/blob/main/examples/http_client.dodo)
and [web_server.dodo](https://github.com/Jotrorox/dodo/blob/main/examples/web_server.dodo).
The previous incremental programs remain as
[http_client_polling.dodo](https://github.com/Jotrorox/dodo/blob/main/examples/http_client_polling.dodo)
and [web_server_polling.dodo](https://github.com/Jotrorox/dodo/blob/main/examples/web_server_polling.dodo).

## Client API and storage

| API | Purpose |
| --- | --- |
| `hosted.Client.new(workspace, config)` | Reusable context borrowing explicit storage |
| `client.get(url, storage)` | Collect decoded body bytes into bounded caller storage |
| `client.get_to(url, writer)` | Stream to any ordinary `std/io` writer |
| `client.request(&request, writer)` | General method, headers, and byte-slice request body |
| `client.header(name, occurrence)` | Borrow a retained final response header; duplicates are preserved |
| `client.request_with(request, writer, resolver, connector, clock, cancel)` | Inject execution capabilities |
| `https.Client.new(workspace, config, trust)` | Same convenience API, accepting HTTP and HTTPS |
| `https.Client.request_with(request, writer, resolver, clock, cancel)` | Inject resolution/cancellation while keeping verified TLS |

`hosted.Request` has `url`, `method`, `headers: &[http.Header]`, and `body: &[u8]`.
For example, a general request inside a function returning `!hosting.Error`:

```dodo
headers := [http.Header { name: b"Content-Type", value: b"text/plain" }]
request := hosted.Request {
    url: b"http://localhost:8080/echo", method: b"POST",
    headers: &headers, body: b"hello"
}
body := [0u8; 1024]
writer := io.MemoryWriter.new(&mut body)
response := client.request(&request, &mut writer)?
```

Import `std/http` and `std/io` for this fragment. `Response` contains `status`,
`body_bytes`, and `redirects`. For collection, `storage[..response.body_bytes as
usize]` is the initialized body. `hosting.Error.delivered` counts the sink's
accepted prefix on failure; it is not an acknowledgement by a downstream device.
An overfull collection fails `BodyLimit`, retaining its accepted prefix.
A failed request clears retained headers. Successful header views last until
the next request and keep the client borrowed. Informational heads and trailers
are parsed and validated but are not returned as final headers.

The client requires 53,248 workspace bytes: 16 KiB parser, 4 KiB input, 16 KiB
output, and 16 KiB retained headers. It never grows them. Per-call stack scratch
also includes two 4 KiB URL buffers, a 4 KiB target, 16 header slots, and 384
resolver bytes (at most 16 addresses). Ordinary stack usage and platform
resolver/socket allocations are separate from workspace capacity. HTTPS adds
two 4 KiB staging buffers and OpenSSL allocations. Context reuse saves workspace;
each exchange uses a fresh connection, closed on all normal/error exits.

Defaults and hard limits:

| Setting | Default / cap |
| --- | --- |
| `header_bytes`, `header_fields` | 16,384 / 100; configurable downward |
| Response `body_bytes` | 16 MiB; configurable, decoded bytes |
| `request_body_bytes` | 16 MiB; configurable |
| URL / hostname | 4,096 / 253 bytes |
| Caller request headers | 14, plus generated Host and Connection |
| Informational responses | 16 per exchange |
| Chunk line / trailers | 1,024 bytes / 4,096 bytes and 32 fields |
| `max_redirects` | 0; configurable through 16 |
| Automatic failed-exchange retries | 0 |

Absolute ASCII `http://` and `https://` URLs support DNS A-labels, IPv4, bracketed
IPv6, explicit ports, empty paths, and queries. Userinfo, fragments, scoped
literals, percent-encoded hostnames, Unicode hostnames, and port zero are
rejected. Host includes the original authority, including IPv6 brackets and an
explicit port. Origin-form request targets omit the authority. Content-Length
is generated from the byte-slice body, including zero. Caller Host/framing,
Connection, Expect, Upgrade, Trailer, TE, and proxy-authorization fields are
rejected before sending. CONNECT and protocol upgrades are unsupported.

Redirects are off by default; 3xx responses then return normally, including their
bodies. When enabled, only absolute URLs and origin-relative `/path` locations
within the same origin are followed. Cross-origin redirects, HTTPS downgrades,
missing/unsupported locations, and hop exhaustion fail `Redirect`. Intermediate
bodies are closed without delivery. The portable redirect policy determines
method handling: 301/302/307/308 preserve method/body; 303 becomes GET except
for HEAD. Every supplied body is a replayable byte slice. No streaming source is
rewound and no failed exchange is retried, even for GET. Address fallback occurs
only before any HTTP request bytes are sent. There are no ambient proxies,
cookies, decompression, connection pools, or credential forwarding across origins.

## Precisely scoped timeouts and cancellation

Client configuration uses nonzero millisecond durations:

* `resolution_timeout_ms` (10,000) is a separate budget passed to the resolver.
  It is checked immediately before and after resolution.
* `request_timeout_ms` (30,000) starts **after successful resolution**, separately
  for each redirect hop. It covers TCP connect, TLS construction/handshake,
  request writes, response headers/body, sink backpressure, and TLS close-notify
  delivery. It does not reset on progress.
* `connect_timeout_ms` (10,000) caps TCP address attempts together and cannot
  extend the request deadline. TLS runs within the remaining request budget.

**The platform resolver is synchronous and cannot be cancelled mid-call.** A
resolution timeout/cancellation is reported after that call returns. A hung
platform resolver can therefore keep the hosted call blocked indefinitely.
These settings are not an enforceable end-to-end URL deadline. A redirect has
another resolution call and another network budget. The platform resolver may
allocate and returns at most the first 16 addresses; additional addresses are
not attempted.

Inject a resolver with this structural method:

```dodo
resolve<C, X>(&mut self, host: &[u8], port: u16, workspace: &mut[u8],
    operation: &net.Operation, clock: &mut C, cancel: &X)
    -> Addresses!net.Error from(workspace)
```

Its result implements `len()` and `get(index) -> net.SocketAddress!net.Error`.
It may use its own result type and borrowed workspace. Return at most 16
addresses and retain no background work, input borrow, or operation after return.
For bounded/cancellable resolution, the provider must honor the supplied
operation during its own I/O. The client also checks that operation after return.
For example a numeric/cached resolver can use `native.resolve_numeric`; the
integration fixture includes a deliberately blocking injected resolver proving
the default scope. A custom connector implements the public `PlainConnector`
contract; `https.Connector` supplies the verified implementation.

`clock.now_ms()` must be monotonic. `cancel.cancelled()` is checked before I/O
and waits; cancellation wins over timeout. Native readiness waits are at most
10 ms, and writer backpressure uses 1 ms sleeps. Deadlines are inclusive; OS
scheduling can delay observations. Ordinary synchronous writers, application
handlers, resolver implementations, and backend construction calls cannot be
preempted by these loops. Checks occur between calls. A blocking console/file
writer can thus exceed a deadline too; use a nonblocking writer when bounded
cancellation is required. Zero timeout durations are rejected.

## Serial server, handlers, and shutdown

`web/hosted.serve(address, routes, handler, workspace, request_body,
response_body, config)` parses a numeric bind address, validates the router,
binds one listener, and accepts repeated connections. It handles one request
per connection and sends `Connection: close`. There is no per-peer thread,
queue allocation, keep-alive pipeline, or unbounded task creation. The OS listen
backlog defaults to 16. A slow client occupies the single execution lane until
its limit, deadline, or cancellation fires.

The server uses the existing ordinary structural interface:

```dodo
handle<B, W>(&mut self, context: &mut web.Context,
    body: &mut B, output: &mut W) -> u16!web.Error
```

The supplied body is an `io.MemoryReader` over the complete bounded decoded
request; output is an `io.MemoryWriter` over bounded response storage. The
handler returns the status. `hosted.text(output, bytes)` writes text and returns
200; `hosted.Text.new(bytes)` supplies a complete constant-text handler. Existing
handlers using `web.handle` work directly. Response Content-Type is currently
`text/plain; charset=utf-8`. Applications needing arbitrary response headers,
streaming request callbacks (`head`/`poll_write`/`complete`), streaming response
production, middleware-controlled I/O, upgrades, or concurrency should keep
using the independently available portable application/connection drivers.

The fixed server workspace is 38,912 bytes (16 KiB parser, 4 KiB input, 16 KiB
output, 2 KiB decoded path). Body arrays are separate and explicit. Request and
response capacities are the smaller of the supplied arrays and the configured
`body_bytes` / `response_bytes`, each defaulting to 65,536. Headers default to
16,384 bytes and 100 fields. Overlarge requests get 413, oversized heads 431,
malformed framing/path 400, absent routes 404, wrong methods 405, and handler
failures/response-storage exhaustion 500. Errors have empty bodies. HEAD runs
the same handler and emits the corresponding Content-Length, without body
bytes. 204 and 304 also suppress body bytes. Valid `Expect: 100-continue` is
answered before body reading. Malformed framing poisons the protocol driver;
error responses use a separate fresh serializer. After final response output
starts, any failure closes the connection without another status line.

Server timeouts are nonzero and absolute within their scopes:

* `header_timeout_ms` (10,000) starts at accepted plaintext request processing.
* `body_timeout_ms` (30,000) starts when headers complete.
* `request_timeout_ms` (30,000) spans plaintext headers, body, handler, and output,
  capping the phase deadlines without resetting on progress.
* `write_timeout_ms` (30,000) caps response output. An input timeout may generate
  408 with a **new** bounded error-write interval; total connection time may thus
  include that extra interval. Cancellation skips error writing.
* HTTPS handshakes have a separate `header_timeout_ms` budget before plaintext
  processing. Successful TLS shutdown has a separate write budget and delivers
  our close_notify without waiting indefinitely for the peer's notification.

`serve_with(..., config, acceptor, clock, cancel)` exposes cooperative shutdown;
use `PlainAcceptor` or `https.Acceptor`. Any provider with
`cancelled(&self) -> bool` works, including a shared atomic provider from
`std/sync/allocated` for another thread or a clock-based provider. It stops
acceptance and aborts the active exchange. An already-running synchronous
handler must return before shutdown is observed. All sockets/listener and TLS
engines drop before return. `Report` gives accepted, completed, rejected, failed,
and cancelled counts; client protocol failures do not terminate the listener.
`max_connections` is an additional explicit finite shutdown condition. The
integration server fixture demonstrates deadline-triggered cancellation while
a peer is holding an incomplete request.

## Complete local HTTPS setup

The HTTPS import selects the existing OpenSSL backend and requires **OpenSSL
3.5+ development headers and libraries**, plus a target C toolchain. Hosted
linking adds libssl/libcrypto. Matching dynamic libraries/provider modules must
be available at runtime. This is also required if an HTTPS-enabled program only
uses an HTTP URL. Plain `http/hosted` does not link OpenSSL.

`https.Trust.system()` uses OpenSSL's configured default paths; on Windows this
does **not** import the Windows certificate store. `https.Trust.pem_roots(pem)`
uses only explicit PEM roots. A `Trust` value can enable both modes. Every HTTPS
connection verifies the chain, validity, purpose, and URL hostname/IP SAN and
uses DNS SNI. ALPN offers only HTTP/1.1; no ALPN is accepted as HTTP/1.1. There is
no insecure verification switch. See [TLS](tls.md) for backend bounds and
revocation limitations. OpenSSL internal heap use is not a hard-bounded arena;
its allocation failures remain typed errors.

From the repository root, create a new example directory and fresh local
credentials. These commands use Python 3, OpenSSL and Dodo on PATH:

```sh
python3 scripts/local_https.py build/local-https
cd build/local-https
dodo build https_server.dodo -o server.exe
dodo build https_client.dodo -o client.exe
./server.exe
# In another terminal, from the same directory:
./client.exe
# Independent client with explicit trust:
curl --cacert ca.pem https://localhost:8443/
```

Both clients print `Hello, Dodo!` followed by a newline and exit 0. A connection
or verification failure exits nonzero; check the listener, explicit trust file,
certificate identity and expiry. Stop the server with Ctrl+C before removing
the generated directory, including its test private keys. The optional `curl`
command requires curl; the Dodo client has no curl dependency.

The script runs reproducible OpenSSL commands to generate a fresh local CA and
one-day DNS-only localhost leaf, saves private keys with restricted permissions,
and copies the complete `examples/https_client.dodo` and `https_server.dodo`
programs. Credential bytes intentionally vary on every run. The examples read
bounded PEM files at runtime, handle all Results, and use `https.serve` with an
explicit `openssl.Config.server(certificate, key)`. Use `localhost`, not
`127.0.0.1`, with this DNS-only certificate. Re-run the generator into a new
directory after expiry. Stop the demonstration listener with Ctrl-C; applications
can use the cooperative `serve_with` API described above.

## Validation and platform limits

`python3 scripts/test_hosted_http.py` (also run by `cargo test --test
http_hosted_library`) generates local credentials, compiles at O0 and O3, and
checks independent Python HTTP/TLS peers. It covers informational responses,
fragmented chunk framing/trailers, partial writes and backpressure, bounded
collection, malformed framing/EOF, body/header/time limits, redirect method and
body handling, chain/hostname rejection, repeated requests, cancellation, and
socket cleanup. The existing HTTP/web, TLS, and Windows harnesses continue to
exercise the lower layers. `--skip-tls` is an explicit reduced test selection.

Supported hosted adapters are x86-64 Linux GNU and Windows x64 MSVC/GNU.
Linux musl/x32, AArch64 Linux, macOS and freestanding ABIs are unsupported by
these hosted adapters. Native Windows TLS uses
OpenSSL; Wine checks cannot prove installed trust paths or native entropy and
provider behavior. These conveniences are HTTP/1.1 only. Platform DNS and
application callbacks have the blocking limitations above. The existing native
listener does not enable address reuse: after closing active connections,
TCP TIME_WAIT can prevent immediate rebinding of the same port even though all
listener/connection descriptors were released.
