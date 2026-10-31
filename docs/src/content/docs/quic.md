---
title: "QUIC"
description: "Verified QUIC connections and multiplexed streams over UDP with OpenSSL 3.5, driven by explicit event processing."
section: "Standard library"
order: 158.5
---

QUIC carries many independent, reliable byte streams over one encrypted UDP
connection. Every connection is TLS 1.3: a client must trust the server's
certificate chain **and** verify that it names the requested host or IP, exactly
as in [TLS](tls.md). Unlike TCP plus TLS, a slow or lost packet on one stream
does not stall the others, and opening a stream costs no extra round trip.

`std/quic/openssl` binds the QUIC client and server stack in OpenSSL 3.5. The
backend owns its UDP socket, packet buffers, protocol timers and stream buffers;
your program chooses when to wait and when to process events. Every call is
nonblocking.

## Choose an entry point

| Need | Starting point | Your responsibility |
| --- | --- | --- |
| Connect to a QUIC server | `openssl.Connection.connect` | Resolve the address, supply trust and ALPN, drive the handshake. |
| Accept QUIC connections | `openssl.Listener.bind` and `accept` | Supply the certificate chain and key; handshake each accepted connection. |
| Exchange messages | `open_bidirectional`, `open_unidirectional`, `accept_stream` | Frame messages on each stream; conclude or reset it. |
| Wait without spinning | `wait(milliseconds)` or `timeout_ms` plus your own poller | Call again until operations stop reporting `Pending`. |
| Share error handling across backends | Portable `std/quic` | Map `quic.Error` kinds to your protocol's policy. |

QUIC is not HTTP/3. This package moves bytes on streams; an application
protocol (your own framing, or a future HTTP/3 layer) defines what they mean.

## Quickstart

