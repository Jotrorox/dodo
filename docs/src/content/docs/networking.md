---
title: "Networking"
description: "Portable IP and DNS codecs, owned nonblocking sockets, explicit execution and resolver providers."
section: "Standard library"
order: 157
---

Networking has three separate steps: identify an address, open a transport, and
exchange bytes or datagrams. `std/net` provides portable address values and
operation policies; `std/net/native` opens operating-system sockets. Start with
a loopback address and `native.connect_with` so you can learn the connection
contract without DNS or an external service.

| Task | Package/API | What it does not do |
| --- | --- | --- |
| Parse or print an IP address | `std/net` | No socket or DNS lookup. |
| Resolve a hostname | `native.resolve` | Does not connect; the system lookup may block. |
| Connect with a deadline | `native.connect_with` | Does not encrypt or frame application messages. |
| Read/write under a policy | `std/net/operations` | Does not allocate or start a scheduler. |
| Exchange separate packets | `native.UdpSocket` | No ordering, delivery, or stream semantics. |
| Encode/decode DNS packets | `std/net/dns` | No ready-made DNS resolver, cache, or retries. |
| Fetch an HTTP URL | [Hosted HTTP](hosted-http.md) | Select [HTTPS](tls.md) explicitly for TLS. |

TCP is a byte stream: one write need not correspond to one peer read. Define a
message boundary (a length, delimiter, or protocol such as HTTP) before building
a request/response exchange. UDP preserves each message boundary, but delivery
and ordering belong to your application protocol.

## Quickstart

Start this Python 3 peer in terminal 1; it accepts exactly one local connection:

```sh
python3 -u -c "import socket; s = socket.socket(); s.bind(('127.0.0.1', 8081)); s.listen(1); print('ready', flush=True); c, _ = s.accept(); c.close(); s.close(); print('accepted')"
```

Wait for `ready`, then use terminal 2 in your example directory.
If bind reports an occupied port, stop your previous example or change 8081 in
both programs. The listener is bound only to loopback.

Save this as `network_start.dodo`:

```dodo
package network_start
import "std/net"
import "std/net/native"

fn connect() -> void!net.Error {
    address := net.parse_socket(b"127.0.0.1:8081")?
    clock := native.MonotonicClock {}
    now := clock.now_ms()
    if now > (~0u64) - 5000 { return err(net.failure(net.ErrorKind.TimedOut)) }
    operation := net.Operation.until(now + 5000)
    cancel := net.NeverCancel {}
    stream := native.connect_with(&address, &operation, &mut clock, &cancel)?
    peer := stream.peer_address()?
    assert_eq(peer.port, 8081u16)
    stream.close()?
    return ok()
}
fn main() -> i32 {
    match connect() {
        ok() => { return 0 },
        err(reason) => {
            if reason.kind == net.ErrorKind.ConnectionRefused { return 2 }
            return 1
        },
    }
}
```

```sh
dodo run network_start.dodo
```

Expected output: none from Dodo; exit 0 means TCP connected to port 8081 and
closed cleanly. The peer prints `accepted` and exits. Restart it for another run.
The address and operation policy use fixed-size values; no caller byte workspace
or allocator is needed for this connection.

Exit 2 means connection refused: start the peer and check the port. Other
failures exit 1: inspect `kind` and native `code`; check an invalid address,
firewall or deadline before retrying. Never spin on `WouldBlock`; wait for the
appropriate readiness event under a deadline. The client uses the convenient
bounded connector; individual nonblocking steps remain available below.
For a complete request/response task, continue to [HTTP](http.md).

`std/net/dns` supplies portable DNS packets and resolver policies; use
`native.resolve_numeric` for numeric hosts without invoking system DNS.
`native.resolve` explicitly uses the hosted resolver and caller workspace;
it can block independently of a connection deadline.

## API and contracts

`std/net` contains address values, parsers, formatting, errors and operation
policy. It imports portable byte-I/O error support and works on freestanding targets. `std/net/dns`
contains DNS message handling. `std/net/operations` composes structural stream,
clock, cancellation and readiness capabilities without selecting a platform.
Importing any of these packages does not open a socket or require an allocator,
thread, filesystem, TLS backend or scheduler.

