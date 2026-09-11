---
title: "Networking"
description: "Portable IP and DNS codecs, owned nonblocking sockets, explicit execution and resolver providers."
section: "Using Dodo"
order: 150
---

`std/net` contains address values, parsers, formatting, errors and operation
policy. It has no imports and works on freestanding targets. `std/net/dns`
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
    operation := net.Operation.until(clock.now_ms() + 2000)
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
`WSAPoll`, `getaddrinfo` and `GetTickCount64`, linking `ws2_32` plus the existing
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
executes real Windows binaries under Wine, including UDP truncation. Wine success
still requires native Windows verification of provider behavior under real
network drivers, IPv6 interface scopes, resolver configuration, socket exhaustion,
and scheduling/load. No Internet endpoint is required: the negative resolver case uses an invalid
numeric address with `AI_NUMERICHOST`, and hostname resolution uses `localhost`.