This hosted example requires Linux GNU, macOS, or Windows x64 and **OpenSSL 3.5+
headers, libssl and libcrypto**, set up as in the [TLS quickstart](tls.md#quickstart);
`dodo run` adds the libraries automatically. One process plays both roles over
loopback, so no peer program or Internet service is needed.

In a fresh disposable example directory, generate a two-day local certificate
and key:

```sh
openssl req -x509 -newkey rsa:2048 -noenc -keyout key.pem -out cert.pem -days 2 -subj /CN=localhost -addext "subjectAltName=DNS:localhost,IP:127.0.0.1"
```

Save this as `quic_start.dodo`:

```dodo
package quic_start
import "std/console"
import "std/fs"
import "std/io"
import "std/net"
import "std/platform"
import "std/quic"
import "std/quic/openssl"

const ALPN: &[u8] = b"\x0bhello-quic1"

// Process both endpoints' network events; one process plays both roles here.
fn turn(listener: &mut openssl.Listener, client: &mut openssl.Connection) -> void!quic.Error {
    client.wait(5)?
    return listener.wait(5)
}
fn exchange(certificate: &[u8], key: &[u8]) -> i32!quic.Error {
    server_config := openssl.Config.server(certificate, key, ALPN)
    any := net.SocketAddress.new(net.IpAddress.v4(127, 0, 0, 1), 0)
    listener := openssl.Listener.bind(&server_config, &any)?
    bound := listener.local_address()?

    client_config := openssl.Config.client(b"localhost", ALPN)
    client_config.system_roots = false
    client_config.trust_pem = certificate
    client := openssl.Connection.connect(&client_config, &bound)?

    request: Option<openssl.Stream> = none
    accepted: Option<openssl.Connection> = none
    reply := [0u8; 64]
    used := 0usize
    for i in 0..2000 {
        // Server: accept the connection, then answer each stream it opens.
        match listener.accept()? {
            some(connection) => { accepted = some(connection) }
            none => {}
        }
        match &mut accepted {
            some(connection) => {
                match connection.accept_stream()? {
                    some(stream) => {
                        sent := stream.send(b"Hello, QUIC!\n")?
                        if sent.transferred != 13 { return ok(5) }
                        stream.conclude()?
                    }
                    none => {}
                }
            }
            none => {}
        }
        // Client: once verified, open one stream and read the reply to its end.
        if client.handshake()? == quic.State.Ready {
            match &mut request {
                some(stream) => {
                    progress := stream.receive(&mut reply[used..])?
                    used += progress.transferred
                    if progress.state == quic.State.Finished {
                        output := console.stdout()
                        match io.write_all(&mut output, &reply[..used]) {
                            ok(_) => { return ok(0) }, err(_) => { return ok(3) },
                        }
                    }
                }
                none => {
                    request = client.open_bidirectional()?
                    match &mut request {
                        some(stream) => { stream.conclude()? }
                        none => {}
                    }
                }
            }
        }
        turn(&mut listener, &mut client)?
    }
    return ok(2)
}
// Reads a whole PEM file, or none if it is missing or larger than `storage`.
fn load(name: &str, storage: &mut[u8]) -> Option<usize> {
    path := platform.workspace()
    match fs.read_file(name, storage, &mut path) {
        ok(report) => {
            if report.eof { return some(report.read) }
            return none
        },
        err(_) => { return none },
    }
}
fn main() -> i32 {
    certificate := [0u8; 4096]
    key := [0u8; 4096]
    cert_length := 0usize
    key_length := 0usize
    match load("cert.pem", &mut certificate) {
        some(length) => { cert_length = length }, none => { return 4 },
    }
    match load("key.pem", &mut key) {
        some(length) => { key_length = length }, none => { return 4 },
    }
    match exchange(&certificate[..cert_length], &key[..key_length]) {
        ok(code) => { return code },
        err(reason) => {
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
dodo run quic_start.dodo
```

Expected stdout is `Hello, QUIC!` followed by a newline. Exit 0 means the client
verified the server certificate for `localhost`, both sides agreed on the ALPN
protocol, and the client received the server's whole reply on a stream. Delete
the example directory, including `key.pem`, when finished.

Exit 4 means a PEM file is missing or larger than 4096 bytes: rerun the setup
command. Exit 1 prints a `quic.Error`; `VerificationFailed` means the trust
file, hostname or validity dates do not match. Exit 2 means no reply arrived
within the loop's bound, and 3 that output failed.

The loop shows the whole model. Each turn makes every endpoint try its next step
without blocking: accept, answer, open, read. `turn` then waits a few
milliseconds for datagrams or timers and processes them. An operation that
reports `Pending` (or `none`) is retried on a later turn, never treated as
failure. The server releases its stream right after `conclude()`; a concluded
stream still delivers its buffered bytes and FIN.

## API and contracts

`std/quic` contains the portable vocabulary: `Error`, `ErrorKind`, `State`,
`Progress` and `CloseInfo`. Importing it loads no socket, cryptography, clock
or allocator, so protocol code above QUIC can be portable. `std/quic/openssl`
selects the OpenSSL backend and needs a hosted target.

| Type | Owns | Created by |
| --- | --- | --- |
| `Listener` | A bound UDP socket, the server context, connections not yet accepted | `Listener.bind(&config, &address)` |
| `Connection` | One QUIC connection; a client connection also owns its connected UDP socket | `Connection.connect(&config, &address)` or `listener.accept()` |
| `Stream` | One stream's send and receive parts | `open_bidirectional()`, `open_unidirectional()`, `accept_stream()` |

### Configuration and verification

`openssl.Config.client(hostname, alpn)` verifies the server's chain and its
hostname; IP literals match iPAddress SANs, DNS names also configure SNI, and
partial-label wildcards are disabled. `Connection.connect` takes an already
resolved `net.SocketAddress` (see [DNS](networking.md)); the hostname is checked
independently of the address. There is no option that disables client peer
verification.

`openssl.Config.server(certificate, key, alpn)` requires an identity. Set
`require_client_certificate=true` and `trust_pem` for mutual authentication.
`trust_pem`, `system_roots`, `certificate_pem`, `private_key_pem` and
`verification_time` behave as in [TLS](tls.md#verification-and-providers), with
the same 1 MiB PEM and 253-byte hostname limits. Encrypted keys are rejected
without prompting.

QUIC requires ALPN, so `alpn` is mandatory: an RFC 7301 list of length-prefixed
protocol names such as `b"\x10rift-messaging/1"`. The server selects the first
protocol in its own list that the client offers; without overlap the handshake
fails and the client sees `ConnectionClosed` with a transport `CloseInfo`
(code 376, `no_application_protocol`). `selected_alpn` copies the result.

`idle_timeout_ms` requests a client idle timeout; 0 keeps OpenSSL's 30-second
default, and the connection uses the smaller nonzero value of both peers.
OpenSSL 3.5 cannot set it on a listener, so a server `Config` with a nonzero
value fails `InvalidInput`. QUIC has no implicit keep-alive here: keep idle
connections open with your own periodic traffic, such as a protocol ping.

Construction validates and copies all configuration; no borrow survives it.
Invalid input fails `InvalidInput` before a socket exists. A bind failure
reports `Network` with the operating-system error code in `code`. Scoped IPv6
(link-local) addresses are rejected because OpenSSL's address type cannot carry
a scope.

### Handshakes and connections

`connect` and `accept` return immediately. Call `handshake()` until it reports
`Ready`; only then is the peer verified. Accepted connections may still be
handshaking, and one that fails, for example because the client rejected the
certificate, reports an error from `handshake()`: drop it and keep accepting.
`handshake()` also reports `ConnectionClosed` once a connection has ended,
including when a server rejects a client certificate after the client finished
its half of the TLS 1.3 handshake.

A client connection uses a connected UDP socket, so a peer that refuses
datagrams (no listener on that port) ends the handshake promptly with
`ConnectionClosed`, a local transport close, instead of waiting for a timeout.
An unreachable host that stays silent is bounded only by your deadline and the
idle timeout.

`handshake_until(now, deadline, cancelled)` takes an explicit monotonic time,
inclusive deadline and cancellation snapshot, like the TLS stream. Timeout or
cancellation aborts the connection.

`close(code)` sends CONNECTION_CLOSE with an application error code after
concluded streams have been flushed; call it until it reports `Finished`.
`close_info()` reports why a connection ended: its code, whether this side
closed it (including idle timeout), and whether the code is a QUIC transport
error rather than an application code. `abort()` and dropping release a
connection immediately **without** notifying the peer, which then waits for its
idle timeout; close first when the peer should know.

### Streams

`open_bidirectional()` and `open_unidirectional()` return `none` until the
handshake has completed and while the peer's stream limit is exhausted;
OpenSSL 3.5 allows each peer 100 concurrent streams of each kind and does not
make that limit configurable. `accept_stream()` returns the next stream the peer
opened, or `none`. Stream IDs follow RFC 9000: bit 0 is set for
server-initiated streams and bit 1 for unidirectional ones. `readable()`,
`writable()` and `local()` describe a stream's direction and origin.

* `send(source)` copies an accepted prefix into the stream's send buffer and
  reports `Pending` when the 512 KiB buffer or the peer's flow-control window is
  full. Accepted bytes no longer borrow your storage.
* `receive(destination)` copies an initialized prefix. `Pending` means no data
  yet; `Finished` means the peer concluded the stream and every byte was read.
* `conclude()` sends FIN after all accepted bytes; it is idempotent, and later
  sends fail `Closed`.
* `reset(code)` abandons the sending part with RESET_STREAM; the peer's next
  `receive` fails `StreamReset` with `application == code`.
* Dropping a stream that was neither concluded nor reset resets it with code 0.
  A concluded stream keeps delivering its buffered bytes.

Streams also implement the `std/io` polling contract with `read`, `write`,
`poll_read` and `poll_write`. `Pending` becomes `WouldBlock` or `io.PollState.Pending`,
and `Finished` becomes EOF. A reset or closed connection maps to
`io.ErrorKind.ConnectionReset`; `take_error()` returns the typed `quic.Error`
with its application code.

### Event processing and waiting

All objects created from one listener share its socket and event loop, so
processing events on any of them advances all of them. `process()` handles
received datagrams and expired timers without waiting. `wait(milliseconds)`
waits for socket readiness until the earlier of your bound (capped at 60 s) and
the next protocol timer, then processes events. `timeout_ms()` reports when the
next timer is due, or `none`, for integration with another poller.

The backend starts no threads. Objects are neither Send nor Sync: serialize all
calls for one listener or client connection, and its streams, on one execution
lane. Releasing a listener or connection while streams still exist is safe; the
native state lives until its last object is released.

## Errors and limits

| Kind | Meaning and next step |
| --- | --- |
| `VerificationFailed` | Check trust roots, hostname or IP SAN, validity dates and verification time; `code` is the X.509 result. |
| `ConnectionClosed` | The connection ended; `application` holds the CONNECTION_CLOSE code and `close_info()` the details. |
| `StreamReset` | The peer reset the stream or stopped reading it; `application` holds its code. Other streams are unaffected. |
| `Closed` | The object was aborted, or the stream part was concluded or reset locally. |
| `InvalidInput` | Invalid configuration, or an operation in the wrong direction such as `receive` on a local unidirectional stream. |
| `Network` | A socket operation failed; `code` is the operating-system error. |
| `TimedOut` / `Cancelled` | `handshake_until` aborted the connection. |
| `Backend` / `Protocol` | Inspect the numeric code and discard the connection. |

Errors carry only kinds and numbers, never plaintext, keys or certificate text.
QUIC error codes are at most `quic.MAX_CODE` (2^62 - 1); larger codes fail
`InvalidInput`.

OpenSSL allocates its connection, packet and stream buffers internally, so
Dodo cannot bound total backend memory; the per-stream send buffer is 512 KiB.
Server address validation (retry tokens) stays enabled. 0-RTT early data,
session resumption, datagrams (RFC 9221), connection migration control and
peer-address queries for accepted connections are not exposed. On Windows the
backend uses Winsock through OpenSSL and links `ws2_32`.

## Verification

`cargo test --test quic_library` generates a local CA and short-lived leaf/key,
then compiles and runs a Dodo fixture at `-O0` and `-O3` on IPv4 loopback. It
checks a 1 MiB stream that must hit send-buffer backpressure, the reply, a
server-initiated unidirectional stream released after FIN, stream IDs and
directions, reset codes through the typed and `std/io` paths, graceful close
codes on both sides, ALPN selection and mismatch, wrong-host, untrusted and
expired rejections, a refused port, mandatory client certificates, idle
timeout, deadline and cancellation aborts, bind conflicts, and invalid
configuration. Further checks
compile the portable `std/quic` vocabulary for WebAssembly and Cortex-M0,
reject the backend on freestanding targets, and cross-compile it for Windows
x64. No private credential is checked into the repository.

## Complete API reference

For every public type, field, constant, and function signature, see [std/quic](api/std/quic.md) and [std/quic/openssl](api/std/quic/openssl.md).
