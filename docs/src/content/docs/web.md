---
title: "Web applications"
description: "Portable routing, checked streaming handlers, optional rooted files, and explicit server execution."
section: "Standard library"
order: 160
---

Use `std/web/app.Server` to run a small HTTP application on Linux GNU x86-64
or Windows x64. Register named handler structs with `std/web/application`;
route IDs and bounded default buffers are supplied by the library. The portable
registration package and `std/web` do not import sockets or start a server.

## Quickstart

Save this as `serve_route.dodo`:

```dodo
package serve_route
import "std/console"
import "std/web"
import "std/web/application"
import "std/web/app"

pub struct Hello {
    pub fn handle(&mut self, request: &mut web.Request, response: &mut web.Response) -> void!web.Failure {
        return response.text(b"Hello, Dodo!\n")
    }
}
fn main() -> i32 {
    routes := application.new().get(b"/", Hello {})!
    server := app.Server.new(routes)
    server.config.max_connections = 1
    match server.serve(b"127.0.0.1:8080") {
        ok(report) => {
            if report.failed != 0 {
                return 2
            };
            return 0
        }
        err(reason) => {
            errors := console.stderr()
            match errors.println(&reason) {
                ok(_) => {}
                err(_) => {}
            }
            return 1
        }
    }
}
```

```sh
dodo run serve_route.dodo
```

The server waits silently for one connection. In terminal 2:

```sh
python3 -c "import http.client; c = http.client.HTTPConnection('127.0.0.1', 8080, timeout=5); c.request('GET', '/'); r = c.getresponse(); print(r.status); print(r.read().decode(), end=''); c.close()"
```

The client prints `200` and `Hello, Dodo!`; the server exits. Set
`server.config.max_connections = 0` (the default) to keep accepting connections.
Ctrl+C stops the process. For cooperative shutdown that returns a report, use
`serve_until(address, cancel)` with a public `cancelled(&self) -> bool` method.
`max_connections` counts accepted connections, including rejected requests.

A successful serving Result contains a `hosted.Report`: inspect `completed`,
`rejected`, `failed`, `accepted`, `cancelled`, and `last_error`. For example, a
request to `/missing` gets 404 and increments `rejected`. Startup errors return
`hosting.Error`. The example prints these errors and exits 1; connection failures
make it exit 2. Binding does not enable address reuse; a recently closed port
may need time to become reusable, or choose a different port.

## Several routes

