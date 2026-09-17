---
title: "HTTP"
description: "Portable, bounded HTTP/1.1 framing and explicit transport composition."
section: "Standard library"
order: 159
---

HTTP exchanges a request and a response. Each has a start line, headers, and a
body whose boundaries are defined by framing rules. A successful HTTP exchange
can still return a status such as 404; the status is application information,
separate from a transport or parsing failure.

Start with `std/http/hosted.Client` to fetch a URL on Linux GNU x86-64 or Windows
x64. It handles resolution, connection, and polling with explicit storage and
limits. Continue into the portable parser and sender when you need custom
transports, incremental bodies, or control over each I/O step.

## Choose an HTTP layer

| Layer | Use it when | You supply |
| --- | --- | --- |
| `std/http/hosted` or `std/http/https` | You want a complete URL request. | Workspace, body sink/buffer, policy; explicit trust for HTTPS. |
| `std/http/client` | You already own a transport and execution loop. | Origin, three buffers, clock observations, polling, and readiness. |
| `std/http/connection` | You are composing a client/server protocol driver. | Transport, buffer storage, event/body consumption, and execution policy. |
| `std/http` | You need HTTP values, parsing, or serialization only. | Input bytes and bounded output/workspace. |
| [std/web/app](web.md) | You want routes, handlers, and a hosted server. | Application behavior and explicit server limits. |

The portable layers do not open connections or read clocks. Importing HTTP
does not select TLS; choose the HTTPS adapter or a verified custom transport.

## Quickstart

Create a fresh directory for a Python 3.11+ local HTTP peer in terminal 1:

```sh
python3 -c "from pathlib import Path; Path('local-http').mkdir(exist_ok=True); Path('local-http/index.html').write_bytes(b'Hello, Dodo!\n')"
python3 -m http.server 8080 --bind 127.0.0.1 --directory local-http --protocol HTTP/1.1
```

Wait for the serving message, then use terminal 2. If port 8080 is occupied,
stop your previous example or change the port in both places. No Internet,
proxy configuration or credentials are involved. `--protocol HTTP/1.1` is
required because Dodo rejects HTTP/1.0 responses.

Save this as `fetch_url.dodo`:

```dodo
package fetch_url
import "std/http/hosted"
import "std/http/hosting"
import "std/console"
import "std/io"

fn fetch() -> i32!hosting.Error {
    workspace := [0u8; hosted.WORKSPACE_BYTES]
    client := hosted.Client.new(&mut workspace, hosted.Config.defaults())?
    body := [0u8; 1024]
    response := client.get(b"http://127.0.0.1:8080/", &mut body)?
    if response.status != 200 { return ok(2) }
    output := console.stdout()
    match io.write_all(&mut output, &body[..response.body_bytes as usize]) {
        ok(_) => { return ok(0) }, err(_) => { return ok(3) },
    }
}
fn main() -> i32 {
    match fetch() {
        ok(code) => { return code },
        err(reason) => {
            if reason.kind == hosting.ErrorKind.BodyLimit { return 4 }
            errors := console.stderr()
            match errors.println(&reason) {
                ok(_) => {}, err(_) => {},
            }
            return 1
        },
    }
}
```

```sh
dodo run fetch_url.dodo
```

Expected stdout is `Hello, Dodo!` followed by a newline; exit 0 means a complete
HTTP 200 response was received. This also works against the [web quickstart](web.md).
Stop the Python server with Ctrl+C when finished.

`hosted.WORKSPACE_BYTES` supplies fixed parser/header/I/O storage; the 1024-byte
array independently bounds the response body. `std/http/hosting` names shared
hosted errors; its driving helpers are implementation plumbing, not the
recommended application entry point. `console` selects stdout and `io.write_all`
handles partial writes.

