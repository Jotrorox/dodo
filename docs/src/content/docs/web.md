---
title: "Web applications"
description: "Portable routing, checked streaming handlers, optional rooted files, and explicit server execution."
section: "Standard library"
order: 160
---

A web application maps an HTTP method and path to a handler. The handler reads
validated request data and builds a bounded response. Start with `std/web/app`
to register routes and run a server on Linux GNU x86-64 or Windows x64.
Use `std/web/application` for the same routing and handlers in portable,
in-process tests.

Work through the first server, several routes, and tests before configuring
concurrency or TLS. The later sections explain exact routing precedence,
request decoding, response limits, streaming, and static files.

| Application need | Starting point |
| --- | --- |
| A fixed page or health endpoint | `web.text`, `html`, `json`, or `bytes` with `app.new().get(...)` |
| Request-dependent output | A public struct with `handle(&mut self, request, response)` |
| Several routes plus shared policy | Fluent registrations and `.middleware(...)` |
| Repeatable behavior tests | `application.builder()` and `site.request(...)` |
| Multiple sockets progressing together | `.concurrent()`; synchronous handlers still share one thread |
| HTTPS | `.run_with(address, https.files(certificate, key))` |
| Streaming bodies/custom execution | `std/web/server` and `std/http/connection` |

## Quickstart

Save this as `main.dodo`:

```dodo
package main
import "std/web"
import "std/web/app"

fn main() -> i32 {
    return app.new()
        .get(b"/", web.text(b"Hello, Dodo!\n"))
        .run(b"127.0.0.1:8080")
}
```

Start it with `dodo run`, then visit `http://127.0.0.1:8080` or run
`curl http://127.0.0.1:8080` in another terminal. Ctrl+C stops the process.
`run(address)` returns an exit code for `main`: 0 after clean shutdown, 1 for a
setup/startup error, or 2 if serving returns with failed connections. It prints
startup errors and the last available connection failure to stderr.

Use `serve(address)` when you want to handle the Result and serving report
yourself. `serve_until(address, cancel)` supports cooperative shutdown with a
public `cancelled(&self) -> bool` method. Serving accepts connections until
cancelled by default; setting `config.max_connections` bounds the total accepted
connections. Routing rejections such as 404 and 405 are counted separately from
connection failures.

## Several routes