[`examples/web_routes.dodo`](https://github.com/Jotrorox/dodo/blob/main/examples/web_routes.dodo)
registers three different named handler types:

```dodo
package web_routes
import "std/web"
import "std/web/application"
import "std/web/app"

pub struct Home {
    pub fn handle(&mut self, request: &mut web.Request, response: &mut web.Response) -> void!web.Failure {
        return response.html(b"<!doctype html><h1>Dodo</h1><a href=\"/hello/Dodo\">Say hello</a>")
    }
}
pub struct Greeting {
    pub fn handle(&mut self, request: &mut web.Request, response: &mut web.Response) -> void!web.Failure {
        match request.parameter(b"name") {
            some(name) => { return response.text(name) }
            none => { return err(web.failure(web.Error.NotFound)) }
        }
    }
}
pub struct Echo {
    pub fn handle(&mut self, request: &mut web.Request, response: &mut web.Response) -> void!web.Failure {
        return response.bytes(request.body())
    }
}
fn main() -> i32 {
    routes := application.new()
        .get(b"/", Home {})!
        .get(b"/hello/:name", Greeting {})!
        .post(b"/echo", Echo {})!
    server := app.Server.new(routes)
    match server.serve(b"127.0.0.1:8080") {
        ok(report) => { if report.failed != 0 { return 2 }; return 0 }
        err(_) => { return 1 }
    }
}
```

`get(pattern, handler)`, `post(pattern, handler)`, and
`route(method, pattern, handler)` consume the previous application and return a
Result containing the extended application. These examples unwrap literal
registration errors with `!`; use `?` or `match` for fallible setup. Registration
checks patterns, methods, ambiguous routes, and the eight-route
`application.ROUTE_CAPACITY` bound. A ninth registration returns
`web.Error.BufferFull`. Route IDs are assigned in registration order; handlers
use request parameters and metadata without an ID dispatch switch. Literal
precedence, parameter/wildcard matching, HEAD fallback, and 404/405 behavior
come from the existing router.

Handlers are owned inline in a generic chain and keep their state between
requests. Dodo supports named structs with public `handle` methods; it has no
closures or ordinary function values. The bounded chain suits small applications;
deeply nested generic types are expensive in the current compiler. Larger route
tables can use `web.Router` and the existing runners with their own dispatcher.
Route patterns and any handler borrows must outlive the application.

`Server.new(application)` owns the application and configuration. Serving consumes
the server, unpacks routes and handlers into separate local values, and drops
handler state when serving returns. This accommodates Dodo's conservative
borrowing rules without pointer erasure or callbacks retaining request storage.
Borrow external state in a handler when it must remain available after serving.

## Configuration and storage

`app.Config.defaults()` is grouped by purpose:

| Setting | Default |
| --- | --- |
| `config.execution` | `app.Execution.Serial`: one request per connection, one socket at a time |
| `config.limits.header_bytes`, `header_fields` | 16,384 bytes, 100 fields |
| `config.limits.body_bytes`, `response_bytes` | 4,096 bytes each |
| `config.limits.path_bytes` | 2,048 bytes |
| `config.limits.query_bytes`, `query_fields` | 2,048 bytes, 100 pairs |
| `config.limits.response_header_bytes`, `response_header_fields` | 4,096 bytes, 32 fields |
| `config.timeouts.header_ms` | 10,000 ms |
| `config.timeouts.body_ms`, `request_ms`, `write_ms` | 30,000 ms each |
| `config.timeouts.idle_ms` | 15,000 ms, concurrent mode only |
| `config.backlog`, `max_connections` | 256; zero (accept until cancelled) |
| `config.requests_per_connection`, `events_per_turn` | 100; 16, concurrent mode only |

Each request/response body limit is the smaller of the configured value and available storage.
Raising a limit does not enlarge a buffer. Protocol/path/query/header hard caps
are the same as [the underlying hosted runner](hosted-http.md#serial-server-handlers-and-shutdown).
Invalid settings fail before binding. Concurrent-only settings are validated
when concurrent execution is selected. Zero body capacities allow empty bodies;
zero query or response-header capacities disable those fields. All active
timeouts, backlog, request counts, and event budgets must be nonzero.

`serve(address)` and `serve_until(address, cancel)` supply fixed local buffers:
68 KiB for serial execution (`app.WORKSPACE_BYTES` is 61,440 protocol bytes, plus two 4 KiB body buffers),
or 544 KiB for eight concurrent slots. Handler values, route metadata, slot
state, the route index, and stack frames add bounded storage. There is no heap
allocation, buffer growth, per-peer thread, or task queue in this layer. Ensure
the serving thread has enough stack, especially in concurrent mode; custom
storage lets the caller choose where buffers live.

`serve_in(address, storage)` uses caller storage and the native clock.
`serve_with(address, storage, clock, cancel)` also accepts a monotonic
`now_ms(&mut self) -> u64` clock and cooperative cancellation. Both consume the
server and borrow the storage for the call. The storage wrapper retains its
buffer borrows until dropped. For example:

```dodo
package custom_storage
import "std/web"
import "std/web/application"
import "std/web/app"
import "std/web/hosted"
import "std/net"
import "std/net/native"

fn main() -> i32 {
    routes := application.new().get(b"/", hosted.Text.new(b"Hello!"))!
    server := app.Server.new(routes)
    server.config.limits.body_bytes = 8192
    server.config.limits.response_bytes = 8192
    workspace := [0u8; hosted.WORKSPACE_BYTES]
    request := [0u8; 8192]
    response := [0u8; 8192]
    storage := app.Storage.new(&mut workspace, &mut request, &mut response)
    clock := native.MonotonicClock {}
    cancel := net.Cancellation.new()
    cancel.cancel() // Demonstrate returning without accepting requests.
    report := server.serve_with(b"127.0.0.1:8080", &mut storage, &mut clock, &cancel)!
    assert(report.cancelled)
    return 0
}
```

For custom concurrency, select `app.Execution.Concurrent` and use
`app.Storage.concurrent(workspace, request, response, slots)`. Supply 1–255
`reactor.Slot.new()` values and at least `hosted.WORKSPACE_BYTES` per slot.
Request and response buffers are divided equally among slots; trailing remainders
are unused. A concurrent server given serial storage returns a workspace error.
The slot count, rather than `max_connections`, bounds simultaneous connections.

## Hosted server choices

The server object uses the existing serial `std/web/hosted` and concurrent
`std/web/reactor` runners. Those lower-level functions remain available for
custom dispatchers and transport adapters. HTTPS continues to use the explicit
TLS acceptor. Streaming bodies, upgrades, and caller-driven execution use
`std/web/server`, `std/web/stream`, `std/web/response`, and
`std/http/connection`; none acquire a hosted dependency. Rooted static files
remain an optional `std/web/static_files` import.

## Concurrent HTTP/1.1 server

`std/web/reactor` lets other connections advance while a socket waits for input
or output. It uses one thread, bounded caller-owned storage, and native readiness
polling. It supports HTTP keep-alive and ordered pipelined requests without an
allocator, external framework, or task runtime.

The complete example is
[`examples/web_server_concurrent.dodo`](https://github.com/Jotrorox/dodo/blob/main/examples/web_server_concurrent.dodo):

```sh
dodo compile examples/web_server_concurrent.dodo -O 3 -o build/web-server
./build/web-server
```

Select concurrent execution on the same server object:

```dodo
package web_server_concurrent
import "std/console"
import "std/web"
import "std/web/application"
import "std/web/app"

pub struct Hello {
    pub fn handle(&mut self, request: &mut web.Request, response: &mut web.Response) -> void!web.Failure {
        return response.text(b"Hello, Dodo!\n")
    }
}
fn main() -> i32 {
    routes := application.new().get(b"/", Hello {})!
    server := app.Server.new(routes)
    server.config.execution = app.Execution.Concurrent
    match server.serve(b"127.0.0.1:8080") {
        ok(report) => {
            if report.failed != 0 {
                return 2
            };
            return 0
        }
        err(reason) => {
            errors := console.stderr()
            match errors.println(&reason) {
                ok(_) => {}
                err(_) => {}
            }
            return 1
        }
    }
}
```

The lower-level reactor also accepts custom slot arrays and configuration:

`slots()` constructs eight `Slot.new()` values. The slot count bounds active
connections; applications may supply another array or owned collection with
1–255 slots. Allocate `WORKSPACE_BYTES * slots.len` workspace bytes. Request and
response arrays are divided equally between slots, with any trailing remainder
unused. The example supplies 4 KiB per request and response, totaling 544 KiB
of byte buffers for eight connections. Slots, the route index, and stack frames
add a separate bounded amount of storage.

| Setting | Default and meaning |
| --- | --- |
| `config.server` | The same header/body/request/write limits as `hosted.Config`; backlog 256. |
| `config.server.max_connections` | Zero serves until cancelled; a positive value stops acceptance after that many connections and drains active slots. It does not set concurrency. |
| `config.requests_per_connection` | 100; the final response advertises `Connection: close`. Set 1 to disable reuse. |
| `config.idle_timeout_ms` | 15,000; bounds waiting between requests. |
| `config.events_per_turn` | 16; bounds protocol work per ready slot before other slots run. |

All timeout durations, the request limit, and the event budget must be nonzero.
The first header deadline starts on acceptance. Reused connections start a new
request budget when their next bytes arrive or are processed from buffered
pipeline input. Bodies remain buffered before dispatch. HEAD, 204, 304,
`100-continue`, size limits, and error responses follow the hosted server rules.
Malformed requests and handler failures close their connection after the error
response; a failure after final output starts closes without a second response.

`serve_with(..., config, clock, cancel)` accepts explicit clock and cancellation
providers. Cancellation closes all active sockets, including partially read
requests. `Report.completed` counts delivered successful dispatches, so it can
exceed `Report.accepted` with keep-alive. Rejected dispatches and failed
connections have separate counters. Idle expiration and clean keep-alive EOF
are normal closure.

Handlers still run synchronously on the serving thread. A long-running handler
blocks that thread until it returns; this API provides concurrent socket progress,
not parallel application callbacks. Slots, bodies, and reuse remain bounded.
The reactor serves plain HTTP; the existing explicit TLS acceptor remains
available through `std/http/https` and the serial hosted API.

## API and contracts

`std/web` provides routing, request context, structural handlers, middleware and
error-response policy. It imports no sockets, TLS, files, scheduler or clock.
`std/web/server` composes routing with the portable HTTP connection driver;
applications supply transport, execution, time and body consumers independently.

## Routing and decoding

Construct `Router.new(&routes)` from caller-owned `Route` entries, each with a
byte method, decoded UTF-8 pattern and application-selected integer ID. The router
borrows its table and pattern strings. Construction validates every route and
rejects duplicate method/pattern shapes, including `/:first` versus `/:second`.
Sorted tables containing only literal paths use binary search. Other tables use
one scan that selects path specificity and method together. Construction of an
unsorted or dynamic table with `Router.new` checks ambiguity pairwise.

`Router.indexed(&routes, &mut index)` adds an allocation-free hash index for
literal paths. Supply at least twice as many `usize` slots as routes. Construction
checks duplicates while inserting literals and compares dynamic route shapes;
collisions always compare the complete path. Both the route table and index
remain borrowed by the router; failed construction may modify index storage.
Literal matches avoid scanning dynamic routes, preserving the same precedence
and method rules. Dynamic matches still scan. The serial hosted server selects
this index for 17–1,024 routes, and the reactor for up to 1,024 routes. Larger
tables use the ordinary router; explicitly supplied index storage has no such
route-count ceiling. There is no allocation or hidden cache.

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

Buffered handlers implement one ordinary, compiler-checked method:

```dodo
pub fn handle(&mut self, request: &mut web.Request,
    response: &mut web.Response) -> void!web.Failure
```

The serial, HTTPS and concurrent servers all use it. `web.handle(handler,
request, response)` also works without a server. A response starts at status
200 with no headers and an empty body. For example:

```dodo
pub struct Greeting {
    pub fn handle(&mut self, request: &mut web.Request,
        response: &mut web.Response) -> void!web.Failure {
        response.set_status(201)?
        response.header(b"Content-Type", b"application/json")?
        response.header(b"X-Request-Method", request.method())?
        return response.text(b"{\"hello\":\"Dodo\"}")
    }
}
```

Register this handler with `application.new().get(b"/hello/:name", Greeting {})!` and pass the application to `app.Server.new`. See the complete
[`web_response.dodo`](https://github.com/Jotrorox/dodo/blob/main/examples/web_response.dodo)
example for HTML and route/query access.

### Request access and decoding

| Method | Result |
| --- | --- |
| `request.method()` | Original, case-sensitive method bytes |
| `request.path()` | Validated UTF-8 path, percent-decoded once, without query |
| `request.body()` | Complete bounded body bytes, after HTTP transfer decoding |
| `request.header(name, occurrence)` | `Option<&[u8]>`; ASCII case-insensitive name |
| `request.query(name, occurrence)` | `Option<&[u8]>`; case-sensitive decoded name |
| `request.parameter(name)` | `Option<&[u8]>`; named route capture in the decoded path |

Occurrences start at zero. Headers and query pairs preserve duplicates in wire
order; lookup never combines comma-separated values or chooses a last value.
Missing values return `none`, while present empty values return `some(b"")`.
Headers retain their value bytes after the HTTP parser trims surrounding spaces
and tabs. Header values need not be UTF-8. Trailers are validated by HTTP but
are not exposed as request headers. Framing headers retain the HTTP parser's
strict duplicate rules. Bodies are arbitrary bytes; there is no automatic
JSON, form-body, multipart, charset or content-encoding conversion.

Queries split on `&`, then on the first `=`, before decoding. `%HH` is decoded
exactly once; `+` becomes space. Encoded `&`, `=`, `+`, `#` and `/` remain value
characters. Empty components between ampersands are skipped, a bare key has an
empty value, and an empty key is allowed. Malformed escapes, controls (including
NUL and DEL), fragments and invalid UTF-8 are rejected before dispatch.
Semicolons do not delimit pairs. `decode_query` percent-error positions refer to
the raw query; HTTP URI errors refer to the whole target. UTF-8 error positions
refer to the decoded key or value. Route captures are not
decoded again: `/users/a%252Fb` captures `a%2Fb`; encoded path separators such as
`%2F` are rejected. There is no normalization or Unicode case folding.

`route_id`, `request_id` and `cancelled` are public request metadata. The route
index used for captures is retained separately from the application route ID.
All accessor views borrow the request. Neither request views nor response
storage views may escape into handler state or survive a conflicting mutation.
The checker enforces this without unchecked pointers or callback lifetime casts.

### Response construction

| Method | Behavior |
| --- | --- |
| `response.set_status(code)` | Select a final status, 200–599 |
| `response.header(name, value)` | Copy and append a validated header |
| `response.text(bytes)` | Validate UTF-8 and copy the body; default type `text/plain; charset=utf-8` |
| `response.html(bytes)` | Validate UTF-8 and copy the body; default type `text/html; charset=utf-8` |
| `response.bytes(bytes)` | Copy arbitrary body bytes; add no Content-Type |
| `response.status()`, `response.body()` | Read status or borrow copied body bytes |
| `response.header_at(index)` | Borrow a copied header by insertion index |

Each body call replaces the previous body. Text/HTML add their default type only
when no Content-Type has been supplied; set a custom type before calling them.
A second Content-Type is rejected case-insensitively. Other duplicate response
headers are appended in insertion order, including separate Set-Cookie fields;
the application must respect each field's HTTP semantics. Header names must be
HTTP tokens; CR, LF, NUL and invalid field-value controls are rejected.

The runner generates Content-Length and Connection. Handlers cannot supply
Content-Length, Transfer-Encoding, Connection, Trailer, TE, Upgrade, Keep-Alive
or Proxy-Connection. HEAD invokes the selected handler and sends its headers
and representation length, without body bytes. 204 omits Content-Length; 205
sends length zero; 304 sends the buffered representation length. None of these
statuses sends body bytes. Informational responses and streaming framing remain
available through the lower-level connection API.

`Response.new(header_storage, header_field_limit, body_storage)` borrows two
caller-owned byte buffers. Mutators copy their arguments and retain no borrow
of handler locals or request data. Failed setters leave the existing response
unchanged. Propagate errors with `?` to discard the whole response; a handler may
also catch an error and deliberately build a smaller response.

`web.Failure` keeps a `kind: web.Error`, plus optional underlying `http.Error`
and `text.Error` diagnostics. Response capacity and header/status validation errors retain
protocol kind and position; invalid UTF-8 retains its exact text diagnostic.
Hosted failures retain this value in `hosting.Error.application`, and server
`Report.last_error` retains the latest observed connection failure. Public error
responses are empty; internal errors and request data are never formatted into
them. `serve_connection` returns the original error even when it successfully
sends an error response (`reason.responded == true`); the serial server counts
these as rejected connections. Inspect reports as well as startup Results.

`Chain<A,B>` invokes `before(request)` outer-to-inner and `after(request, status)`
inner-to-outer. `before` returns `void!web.Failure`; a failure short-circuits
without invoking the handler or `after`. Handler failure invokes `after` with
500 and preserves the failure. Cancellation before dispatch invokes neither.
Applications own any additional handler or middleware state.

The lower-level streaming `Context` remains separate. `ErrorResponses` maps its
routing errors, and `std/web/response.Builder` still builds borrowed HTTP framing
metadata from preinitialized header slots.

## Streaming composition and execution

`std/http/connection.Connection` owns borrows of three caller buffers: parser
workspace, input and output. `poll_event` and `poll_flush` perform at most one
provider I/O operation per step. A body fragment must be consumed before advancing.
`body()` returns a checked view; after a sink accepts a prefix, release the view
and call `consume_body(count)`. The borrow checker prevents refilling storage while
that view remains live. `send_body` copies accepted bytes into output; flush them
before encoding another fragment. `finish_body` writes final chunks/trailers or
validates the promised length. Dropping discards pending data without hidden I/O.

`append_body` can place body bytes after entirely unsent output, allowing a small
response head and body to share one write. Once flushing starts, finish flushing
before appending. `poll_body(stream, bytes)` writes fixed-length or close-delimited
body bytes directly from the caller's slice in at most one transport operation.
It retains no borrow after return; advance the source by the reported progress.
Flush pending output first. Chunked bodies continue through `send_body` and
`poll_flush`. Hosted adapters use these paths automatically.

`Connection.suspend()` consumes a driver and returns its opaque `State` without
buffer borrows. `Connection.resume(state, workspace, input, output)` reattaches
the same buffer contents and checks capacities. Preserve each state's association
with its buffers; capacity checks cannot detect unrelated or overwritten bytes.
`Parser.suspend()` / `Parser.resume()` provide the equivalent parser operation.
This lets the reactor keep per-peer protocol state while borrowing one slot's
buffers only for its current turn.

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

The [web server example](https://github.com/Jotrorox/dodo/blob/main/examples/web_server_polling.dodo)
demonstrates bounded serial execution: one connection, fixed buffers, a 30 s
absolute deadline, routing/error responses, streaming, TCP half-close and cleanup.
The [client](https://github.com/Jotrorox/dodo/blob/main/examples/http_client_polling.dodo)
consumes its response with a five-second deadline. Run these in two terminals:

```sh
dodo run examples/web_server_polling.dodo
dodo run examples/http_client_polling.dodo
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

## Hosted convenience layer

Use `std/web/app.Server` for grouped application setup and bounded default
storage. Its underlying `std/web/hosted` runner provides serial connection,
readiness, deadline, and body-transfer loops. See [Hosted HTTP and HTTPS](hosted-http.md) for complete small programs,
explicit storage bounds, resolver/deadline scope, cancellation, and a generated
local HTTPS setup. Protocol and routing APIs remain usable independently.