Exit 1 means the request failed: start the local server for a connect error,
check the URL for resolution errors, and inspect the nested network, transport
or protocol cause. Exit 4 means the body exceeded your bound; increase it within
a chosen limit or use `get_to` with a streaming sink. Error `delivered` counts
body bytes already accepted by the sink. Exit 2 is an HTTP status other than 200,
which is a completed response, not a transport failure. Exit 3 means stdout failed.

## Hosted client choices

`Config.defaults()` disables redirects and sets explicit positive resolution,
connection and request budgets. Configure `max_redirects` only when following
locations is intended. System resolution cannot interrupt `getaddrinfo`; its
budget is checked before and after that call. Deadlines bound cooperative I/O,
not arbitrary native blocking or application work.

`get_to(url, sink)` streams to an `std/io` writer; `request` accepts explicit
method, headers and borrowed body bytes. `header(name, occurrence)` exposes the
final response's copied headers until the next request, and errors clear them.
`request_with` selects resolver, connector, clock and cancellation providers.
The default connector is plaintext HTTP. For HTTPS, start with
`std/http/https.Client` and explicit `https.Trust`, as shown in the
[TLS quickstart](tls.md). Never disable peer verification to make a
request succeed. `std/http/connection` and the polling `std/http/client` are the
advanced portable path described below.

## API and contracts

`std/http` contains HTTP types and an incremental HTTP/1.1 engine. Importing it
requires only portable `core/ascii`, `core/bytes`, `core/mem`, and `std/net`
address parsing. It performs no
allocation, socket operation, clock read, filesystem access, TLS operation, or
thread creation. Transport composition belongs to `std/http/connection`,
`std/http/client`, and `std/http/server`; TLS backend choice remains external.

