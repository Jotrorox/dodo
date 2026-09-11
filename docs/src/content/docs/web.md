---
title: "Web applications"
description: "Portable routing, checked streaming handlers, optional rooted files, and explicit server execution."
section: "Using Dodo"
order: 154
---

`std/web` provides routing, request context, structural handlers, middleware and
error-response policy. It imports no sockets, TLS, files, scheduler or clock.
`std/web/server` composes routing with the portable HTTP connection driver;
applications supply transport, execution, time and body consumers independently.

## Routing and decoding

Construct `Router.new(&routes)` from caller-owned `Route` entries, each with a
byte method, decoded UTF-8 pattern and application-selected integer ID. The router
borrows its table and pattern strings. Construction validates every route and
rejects duplicate method/pattern shapes, including `/:first` versus `/:second`.
Construction is quadratic in table size; matching scans the table with work
bounded by route count and path length. There is no allocation or hidden cache.

| Pattern | Meaning |
| --- | --- |
| `/users/new` | Literal match, including case and trailing slash |
| `/users/:id` | One nonempty segment |
| `/files/*rest` | Final wildcard: multiple segments or an empty remainder after `/files/` |

The first differing segment determines precedence: literal, parameter, wildcard.
Registration order has no effect. Path specificity is selected before method
matching: a less-specific route never overrides a more-specific path's method
restriction. Methods are case-sensitive. An explicit HEAD route wins; otherwise
HEAD falls back to GET with `Match.head = true`. Other methods need explicit
registrations. There is no automatic OPTIONS response or slash redirect.
`/files/*rest` matches `/files/`, but not `/files`. A path with no matching method
returns `MethodNotAllowed`; an unmatched path returns `NotFound`. Parameter names
contain ASCII letters, digits or underscore, and cannot repeat in a pattern.
`parameter()` returns offsets into the decoded path, not raw pointers.