`std/net/native` selects the Linux or Windows socket adapter. Its public API is
identical on Linux x86-64 GNU and Windows x64. Other hosted ABIs currently produce
a compiler error. `std/net/linux` and `std/net/windows` may be imported explicitly
only for their matching target. There is no allocating network convenience
package in this release.

## Addresses and storage

Address parsing and formatting can be tested without a network. This example
normalizes IPv6 spelling and checks the port, using a fixed output array:

```dodo test
package address_example
import "core/bytes"
import "std/net"

@test
fn normalizes_a_socket_address() {
    address := net.parse_socket(b"[2001:0DB8:0:0:0:0:0:1]:443")!
    assert_eq(address.port, 443u16)
    output := [0u8; 64]
    count := net.format_socket(&address, &mut output)!
    assert(bytes.equal(&output[..count], b"[2001:db8::1]:443"))
}
```

`IpAddress` contains `family: Family.V4 | Family.V6` and `[16]u8` network-order
`octets`. `IpAddress.v4(a, b, c, d)` uses the first four octets and clears the
remaining twelve; `IpAddress.v6(octets)` uses all sixteen. `clone()` makes an
explicit owned copy and `equals(&other)` compares values. `SocketAddress.new(ip,
port)` adds a `u16` port and initializes its numeric `scope_id` to zero.