The wire grammar and framing follow [RFC 9112](https://www.rfc-editor.org/rfc/rfc9112.html),
message semantics follow [RFC 9110](https://www.rfc-editor.org/rfc/rfc9110.html),
and URI components follow [RFC 3986](https://www.rfc-editor.org/rfc/rfc3986.html).
The supported wire version is HTTP/1.1. HTTP/1.0 fallback, HTTP/2,
HTTP/3/QUIC, WebSockets, cookies, multipart parsing, and compression are outside
this package. Unknown content encodings are delivered as opaque body bytes.

## Types and ownership

`Method.new(bytes)` validates a case-sensitive token. Extension methods are
accepted; `idempotent()` recognizes the standard idempotent methods. `Status.new`
accepts codes 100–599. `Header` holds borrowed name/value byte slices. Header
names compare using ASCII case folding; values remain bytes, including obs-text.
`Request` contains method, request target, headers, and `Body`; `Response`
contains numeric status, reason, headers, and `Body`. These are head descriptions,
not owners of a buffered body. `Body.empty()`, `Body.sized(n)`, and
`Body.streaming()` describe no body, content-length, and chunked framing.
`BodyKind.Close` is available for close-delimited responses.

`Uri.parse` accepts origin-form paths, `*`, and absolute HTTP(S) URIs, validates
percent escape spelling, and preserves encoded bytes. URI accessors borrow the
original input. Userinfo, fragments, spaces, controls, raw non-ASCII, and
backslashes are rejected. Scheme and origin comparisons use ASCII case folding;
origin comparison is conservative and does not normalize default ports, DNS
aliases, or percent-encoded host spellings. Path decoding and routing are
separate decisions in `std/web`; `+` is not a space in a path. An absolute URI
with no path has an empty `path()` accessor, which a client should serialize as
`/`. CONNECT authority-form is accepted separately by the wire parser/serializer
and requires an explicit numeric port. IPv6 authorities require brackets;
malformed IP literals and ports above 65535 are rejected. Scoped IPv6 literals
and IPvFuture authority syntax are outside this implementation. Asterisk-form
is accepted only for OPTIONS.

The compiler checks parser workspace, URI, header, request, and response
lifetimes. `Parser.target()`, `method()`, and `field(offset, length)` return
borrowed views; a live view prevents parser mutation or releasing its backing
storage. Event values contain offsets, never references into a transient input
buffer. A caller must obtain views only from their corresponding parser event:
offsets do not carry a message generation identifier.

## Incremental parsing

### Parse a complete in-memory request

You can learn the parser without a socket. This complete test repeatedly feeds
the unconsumed suffix of one HTTP/1.1 request and counts its body bytes:

```dodo test
package parse_request
import "std/http"

@test
fn reads_a_framed_body() {
    workspace := [0u8; 1024]
    parser := http.Parser.new(http.Role.Request, &mut workspace, http.Limits.defaults())
    input := b"POST /items HTTP/1.1\r\nHost: localhost\r\nContent-Length: 3\r\n\r\nabc"
    cursor := 0usize
    body_bytes := 0usize
    for true {
        event := parser.feed(&input[cursor..])!
        cursor += event.consumed
        if event.kind == http.EventKind.Body {
            body_bytes += event.length
        }
        if event.kind == http.EventKind.Complete { break }
        assert(event.kind != http.EventKind.NeedInput,
            "the complete test message must provide every required byte")
    }
    assert_eq(cursor, input.len)
    assert_eq(body_bytes, 3usize)
    assert(parser.reusable())
}
```

One `feed` can consume fewer bytes than it receives because it stops at the
next event. It may also emit an event while consuming zero bytes. The loop
therefore advances by `event.consumed`, not by the length of the supplied input.
In a network program, `NeedInput` means to obtain more bytes; it does not mean
EOF. The test asserts against it because all bytes are already available.

### Drive events and retain the right storage

Create `Parser.new(Role.Request, &mut workspace, Limits.defaults())` for a server
or `Role.Response` for a client. Call `feed(input)` and advance the input cursor
by `Event.consumed`. Continue feeding the unconsumed suffix. A single call
produces one event. No read-ahead past that event is lost, including pipelined
messages or bytes after a protocol upgrade.

| Event | Meaning |
| --- | --- |
| `NeedInput` | Supplied input exhausted; retain parser state and supply more bytes. |
| `StartLine` | Method/target or response status is available. |
| `Header` | Name/value offsets identify validated bytes in parser workspace. |
| `HeadersComplete` | Validated framing is fixed; the body can be streamed. |
| `Body` | `offset` and `length` index this call's input, already dechunked. |
| `Trailer` | Name/value offsets identify a trailer in workspace. |
| `Informational` | A 1xx response other than 101 is complete; next feed starts the next response. |
| `Complete` | This message has ended. |
| `Upgrade` | Switch protocols; all unconsumed bytes belong to the new protocol. |

Call `feed(empty)` after consuming a complete head or final length-delimited
body to obtain pending zero-consumption events. Chunked messages emit `Complete`
while consuming the final trailer CRLF. Stop feeding after `Complete` until
`next_message()` succeeds. This explicit transition prevents silently treating
unconsumed bytes as another message. `next_message()` requires `reusable()`;
connection-close, upgrade, incomplete, and poisoned states cannot be reused.
It resets HEAD/CONNECT association, which must be set for each new request.

Set `expect_head(true)` before parsing a response to HEAD; representation
Content-Length is allowed but no body is consumed. Set `expect_connect(true)`
for a CONNECT response; a successful response transitions to `Upgrade` at the
head boundary. 101 requires Connection: upgrade and a nonempty Upgrade field.
The caller must additionally validate the selected upgrade protocol against its
offer before handing over the stream. Other 1xx responses restart the parser
without ending the outstanding request. Responses to 204 and 304 have no body.
Requests without framing have no body. Other responses without explicit
framing are close-delimited and permanently ineligible for reuse.

At transport EOF call `eof()`. Only a close-delimited body or an already complete
message ends successfully. Truncated start lines, headers, content-length,
chunks, or trailers fail with `UnexpectedEof` and poison the parser. Empty feed
input does not mean EOF. Transport/TLS failures must call `poison()`; a TLS
truncation error must never be presented as clean transport EOF.

### Strict framing and errors

Bare LF, bare CR, obsolete folding, whitespace before a colon, invalid field
names/values, missing or duplicate Host, invalid decimal/hexadecimal lengths,
and length overflow are rejected. Content-Length combined with Transfer-Encoding
is rejected in either order. Duplicate Content-Length is rejected even when its
values agree; comma lists are rejected. Only a single `chunked` transfer coding
is implemented. Unsupported or repeated transfer codings fail instead of being
interpreted as another framing mode. Content-Length or Transfer-Encoding on
1xx and 204 responses is rejected.

Chunk extensions support tokens and quoted strings with escaped bytes; they
are validated and discarded. Trailers are emitted separately and never merged
into headers. Framing, routing, connection, authorization, and representation
metadata fields cannot appear as trailers. Applications should further restrict
trailer names to those explicitly meaningful for their own protocol.

All parser errors permanently poison the connection. `Error.kind` distinguishes
syntax, framing, resource limits, truncation, state, and bounds failures.
`position` identifies the relevant input/workspace location; an error is not a
recoverable input-progress report. Close the invalid connection instead of
trying to resume parsing or returning it to a pool. Request/header/body bytes
and credential values are never included in diagnostic formatting.

## Bounded workspace and backpressure

`Limits.defaults()` sets 16,384 head bytes, 100 header fields, 16,777,216 body
bytes, 1,024 bytes per chunk-size line, 4,096 trailer bytes, and 32 trailer
fields. Counts include line delimiters where applicable. The body limit applies
to decoded body bytes and is checked before producing an oversized event. A
Content-Length exceeding it fails at the head boundary. Chunk sizes are checked
against remaining body allowance before accepting the chunk.

The caller selects workspace size. Headers and start-line bytes remain intact
throughout the message. Chunk-size lines reuse the workspace suffix; trailers
accumulate after the head. Reserve `header_bytes + max(chunk_line_bytes,
trailer_bytes)` to accommodate every configured limit. Smaller storage is
supported and yields `BufferFull` if exhausted; this is terminal because part of
a message may already have been consumed. No heap allocation can fail inside
the engine. Increasing a limit never causes an allocation.

Body events directly view the supplied input. A consumer processes or copies
that fragment before reusing the input storage. Backpressure means simply
stopping calls to `feed`: the engine starts no work on its own. Header and body
deadlines, cancellation, scheduling, and transport operation ownership belong to
the composing connection provider. `poison()` invalidates a cancelled exchange;
retry policy never turns an incomplete exchange into a reusable connection.

## Serialization and streaming output

Serialization has two stages: write the head, then frame and deliver the body.
`write_request` and `write_response` write into memory; they do not send those
bytes to a socket. Likewise, `Sender.frame` reports encoded bytes ready for
delivery, not bytes received by a peer.

For a length-delimited body, declare the length before sending and finish only
after accepting exactly that many bytes. For a chunked body, stage chunks as
data becomes available and finish with the final zero chunk. In both cases,
retain unsent encoded bytes across partial transport writes.

`write_request(&request, destination)` and `write_response(&response,
destination)` produce a complete head into caller memory and return its length.
Publish only a successful returned prefix: errors can leave unpublished partial
bytes in the buffer. Field values cannot inject CR/LF. The `Body` descriptor is
the sole source of framing headers; manually supplied Content-Length or
Transfer-Encoding is rejected. Ordinary no-body responses receive
Content-Length: 0; bodyless 1xx/204/304 do not. A Close response receives
Connection: close if the caller has not supplied it.

`Sender.new(body)` tracks streaming output independently of a writer.
`frame(input, destination)` returns `Encoded { consumed, written }`; send the
written prefix and preserve it until the transport reports that it is fully
accepted. A short output buffer produces partial body progress. Chunked framing
requires at least 21 destination bytes. Empty input emits no chunk and does not
end the body. Call `finish(trailers, destination)` to produce the zero chunk and
trailers, or to validate completion of a fixed-length body. `complete()` becomes
true only after finish succeeds. The caller still owns the obligation to flush
the final bytes.

BufferFull on Sender output is retryable with more workspace and does not
change its state. Too many or too few length-delimited bytes, invalid trailers,
output after finish, and other state violations poison it. Transport failures
must call `poison()`. A sender has no implicit flush, destructor I/O, body replay,
allocation, or deadline.

## Explicit policies

Different layers report different kinds of incompleteness:

| Signal | May you retry on the same state? | Required action |
| --- | --- | --- |
| Parser `NeedInput` | Yes | Supply the next bytes, or call `eof()` only for clean transport EOF. |
| Driver `Backpressure` | Yes | Consume the pending body or flush pending output before retrying. |
| Sender `BufferFull` | Yes | Provide sufficient encoding workspace. |
| Parser syntax/framing/capacity error | No | Close the poisoned connection. |
| Transport/TLS error or cancellation | No implicit reuse | Poison/end the exchange and handle any delivered prefix. |

Always examine the layer returning an error. A retryable sender output shortage
does not make parser workspace exhaustion retryable, and an HTTP 500 response
is a completed protocol response rather than a parser failure.

`RedirectPolicy.defaults()` disables automatic redirects. `permits` receives
explicit source/target origins, hop count, and body replayability; HTTPS downgrade
is refused by default. `RetryPolicy.defaults()` allows one attempt and no retry.
Additional attempts require an idempotent method, replayable body, and no started
response. These helpers make decisions without executing them. A convenience
client must apply its configured status/method rewrite policy and supply a
replayable body provider before starting another attempt.

`forward_header(name, same_origin, to_proxy)` strips Host for recomputation,
strips authorization and cookies across origin changes, and never sends
Proxy-Authorization to an origin server. Application-specific secret headers
need an application allowlist. Connection pool admission requires both a fully
finished sender and `Parser.reusable()`, an alive transport, no unread body, no
pending output, matching origin/proxy/TLS identity, and nonexpired explicit
idle policy. The portable parser's reusable flag alone is insufficient.

## Verification

`cargo test --test http_library` runs fixtures at `-O0` and `-O3`, tests every
fragment width for ordinary/chunked/informational/upgrade messages, malformed
framing and truncation, pipelined input, output backpressure, resource limits,
and deterministic byte-mutation progress properties. Separate compile-fail
fixtures enforce parser view lifetimes. CPython's `http.client.HTTPResponse`
independently decodes Dodo-produced headers, chunks, trailers, and streamed body
reads. The same portable fixture emits objects for `wasm32-unknown-unknown` and
`thumbv6m-none-eabi`; it has no platform import.

## Convenient client composition

`std/http/client.Client` wraps the bounded connection driver around an explicit
origin and caller-owned parser/input/output buffers. The runnable
[`examples/http_client_polling.dodo`](https://github.com/Jotrorox/dodo/blob/main/examples/http_client_polling.dodo)
connects to the bundled web-server example with `std/net/native`, passes its
nonblocking stream to Client methods, and streams the response without allocating
or buffering the complete body.

Construct `Client.new(origin, workspace, input, output, limits, policy)` after
establishing the provider stream. `origin` is an absolute `Uri` whose bytes remain
borrowed by the client. `begin(&request, now_ms, cancelled)` validates origin,
Host, and proxy target form and queues the request head. Use `poll_flush(stream,
now_ms, cancelled)` until `pending_output()` is zero. `send_body(source, now_ms,
cancelled)` accepts only the prefix it can stage; flush it before submitting the
next fragment. `finish_body(trailers, now_ms, cancelled)` ends the request.
`poll_event(stream, now_ms, cancelled)`, `body()`, and `consume_body(count,
now_ms, cancelled)` expose response events and bounded backpressure. `next` resets
only a fully drained reusable exchange. An upgrade exposes `unread()` for explicit
handoff and cannot return to HTTP reuse.

Every action receives explicit monotonically nondecreasing milliseconds and a
cancellation snapshot. A backwards timestamp, cancellation, or reached deadline
fails before another provider operation. The request timeout covers the whole
exchange from begin, including application body processing and queued output.
Defaults are 10 seconds to connect, 30 seconds per request, 30 seconds idle,
and four idle slots. A zero timeout disables that deadline. Addition saturates
instead of wrapping. `connect_deadline(now_ms)` supplies the external connector
with the configured absolute deadline; the client does not execute DNS,
connection establishment, waits, or TLS handshakes itself. The provider must
complete or cancel its own pending operations before releasing their storage.

`Backpressure` is a retryable flow-control result. Protocol, transport,
cancellation, deadline, and invalid sequencing failures poison the connection.
A client cannot reset a failed exchange into reuse. Getters borrow buffered
bytes and perform no I/O; a live body view prevents any mutation of the client.
After an action is backpressured, the caller must explicitly retry it when the
sink or output buffer can progress. This API does not install an executor or
silently spin on Pending.

Proxy policy is mandatory and never inferred from the process environment:

| Policy | Required request target and provider state |
| --- | --- |
| `Direct` | Origin-form path (or OPTIONS `*`); attached origin stream. |
| `Forward` | Absolute-form URI matching the configured HTTP origin; attached proxy stream. HTTPS origins are rejected. |
| `Tunnel` | Origin-form after an external CONNECT and any required TLS verification; caller explicitly calls `tunnel_established()`. |

Host must match the configured authority. Proxy-Authorization is accepted only
for forward-proxy traffic and is rejected on direct or tunneled origin traffic.
`Client` does not initiate CONNECT; use the independent connection engine to
perform it, preserve upgrade bytes, finish TLS if needed, then attach a fresh
client to the established provider. For HTTPS the provider must be a verified
TLS stream: passing a plaintext socket under an HTTPS URI does not magically
upgrade it. TLS provider selection and trust credentials remain explicit.

Redirect and retry actions are deliberately visible to the caller. Configure
`Policy.redirects`/`retries`, obtain the decision, and construct the next request
only from an explicitly replayable source. 303 rewrites non-HEAD methods to GET
and discards the body; 301/302/307/308 preserve the method and require replayability.
`retry_allowed` refuses replay after any response bytes have arrived and requires
an idempotent method. Failed-connection retries require a fresh provider stream
and client. No request is sent twice automatically.

`RedirectHeaders.new(headers, same_origin, to_proxy)` iterates fields permitted
for a redirected head. It removes framing fields, Host, connection-specific
fields (including names nominated by Connection), and credentials crossing origin
boundaries. It never forwards proxy credentials to an origin. The caller supplies
new Host/framing and applies an additional allowlist for custom secret headers.

`Pool.new(slots)` uses caller-owned slot metadata. `Pool.from_policy(slots,
&policy)` additionally limits usable slots to `max_idle`. Each slot binds explicit,
collision-free origin, proxy, and TLS/trust identities. `take` rejects mismatched
identities and expires idle slots using supplied time; `release` requires a
reusable completed connection. Check the current client deadline/cancellation
before pool admission. Transport owners remain in the application's corresponding
slots and must be closed when metadata becomes vacant. There is no global pool,
allocation, or implicit transport destructor in this metadata layer.

## Hosted convenience layer

Use `std/http/hosted` for a reusable URL client and `std/http/https` for verified
HTTPS. They drive connection, readiness, deadline, and body-transfer loops.
See [Hosted HTTP and HTTPS](hosted-http.md) for complete small programs, explicit
storage bounds, resolver/deadline scope, cancellation, and a generated local
HTTPS setup. The protocol APIs on this page remain usable independently.

## Complete API reference

For every public type, field, constant, and function signature, see [std/http](api/std/http.md), [std/http/connection](api/std/http/connection.md), [std/http/client](api/std/http/client.md), [std/http/server](api/std/http/server.md).