Before routing an HTTP target, call `decode_path(target, output)`. It strips the
query and decodes percent escapes **once** into caller storage, respecting the
component distinction in [RFC 3986](https://www.rfc-editor.org/rfc/rfc3986.html).
It rejects malformed escapes, encoded slash/backslash, NUL and controls, invalid
UTF-8, fragments, backslashes and `.`/`..` segments. `+` remains `+`. An escaped
percent remains literal: routing never decodes it again. Repeated and trailing
slashes remain significant. Patterns contain decoded UTF-8 literals, not percent
escapes. Insufficient output returns `BufferFull`; an error may leave a modified
prefix that must not be used as a path. `Router.find` accepts already-decoded
paths. The server composition also extracts paths from absolute-form HTTP targets;
its router does not implement authority/virtual-host selection or OPTIONS `*`.

## Handlers and middleware

`Context` contains borrowed method/path, route ID, explicit request ID and a
cancellation flag. Applications own additional state in handler structs.
`handle` and `dispatch` use ordinary generic methods, checked and monomorphized
by the compiler, without closures, runtime vtables or native function-pointer
casts. A handler implements `handle(context, body, output) -> u16!web.Error`.
The caller selects its body reader and response writer; no full buffering is
required. Serialization packages can integrate through `std/io` when supplied
by an application; none is imported implicitly.

`Chain<A,B>` invokes `before` outer-to-inner and `after` inner-to-outer. Failed
`before` short-circuits without calling the handler or any `after`. Handler failure
invokes `after` with status 500 and preserves the typed error. Cancellation before
dispatch invokes neither middleware nor handler. Middleware owns any logging,
authentication or serialization capabilities it needs.

Borrowed context, parser fields and body fragments cannot escape into handler
state. The checker rejects replacing borrow-carrying storage through a mutable
reference, including external callback receivers. Copy required bytes into
caller-backed or owned storage and retain lengths/offsets instead. Moving an
entire owned aggregate preserves its inferred or explicit `from(...)` sources.

`ErrorResponses` provides configurable routing-error statuses (defaults 400, 404,
405, 500). Applications select response content and headers, including `Allow`
when publishing 405. No internal error, path or credential is automatically
formatted into a public response. `std/web/response.Builder` validates a status
and selected prefix of preinitialized caller header slots, then builds a borrowed
`http.Response`. Its body describes framing; it does not buffer content.

## Streaming composition and execution

`std/http/connection.Connection` owns borrows of three caller buffers: parser
workspace, input and output. `poll_event` and `poll_flush` perform at most one
provider I/O operation per step. A body fragment must be consumed before advancing.
`body()` returns a checked view; after a sink accepts a prefix, release the view
and call `consume_body(count)`. The borrow checker prevents refilling storage while
that view remains live. `send_body` copies accepted bytes into output; flush them
before encoding another fragment. `finish_body` writes final chunks/trailers or
validates the promised length. Dropping discards pending data without hidden I/O.

`std/web/stream.PendingBody` applies the same discipline to any polling writer.
It borrows the source until delivered, and one `step` makes at most one writer
call. Pending means backpressure, not EOF or a retry loop.

`std/web/server.Application` combines routing, caller path storage and the HTTP
driver. Streaming handlers implement `head(&mut Context)`, `poll_write(body)`
and `complete(&mut Connection)`. Head dispatch happens only after all headers
pass protocol validation. Bodies go directly to the handler's polling sink.
Completion queues a response, which the caller streams and flushes. Observers
receive numeric IDs, accepted-byte counts and head/completion/failure events;
they own their sinks. Routing errors disable reuse and permanently stop that
Application before it can deliver bytes to a stale handler. An application may
queue a configured error response, flush it, and close the transport.

Every execution loop supplies absolute monotonic milliseconds and cancellation
to `Connection.check` before each step. `std/http/server.Budget` provides header,
body, response and idle phases and deadline calculations. `Limits` contains
phase timeouts, protocol bounds, connection capacity and requests per connection.
Defaults are 128 connections, 100 requests/connection, 10 s headers, 30 s body/
write/shutdown, and 15 s idle. The caller applies these settings and chooses phase
transitions. Protocol buffers never grow, and no thread or queue is created.

`std/http/server.Server` controls admission and graceful shutdown. Admit before
owning an active connection; release exactly once on every exit path. After
`begin_shutdown`, stop accepting and finish active responses. Close remaining
transports when `must_close(now)` becomes true. `drained()` confirms all admissions
were released. The gate requires exclusive mutable access; concurrent execution
must select explicit synchronization. It is not an implicit worker pool.

The [web server example](https://github.com/Jotrorox/dodo/blob/main/examples/web_server.dodo)
demonstrates bounded serial execution: one connection, fixed buffers, a 30 s
absolute deadline, routing/error responses, streaming, TCP half-close and cleanup.
The [client](https://github.com/Jotrorox/dodo/blob/main/examples/http_client.dodo)
consumes its response with a five-second deadline. Run these in two terminals:

```sh
dodo run examples/web_server.dodo
dodo run examples/http_client.dodo
```

The server exits after one request; restart it for another. An ordinary HTTP client
can also request `http://127.0.0.1:8080/`. The
[in-memory example](https://github.com/Jotrorox/dodo/blob/main/examples/http_memory.dodo)
parses a fragmented chunked request without any OS imports.

## Optional static files

`std/web/static_files` independently imports `std/fs` and a native rooted-open
adapter. Open an explicit `Root` with `Symlinks.Reject`, then supply an
already-decoded absolute URL path to `open_file`. The result is a regular
`fs.File`, suitable for streaming through caller scratch. There is no directory
listing, implicit index lookup, MIME database, range cache or full-file allocation.

Root selection resolves the caller's filesystem path, including ancestor links,
but rejects a link/reparse point at the final component. The opened directory is
the authority for requests. Request traversal opens one component at a time
relative to held directory handles and rejects every symlink/reparse point.
Linux uses `openat` with `O_NOFOLLOW`; Windows uses handle-relative `NtCreateFile`
and denies directory write/delete sharing while resolving. Final handles must be
regular disk files. URL paths additionally reject percent signs, colon/alternate
streams, empty internal segments, trailing dot/space and dot segments. Handles
close on all failure paths and deterministic drop. Linux paths are limited to
4095 bytes; Windows conversion uses bounded 32768-unit stack workspace. Content
may still change through independent writers; rooted opens do not promise a
snapshot or prohibit hard links.

## Verification and scope

Tests run routing, middleware, backpressure, compiler rejection, static boundaries
and HTTP composition at O0/O3. Portable fixtures emit WebAssembly and Cortex-M0
objects. Loopback checks use independent Python HTTP decoding, Dodo client/server
examples, byte-fragmented chunks, HEAD, 404, malformed framing and disconnects.
Wine executes Windows x64 HTTP peers and real Windows symlink fixtures at O0/O3.
Native Windows filesystem/sharing behavior still merits verification.

HTTP/2, HTTP/3/QUIC, WebSockets, cookies, multipart, compression and serialization
integrations are separate extension work. HTTPS uses an independently supplied
[TLS transport](tls.md); HTTP types and routing never select it implicitly.