`parse_v4`, `parse_v6` and `parse_ip` take `&[u8]`. IPv4 accepts exactly four
canonical decimal octets, rejecting leading zeroes, signs, whitespace, and
legacy abbreviated/octal/integer forms. IPv6 accepts eight hexadecimal groups,
one optional `::` compression and optional final dotted IPv4. Parsing follows the
address forms in [RFC 4291 section 2.2](https://www.rfc-editor.org/rfc/rfc4291.html#section-2.2).

`parse_socket` accepts `127.0.0.1:80`, `[::1]:443` and `[fe80::1%3]:80`. IPv6 must
be bracketed. Ports are mandatory decimal values from 0 through 65535; numeric
zone indices range from 0 through 4294967295. Zone names require a separately
selected platform interface provider. A zone is not accepted by `parse_ip`.
These socket strings are not URI authorities: URI escaping is an HTTP concern.

`format_ip(&ip, output)` and `format_socket(&address, output)` return a byte count.
They write no NUL and leave output unchanged on `BufferTooSmall`. IPv6 output
uses lowercase hexadecimal, removes leading zeroes and compresses the first
longest run of at least two zero groups, following the normalization rules in
[RFC 5952 section 4](https://www.rfc-editor.org/rfc/rfc5952.html#section-4). Embedded
IPv4 input may be formatted as equivalent hexadecimal groups. A 64-byte output
buffer suffices for every supported address including scope and port. Formatting
uses a fixed 64-byte local workspace; parsing uses fixed small arrays. Neither
allocates.

Public address fields allow explicit construction; callers must preserve the
IPv4 trailing-zero invariant. IPv4 adapters reject a nonzero scope ID. IPv6
listeners explicitly use `IPV6_V6ONLY`, so IPv4 and IPv6 listeners have separate
binds. Port zero asks the OS to choose a port; `local_address()` reports it.

## TCP ownership, progress and readiness

```dodo
import "std/net"
import "std/net/native"

fn connect_local() -> native.TcpConnection!net.Error {
    address := net.parse_socket(b"127.0.0.1:8080")?
    clock := native.MonotonicClock {}
    cancel := net.NeverCancel {}
    now := clock.now_ms()
    if now > (~0u64) - 2000 { return err(net.failure(net.ErrorKind.TimedOut)) }
    operation := net.Operation.until(now + 2000)
    return native.connect_with(&address, &operation, &mut clock, &cancel)
}
```

`TcpConnection.connect(&address)` creates an owned socket and starts a
nonblocking connection attempt. `poll_connected()` returns false while pending,
true after success or a typed error. It must succeed before stream I/O; an
uncompleted connection reports `WouldBlock`. A failed connect remains failed and
cannot be retried or mistaken for a later successful connection; close it and
create a new connection. `peer_address()` and `local_address()` return owned
portable values.

`TcpListener.bind(&address, backlog)` requires a positive backlog, clamped to the
platform signed integer range; the OS may further constrain it. `accept()` returns
`none` when no connection is available, `some(TcpConnection)` on success or an
error. There is no hidden worker thread. Accepted sockets are nonblocking.

`PollSet.new()` batches readiness checks for up to `POLL_CAPACITY` (256) sockets
using one native poll call. Register a listener with `listener.register(&mut set)`
or a connection with `connection.register(&mut set, interest)`; each returns an
index for `set.ready(index)`. `set.wait(timeout_ms)` updates readiness and returns
the number of ready registrations; an empty set returns zero immediately.
Readiness includes hangup/error notifications: the next socket operation reports
the result. `set.clear()` removes registrations for the next batch. Registrations
borrow native handle values without transferring ownership; keep every registered
owner open until the wait finishes. `TcpConnection.closed()` supplies an empty,
safe-to-drop owner for caller-owned connection slots.

Every socket owns exactly one native descriptor/handle, moves as an aggregate,
and releases it on `close()` or `drop`. `close()` is idempotent. A close error
still invalidates the owner, and Linux interrupted close is never retried against
a possibly reused descriptor. Destruction ignores close errors; call `close()`
explicitly to observe them. Explicit raw handle access is unsafe and borrowed:
`raw_handle()` never transfers ownership and does not authorize closing the
handle or retaining an OS operation beyond the checked owner's lifetime.

`read(&mut[u8]) -> usize!io.Error` and `write(&[u8]) -> usize!io.Error` perform one
nonblocking OS attempt. Partial transfers are successful. A nonempty read of zero
means EOF; empty slices return zero without accessing the socket. Pending I/O is
`io.ErrorKind.WouldBlock`. `poll_read` and `poll_write` instead report
`io.PollState.Pending` with zero progress. Read EOF is `io.PollState.Eof`.
Platform errors retain their numeric code, and transferred counts refer to this
call's initialized/accepted prefix. TCP syscall lengths are capped at `INT_MAX`,
so larger slices naturally make partial progress. Linux sends suppress SIGPIPE.

`shutdown(net.Shutdown.Write)` half-closes transmission while retaining reads;
`Read` and `Both` select the other directions. Receiving EOF does not close the
write direction. A connection owns a pending connect attempt until successful,
closed or dropped. All data syscalls are synchronous attempts on nonblocking
sockets: no overlapped operations, callbacks or retained caller pointers exist.
Every slice borrow covers the entire foreign call, and the OS has finished
accessing that memory before the call returns. Pending data can therefore be
moved or reused after return. A future asynchronous adapter must keep its own
operation state and buffer borrows alive until OS completion, including after a
cancellation request.

`connection.wait_ready(Interest.Read | Write, milliseconds)` waits explicitly for
readiness, returning false on timeout. A listener has `wait_ready(milliseconds)`.
Readiness includes disconnect/error conditions and is only a suggestion to retry;
an operation can still return `WouldBlock`. Waits are one synchronous poll call,
clamped to `INT_MAX` milliseconds, and may return `Interrupted`.

## Deadlines, cancellation and execution

A nonblocking call is an attempt, not a loop that waits for completion. Handle
its result according to progress:

| Result | Action |
| --- | --- |
| A positive byte count | Advance by exactly that count before trying the remainder. |
| Zero from a nonempty TCP read | Finish reading: the peer sent EOF. |
| `WouldBlock` or `PollState.Pending` | Wait for relevant readiness, then retry under your operation policy. |
| `Interrupted` | Recheck cancellation/deadline before retrying. |
| Another error | Preserve any reported progress and end or explicitly recover the operation. |

For example, if an eight-byte message is accepted in chunks of three and five,
the second write receives `message[3..]`. Re-sending all eight bytes would
duplicate the prefix. `operations.write_all` maintains that offset for you.

A deadline is an **absolute** millisecond value, whereas `wait_ready` takes a
**relative** wait duration. Form the deadline once and keep it across partial
progress; recomputing `now + timeout` after every byte can let a slow peer keep
an operation alive indefinitely. Check integer addition before constructing it,
as the quickstart does; a failed native clock reads as the maximum `u64`.

`Operation.until(absolute_milliseconds)` carries a deadline in the supplied
clock's domain. `Operation.unlimited()` has no deadline. Structural clock providers
implement `now_ms(&mut self) -> u64` and must be monotonic. Cancellation providers
implement `cancelled(&self) -> bool`. `NeverCancel` always returns false;
`Cancellation.new()` and `.cancel()` provide caller-local cancellation. A shared
cross-thread provider can implement `cancelled()` with `std/sync` atomics; ordinary
`Cancellation` is not a concurrent cancellation token.

`operation.check(&mut clock, &cancel)` checks cancellation first, then rejects a
deadline at or before now. It opens no socket. An unlimited operation does not
read the clock. `native.MonotonicClock` explicitly selects the OS monotonic clock;
its timestamps cannot be compared with another clock provider's timestamps. A
native clock failure returns the maximum timestamp, causing bounded operations
to fail closed as timed out.

`native.connect_with` composes connection polling with these providers. It checks
before opening and before every connect poll, then waits for at most 10 ms per
pending attempt. Timeout/cancellation drops and closes the in-progress socket.
`std/net/operations.read` and `.write_all` apply the same explicit policy to any
stream with `poll_read`, `poll_write` and `wait_ready`. `write_all` retains total
progress in its error, including when a deadline or cancellation occurs between
partial writes. Already accepted bytes are never replayed. `read` returns after
any progress or EOF; it does not fill the whole destination. Empty operations
return immediately without clock or cancellation calls.

These are explicit blocking compositions of nonblocking attempts. Cancellation
and deadline checks occur between attempts, with latency up to one 10 ms wait
plus one OS attempt and provider execution. Providers must finish promptly.
There is no scheduler, background execution, hidden allocation or busy retry
loop. `std/io.write_all` remains available but stops on `WouldBlock`; selecting
a completion policy is the caller's responsibility.

## UDP datagrams

`UdpSocket.bind(&address)`, `local_address()`, `wait_ready(interest, milliseconds)`
and `close()` follow the TCP ownership and execution rules. `send_to(data,
&destination)` sends exactly one packet, including an empty packet. A partial
native send becomes an error with its actual transferred count. Oversize sends
fail; they are never split into multiple datagrams.

`receive_from(&mut buffer)` consumes exactly one message and returns
`Datagram { transferred, peer, truncated, original_size }`. Truncated suffixes
are discarded, never exposed as the next read. A zero-capacity destination still
consumes a packet, reporting truncation if it was nonempty. Empty datagrams are
successful messages, not EOF. `original_size` is `some(full_length)` on Linux;
Windows reports `none` for an oversized packet because Winsock does not provide
the original length after consuming it. The copied prefix is always initialized
and bounded by the supplied slice. UDP has no stream `read`/`write` methods,
preventing accidental loss of message boundaries.

## DNS messages and resolution policy

`std/net/dns` provides a bounded [RFC 1035](https://www.rfc-editor.org/rfc/rfc1035.html)
codec: header parsing, question serialization, compressed-name expansion,
question/record iteration and borrowed RDATA. `query(name, id, type, recursion,
output)` accepts an ASCII presentation name, an optional final dot and explicit
transaction ID, record type and recursion decision. A query requires at most
271 bytes and leaves caller output unchanged on failure.

`expand_name(packet, offset, output)` emits uncompressed DNS wire labels,
including their length octets and the final zero; arbitrary label bytes remain
unambiguous. Labels are at most 63 bytes, expanded names at most 255 bytes, and
expansion performs at most 128 steps. Compression pointers must refer backward;
forward/self pointers, truncated pointers, invalid label tags and capacity
exhaustion fail. This avoids the compression-loop and unchecked-offset problems
described in [RFC 9267](https://www.rfc-editor.org/rfc/rfc9267.html).

`Reader.new(packet)` borrows the message. Consume questions using
`next_question(name_workspace)` before `next_record(name_workspace)`, then call
`finish()` to reject missing records or trailing bytes. Output names borrow the
caller workspace; RDATA borrows the packet. The compiler rejects mutation or
release of either storage while a returned view is live. Name-expansion failure
may leave an initialized prefix in workspace, but no valid view is returned.
Reader advancement is transactional on parse failure. Header counts preserve
answer/authority/additional section sizes for policy consumers; the record
iterator visits those sections consecutively. RDATA interpretation, EDNS,
DNSSEC validation, caching, retries, server discovery and TCP fallback are not
implemented by this codec.

`native.resolve(host, port, workspace)` explicitly selects libc `getaddrinfo` or
Winsock `getaddrinfo`, independent of the DNS codec. Host input is 1–253 ASCII
bytes without NUL or whitespace. Internationalized names require an independently
selected IDNA integration. The provider accepts a caller workspace with 24 bytes
per result; at least one slot is required. It deduplicates addresses and returns
borrowed `Addresses`, with `len()`, `get(index)` returning owned socket addresses,
and a `truncated` flag when caller capacity is exhausted. Never assume the first
result is preferred or connect all results implicitly. `resolve_numeric` selects
the platform numeric parser with `AI_NUMERICHOST`, so it never consults a name
service; portable strict numeric syntax remains available through `parse_ip`. `NameNotFound` and
`Exhausted` distinguish common resolver failures; other native codes are retained.

The platform resolver may allocate internally and always frees its result chain
before returning. It is a synchronous, potentially blocking call with OS resolver
policy, ordering and configuration. It cannot be interrupted by `Operation` or
cancelled mid-call. Callers needing bounded/cancellable DNS must supply a resolver
using the portable message codec and their transport/clock/entropy policy; this
release does not provide that DNS resolver implementation. A caller can isolate
platform resolution in a explicitly managed thread, retaining all borrowed
storage until it finishes. No API claims that dropping a waiter stops the OS
resolver.

## Providers, dependencies and verification

The native boundary is bundled C11 source compiled against actual system socket
headers, avoiding hand-declared platform structure layouts. Linux uses libc
sockets, `poll`, `getaddrinfo` and `CLOCK_MONOTONIC`. Windows uses Winsock 2.2,
`WSAPoll`, `getaddrinfo` and the shared `std/time/hosted` performance counter, linking `ws2_32` plus the existing
Windows runtime libraries. Winsock initialization happens once per process under
`InitOnceExecuteOnce`; the process-wide startup reference remains until process
exit. Socket options and local-domain sockets have no portable API in this
release and belong in separately selected extensions. All Dodo network sources
and their C bridge use the repository BSD-2-Clause license; OS provider licensing
and implementation updates belong to the platform.

`cargo test --test net_library` exercises fixtures at `-O0` and `-O3`, IPv4 and
IPv6 loopback TCP/UDP, half-close and EOF, partial reads, write backpressure,
slow-peer deadlines, cancellation with partial progress, resource exhaustion and
cleanup, name-resolution errors, independent Rust TCP peers, parser properties
and malformed DNS input, checked borrowing and moved socket rejection. Portable
fixtures cross-compile for wasm32 and Cortex-M0; native fixtures cross-compile to
Windows x64. `scripts/test_stdlib_windows.py --fixture tests/net/native_checks.dodo`
executes real Windows binaries under Wine, including UDP truncation. Native
Windows CI runs the loopback and backpressure fixtures at both optimization
levels and checks connected/accepted socket inheritability with
`scripts/test_windows_native.py`. This does not cover all network drivers,
IPv6 interface scopes, resolver configurations, socket exhaustion, or load.
No Internet endpoint is required: the negative resolver case uses an invalid
numeric address with `AI_NUMERICHOST`, and hostname resolution uses `localhost`.

## Complete API reference

For every public type, field, constant, and function signature, see [std/net](api/std/net.md), [std/net/operations](api/std/net/operations.md), [std/net/dns](api/std/net/dns.md), [std/net/linux](api/std/net/linux.md), [std/net/windows](api/std/net/windows.md).