For `/hello/Ada?suffix=+friend`, the method and decoded path select a route,
`:name` captures `Ada`, and the query helper returns ` friend`. Path captures
and query values are separate inputs; the `+` convention applies only to query
decoding. Read [request access](#request-access-and-decoding) for the exact rules.

Static responses need no custom handler. Dynamic handlers receive a request and
write into a response:

```dodo
package web_routes
import "std/web"
import "std/web/app"

pub struct Greeting {
    pub fn handle(&mut self, request: &mut web.Request, response: &mut web.Response) -> void!web.Failure {
        response.text(b"Hello, ")?
        response.append(request.param(b"name")?)?
        return response.append(request.query_or(b"suffix", b"!"))
    }
}
fn main() -> i32 {
    return app.new()
        .get(b"/", web.html(b"<h1>Dodo</h1>"))
        .get(b"/hello/:name", Greeting {})
        .post(b"/created", web.json(b"{\"created\":true}").status(201))
        .get(b"/old", web.redirect(b"/"))
        .run(b"127.0.0.1:8080")
}
```

`/hello/Ada` responds with `Hello, Ada!`; `/hello/Ada?suffix=+friend` responds
with `Hello, Ada friend`. See the complete [multi-route example](https://github.com/Jotrorox/dodo/blob/main/examples/web_routes.dodo)
for a body echo route too.

| Registration | Purpose |
| --- | --- |
| `.get`, `.post`, `.put`, `.patch`, `.delete`, `.head`, `.options` | Register a method and path pattern |
| `.route(method, pattern, handler)` | Register another case-sensitive method |
| `.middleware(value)` | Apply middleware to every matched route |
| `.concurrent()` | Enable concurrent socket progress on the hosted builder |
| `.max_connections(count)` | Stop after this many accepted connections; zero is unlimited |
| `.build()` | Validate setup and obtain a reusable application/server for tests or configuration |
| `.run(address)` | Serve with error printing and a process exit code |
| `.serve(address)`, `.serve_until(address, cancel)` | Serve with a Result and detailed report |

Registration consumes the builder and returns its new type, so chain calls or
bind their result. It retains the **first** registration error; `build`, `serve`,
and `run` report it before binding a socket. A duplicate route produces a
printable diagnostic such as:

```text
web route #2 (GET /:second): web: AmbiguousRoute; this method and path shape are already registered
```

`RegistrationError` exposes `kind`, `method`, `pattern`, and the zero-based
`index`. Fluent serving returns `app.Error`, with either `registration` or
`serving` populated. `build()` returns `application.RegistrationError` directly;
serving on a built `Server` returns the underlying `hosting.Error`.

Routes use `/users/:id` for a segment and `/files/*rest` for a final wildcard.
Literal paths take precedence. HEAD falls back to GET, unmatched paths return
404, and unsupported methods on a matching path return 405. There is no automatic
OPTIONS response or trailing-slash redirect.

The fluent builder stores up to eight routes (`application.ROUTE_CAPACITY`).
Handlers are owned inline, retain state between requests, and use ordinary
compiler-checked methods. Dodo currently has no closures or function values;
large generic chains are costly to compile. Larger tables can use `web.Router`
and an explicit dispatcher. Route patterns and handler borrows must outlive the
application. No request/response borrow can escape into handler state.

### Ready-made responses

`web.text(body)`, `web.html(body)`, `web.json(body)`, and `web.bytes(body)` return
ordinary handlers. `.status(code)` changes their status; validation occurs when
they handle a request. `web.redirect(location)` defaults to 303 (See Other);
`.status(301)`, 302, 307, and 308 select other redirect behaviors.

Text and HTML validate UTF-8. JSON accepts **already serialized UTF-8 JSON**; it
does not serialize structs or validate JSON syntax. HTML helpers do not escape
untrusted text. Body and header bytes are copied into bounded response storage.

## Test without a server

Test application behavior before introducing sockets. Start with successful
requests, then add missing paths, wrong methods, missing required input,
invalid numeric input, and handler failures. These tests use the real router
and handler, so a rejected request should assert both status and public body.

Use `application.builder()` for portable application tests. It offers the same
route and middleware methods; `build()` returns an `Application` without hosted
configuration. `request(method, target)` decodes and dispatches a request in
process, returning an owned response:

```dodo test
package web_test
import "std/web"
import "std/web/application"

@test
fn health_check() {
    site := application.builder()
        .get(b"/health", web.json(b"{\"ok\":true}"))
        .build()!

    site.request(b"GET", b"/health")
        .expect_status(200)
        .expect_header(b"Content-Type", b"application/json")
        .expect_body(b"{\"ok\":true}")
    site.request(b"GET", b"/missing").expect_status(404)
    site.request(b"HEAD", b"/health").expect_status(200).expect_body(b"")
}
```

Run `dodo test`. Responses own their bytes and remain valid after another request
or after dropping the application. Inspect `status()`, `body()`, `header(name)`,
or `header_at(index)` directly, or chain the consuming `expect_*` assertions.
For enumeration, initialize `cursor := 0usize` and call
`header_next(&mut cursor)` until it returns `none`. The cursor tracks a byte
offset, so each header is visited once. This also works on `web.Response`;
`Fields.next` and `web.field_next` expose the same traversal for field storage.
The response's `failure` field retains a handler/decoding failure for debugging;
HTTP error bodies stay empty and partial handler output is discarded.

For headers and request bodies, import `std/web/testing`:

```dodo test
package echo_test
import "std/web"
import "std/web/application"
import "std/web/testing"

pub struct Echo {
    pub fn handle(&mut self, request: &mut web.Request, response: &mut web.Response) -> void!web.Failure {
        response.header(b"X-Token", request.require_header(b"X-Token")?)?
        return response.bytes(request.body())
    }
}
@test
fn echoes_body() {
    site := application.builder().post(b"/echo", Echo {}).build()!
    input := testing.Request.new(b"POST", b"/echo").body(b"hello")
    input.header(b"X-Token", b"test")!
    site.request_with(&input)
        .expect_status(200)
        .expect_header(b"X-Token", b"test")
        .expect_body(b"hello")
}
```

Built hosted servers expose the same `request` and `request_with` methods.
Tests run the real router, decoding, handler state, and middleware. They use fixed
4 KiB bodies, 4 KiB/32-field headers, and 2 KiB path and query buffers (100 query
pairs). They do not simulate HTTP framing, socket behavior, deadlines, or custom
server limits. Request IDs are 1 for each synthetic request. Keep independent
HTTP integration tests for those boundaries.

### Test validated input and failure responses

This handler requires a decimal route parameter. A malformed ID automatically
becomes a 400 response through `?`; a valid but unknown ID is an explicit 404.
There is no need to build an error body for either case.

```dodo test
package validated_route
import "std/web"
import "std/web/application"

pub struct Item {
    pub fn handle(&mut self, request: &mut web.Request,
        response: &mut web.Response) -> void!web.Failure {
        id := request.param_u64(b"id")?
        if id != 7 { return err(web.reject(404)) }
        return response.text(b"item seven")
    }
}

@test
fn distinguishes_bad_input_from_missing_items() {
    site := application.builder().get(b"/items/:id", Item {}).build()!
    site.request(b"GET", b"/items/7").expect_status(200).expect_body(b"item seven")
    site.request(b"GET", b"/items/seven").expect_status(400).expect_body(b"")
    site.request(b"GET", b"/items/8").expect_status(404).expect_body(b"")
    site.request(b"POST", b"/items/7").expect_status(405)
}
```

The 404 produced by the handler is distinct from the router's 404 for an
unmatched path, even though the public status can be identical. Inspect a test
response's `failure` or the hosted serving report when diagnosing the cause.

### Existing registration API

`application.new().get(pattern, handler)!` and `app.Server.new(routes)` remain
available. Each registration returns a Result immediately. This API now also
has all seven method shortcuts and the same in-process testing methods. The
fluent builder is the simpler default for new apps.

## Configuration and storage

Choose bounds from the messages your application accepts. Increasing a numeric
limit alone does not add storage: the effective body limit is also constrained
by the actual request/response buffers. A response body is assembled before it
is sent, so large downloads belong on the streaming path described later.

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

## HTTPS

Use the same fluent routes and handlers with an explicit TLS transport:

```dodo
package https_server
import "std/web"
import "std/web/app"
import "std/web/https"

fn main() -> i32 {
    return app.new()
        .get(b"/", web.text(b"Hello, Dodo!\n"))
        .concurrent()
        .run_with(b"127.0.0.1:8443", https.files("cert.pem", "key.pem"))
}
```

`https.files` reads the certificate chain and private key from PEM files once
at startup, with an 8,192-byte limit for each file. Relative paths use the
process's working directory. Missing and oversized files report their path;
invalid PEM or a mismatched key fails before binding. Route registration is
checked first. `run_with` prints failures and uses the same exit codes as `run`:
0 on success, 1 for startup failure, and 2 for failed connections.

The HTTPS adapter uses `app.Config` limits and timeouts, bounded default buffers,
middleware, and the same request/response helpers. Add `.concurrent()` before
`.run_with(...)` to enable concurrent TLS handshakes, keep-alive, and ordered
HTTP/1.1 pipelining through the reactor. The request limit, idle timeout, and
per-turn event budget apply just as they do for concurrent HTTP. A stalled
handshake or socket does not block other connections; handlers run synchronously.
The header timeout bounds the TLS handshake, then a fresh header and request
budget starts for HTTP. Responses are complete only after their encrypted output
has drained. Normal closure sends TLS `close_notify`; cancellation aborts all
active connections immediately. The default serial mode still handles one
request per connection.

Importing `std/web/https` selects OpenSSL 3.5+; ordinary `std/web/app` programs
retain their existing dependencies.

For typed errors and cooperative cancellation, build the app and call the
transport directly:

```dodo
server := app.new().get(b"/", web.text(b"hello")).build()!
identity := https.files("cert.pem", "key.pem")
report := identity.serve_until(server, b"127.0.0.1:8443", &cancel)!
```

Here `cancel` supplies `cancelled(&self) -> bool`, as in the configuration
example above. `identity.serve(server, address)` uses default cancellation;
`identity.serve_with(server, address, &mut storage, &mut clock, &cancel)` accepts
the same `app.Storage` used for custom HTTP buffers. For concurrent HTTPS, use
`app.Storage.concurrent` with 1–255 reactor slots and at least
`https.WORKSPACE_BYTES` (69,632 bytes) per slot. This includes 8 KiB of TLS
ciphertext staging in addition to the HTTP workspace. Request and response
buffers are divided equally among the slots. Default concurrent HTTPS reserves
608 KiB for eight slots' byte buffers, plus slot bookkeeping and OpenSSL's
internal allocations. Serial storage uses `app.WORKSPACE_BYTES` as before.
These calls consume the server and return `hosted.Report!https.Error`. Errors
retain the file cause or underlying `hosting.Error`; a file error borrows its path from `identity`.
For larger credential buffers or explicit OpenSSL settings, pass
`https.Provider.new(identity)` to `Server.serve_concurrent_with`, where `identity`
is an `openssl.Config`. The existing serial acceptor API is in `std/http/https`.

See [complete local HTTPS setup](hosted-http.md#complete-local-https-setup) for
generating test credentials and running a verified client.

## Hosted server choices

The server object uses the existing serial `std/web/hosted` and concurrent
`std/web/reactor` runners. Those lower-level functions remain available for
custom dispatchers and transport adapters. `Builder.run_with(address, transport)`
accepts a transport with `serve(server, address) -> hosted.Report!E`, where `E`
is printable. `Server.serve_serial_with` exposes the serial acceptor hook with
custom storage, clock, and cancellation. `Server.serve_concurrent_with` exposes
the reactor transport hook. HTTPS selects the corresponding runner through the
convenience adapter above. Streaming bodies, upgrades, and
caller-driven execution use `std/web/server`, `std/web/stream`, `std/web/response`, and
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
import "std/web"
import "std/web/app"

fn main() -> i32 {
    return app.new()
        .get(b"/", web.text(b"Hello, Dodo!\n"))
        .concurrent()
        .run(b"127.0.0.1:8080")
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
The reactor serves HTTP and, through `std/web/https`, HTTPS. Custom polling
transports can use `reactor.serve_using`; the explicit serial TLS acceptor
remains available through `std/http/https`.

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
literal paths and the literal prefix before a route's first parameter or wildcard.
Supply at least twice as many `usize` slots as routes. Construction checks
duplicate shapes while inserting; hash collisions always require full comparison.
Both the route table and index remain borrowed by the router; failed construction
may modify index storage. Dynamic lookup probes prefixes at segment boundaries
and checks candidates with those prefixes. Routes sharing a prefix still scan
within that group, including routes beginning with a parameter or wildcard.
Literal precedence, method selection, and HEAD fallback remain identical.

Application registration retains its validated index. `table.router()` borrows
it without rebuilding or revalidating; in-process application requests reuse this
view. `testing.send_router(&router, &mut handler, &input)` also accepts an existing
router, while `testing.send(routes, ...)` validates raw routes on each call.

Both hosted runners automatically index every table. Up to 1,024 routes use
inline storage; larger tables allocate an index once at startup and free it when
serving returns. The index has two `usize` slots per route; allocation or size
failure returns `hosting.ErrorKind.Workspace` before binding. Request routing
does not allocate. Portable `Router.indexed` continues to use only caller storage.

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

`Request` captures the first eight named parameter spans during construction.
Repeated parameter reads reuse those offsets; routes with more parameters remain
supported and look up later names without repeating the full pattern match.

Before routing an HTTP target, call `decode_path(target, output)`. It strips the
query and decodes percent escapes **once** into caller storage, respecting the
component distinction in [RFC 3986](https://www.rfc-editor.org/rfc/rfc3986.html).
It rejects malformed escapes, encoded slash/backslash, NUL and controls, invalid
UTF-8, fragments, backslashes and `.`/`..` segments. `+` remains `+`. An escaped
percent remains literal: routing never decodes it again. Repeated and trailing
slashes remain significant. Patterns contain decoded UTF-8 literals, not percent
escapes. Insufficient output returns `BufferFull`; an error may leave a modified
prefix that must not be used as a path. `Router.find` accepts already-decoded
paths and validates them. `Path.decode(target, output)` decodes and returns a
validated borrowed path; `Path.new(bytes)` validates an already-decoded path.
Pass either to `Router.find_path(method, &path)` to reuse validation. The path
holds a checked borrow, so its bytes cannot change while that proof is in use.
The server composition also extracts paths from absolute-form HTTP targets;
its router does not implement authority/virtual-host selection or OPTIONS `*`.

## Handlers and middleware

The buffered request lifecycle is:

1. HTTP framing and bounded body collection complete.
2. The target is decoded and a route/method selected.
3. Middleware `before` methods run in order.
4. The selected handler reads the request and writes a response.
5. Middleware `after` methods run in reverse order, subject to the failure rules
   below, and the runner sends the response.

Handlers retain their own state between requests, but request and response
views are temporary. Copy the particular data you need into appropriately
owned state; do not retain a borrowed path, header, body, or response slice.

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
        response.header(b"X-Request-Method", request.method())?
        return response.json(b"{\"hello\":\"Dodo\"}")
    }
}
```

Register this handler with `app.new().get(b"/hello/:name", Greeting {})`. See the complete
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
| `request.param(name)` | Required route capture; use `?` to reject missing input with 400 |
| `request.param_u64(name)` | Required ASCII decimal capture, checked for overflow; invalid input returns 400 |
| `request.header_value(name)`, `query_value(name)` | First occurrence as an Option |
| `request.require_header(name)`, `require_query(name)` | First occurrence as a Result; missing input returns 400 |
| `request.query_or(name, fallback)` | First value, or fallback when absent; preserves empty values |
| `request.query_u64(name, fallback)` | First ASCII decimal value; missing uses fallback, empty/invalid/overflow returns 400 |

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
| `response.json(bytes)` | Copy already serialized UTF-8 JSON; default type `application/json` |
| `response.append(bytes)` | Append bytes to the current body within its capacity |
| `response.empty(code)` | Set a status and clear the body |
| `response.redirect(location)` | Empty 303 response with a validated Location header |
| `response.redirect_to(location, code)` | Redirect with 301, 302, 303, 307, or 308 |
| `response.header_value(name)` | Inspect the first response header without case sensitivity |
| `response.header(name, value)` | Copy and append a validated header |
| `response.text(bytes)` | Validate UTF-8 and copy the body; default type `text/plain; charset=utf-8` |
| `response.html(bytes)` | Validate UTF-8 and copy the body; default type `text/html; charset=utf-8` |
| `response.bytes(bytes)` | Copy arbitrary body bytes; add no Content-Type |
| `response.status()`, `response.body()` | Read status or borrow copied body bytes |
| `response.header_at(index)` | Borrow a copied header by insertion index |
| `response.header_next(&mut cursor)` | Iterate headers in insertion order; start the byte cursor at zero |

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

Use `return err(web.reject(401))` for an intentional HTTP rejection, or propagate
required/numeric input failures with `?`. Valid rejection codes are 400–599;
invalid codes become internal 500 errors. `Failure.status()` exposes the selected
status. Ordinary `web.failure(...)` values remain internal 500 errors when
returned by a handler. Serial, concurrent, and in-process dispatch agree.

`web.Failure` keeps a `kind: web.Error`, plus optional underlying `http.Error`
and `text.Error` diagnostics. Response capacity and header/status validation errors retain
protocol kind and position; invalid UTF-8 retains its exact text diagnostic.
Hosted failures retain this value in `hosting.Error.application`, and server
`Report.last_error` retains the latest observed connection failure. Public error
responses are empty; internal errors and request data are never formatted into
them. `serve_connection` returns the original error even when it successfully
sends an error response (`reason.responded == true`); the serial server counts
these as rejected connections. Inspect reports as well as startup Results.

### Middleware composition

Register a struct with public `before` and `after` methods using
`.middleware(value)`. It applies to **every matched route**, including routes
added earlier or later. Multiple middleware values run `before` in registration
order and `after` in reverse order. Routing errors occur before middleware.

```dodo test
package middleware_test
import "std/web"
import "std/web/application"

pub struct RequireToken {
    pub fn before(&mut self, request: &mut web.Request) -> void!web.Failure {
        match request.header_value(b"Authorization") {
            some(_) => { return ok() }
            none => { return err(web.reject(401)) }
        }
    }
    pub fn after(&mut self, request: &mut web.Request, status: u16) {}
}
@test
fn rejects_missing_token() {
    site := application.builder()
        .get(b"/private", web.text(b"Hello"))
        .middleware(RequireToken {})
        .build()!
    site.request(b"GET", b"/private").expect_status(401).expect_body(b"")
}
```

This demonstrates a header-presence check; a real authentication middleware also
validates the token. For middleware on one route, register
`web.with(handler, middleware)` as its handler.

`Chain<A,B>` invokes `before(request)` outer-to-inner and `after(request, status)`
inner-to-outer. `before` returns `void!web.Failure`; a failure short-circuits
without invoking the handler or `after`. Handler failure invokes `after` with
the failure’s HTTP status (500 by default) and preserves the failure. Cancellation
before dispatch invokes neither.
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

## Complete API reference

For every public type, field, constant, and function signature, see [std/web](api/std/web.md), [std/web/application](api/std/web/application.md), [std/web/app](api/std/web/app.md), [std/web/testing](api/std/web/testing.md), [std/web/https](api/std/web/https.md), [std/web/reactor](api/std/web/reactor.md), [std/web/server](api/std/web/server.md), [std/web/static_files](api/std/web/static_files.md).
