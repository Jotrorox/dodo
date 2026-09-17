---
title: "TLS"
description: "Verified TLS engines, bounded record staging, and explicit transport composition."
section: "Standard library"
order: 158
---

TLS encrypts a connection and authenticates its peer. An HTTPS client must trust
the server's certificate chain **and** verify that the certificate names the
requested host or IP. Selecting roots establishes who may issue certificates;
selecting the URL hostname establishes which server you intend to reach.

For an HTTPS request, start with `std/http/https.Client.new` and explicit trust.
It combines the hosted HTTP client with verified OpenSSL TLS. The quickstart
uses a local Python peer and a temporary certificate, so no Internet service or
existing credentials are required.

## Choose a TLS integration

| Need | Starting point | Your responsibility |
| --- | --- | --- |
| Fetch an HTTPS URL | `std/http/https.Client` | Supply trust, workspace, and response limits. |
| Serve an HTTPS application | `std/web/https.files` with `std/web/app` | Supply the certificate chain and matching private key. |
| Encrypt a custom byte protocol | `std/tls/stream.Stream` plus an engine | Drive readiness, deadlines, flush, and shutdown. |
| Drive TLS records directly | `std/tls/openssl.Engine` | Feed/drain encrypted bytes and handle engine progress. |
| Supply a different backend | Portable `std/tls` contracts | Implement verification and provider lifecycle obligations. |

The lower-level engine never opens sockets. The transport wrapper never chooses
your trust policy. HTTPS is the layer that connects the URL, TCP transport,
verified TLS engine, and HTTP exchange.

## Quickstart

This hosted example requires Linux GNU x86-64 or Windows x64, Python 3 with
`ssl`, and **OpenSSL 3.5+ headers, libssl and libcrypto** for the target C toolchain.
On Fedora install `openssl-devel`; elsewhere use matching development packages
or the pinned source-build procedure in the
[compiler workflow](https://github.com/Jotrorox/dodo/blob/main/.github/workflows/ci.yml).
A runtime-only `openssl` command is not enough to compile a TLS program.
`dodo run` adds the TLS libraries automatically; custom installations may need
`--link-arg -I/path/to/include --link-arg -L/path/to/lib` and a runtime library path.

In a fresh disposable example directory, generate a two-day local certificate
and key. No credentials or public service are needed:

```sh
openssl version
openssl req -x509 -newkey rsa:2048 -noenc -keyout key.pem -out cert.pem -days 2 -subj /CN=localhost -addext "subjectAltName=DNS:localhost,IP:127.0.0.1"
```

Save this complete peer as `tls_peer.py`:

```python
import http.server
import ssl

class Hello(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def do_GET(self):
        body = b"Hello, TLS!\n"
        self.send_response(200)
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Connection", "close")
        self.end_headers()
        self.wfile.write(body)
        self.close_connection = True

context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
context.load_cert_chain("cert.pem", "key.pem")
context.set_alpn_protocols(["http/1.1"])
server = http.server.HTTPServer(("127.0.0.1", 8443), Hello)
server.socket = context.wrap_socket(server.socket, server_side=True)
print("ready on 127.0.0.1:8443", flush=True)
try:
    server.serve_forever()
finally:
    server.server_close()
```

In terminal 1 run `python3 tls_peer.py`. Wait for `ready`; if the port is occupied,
stop your previous peer or change 8443 in both programs. Use terminal 2 for Dodo.

Save this as `tls_start.dodo`:

```dodo
package tls_start
import "std/fs"
import "std/platform"
import "std/http/hosted"
import "std/http/https"
import "std/http/hosting"
import "std/console"
import "std/io"

fn fetch(roots: &[u8]) -> i32!hosting.Error {
    workspace := [0u8; hosted.WORKSPACE_BYTES]
    trust := https.Trust.pem_roots(roots)
    client := https.Client.new(&mut workspace, hosted.Config.defaults(), trust)?
    body := [0u8; 1024]
    response := client.get(b"https://127.0.0.1:8443/", &mut body)?
    if response.status != 200 { return ok(2) }
    output := console.stdout()
    match io.write_all(&mut output, &body[..response.body_bytes as usize]) {
        ok(_) => { return ok(0) }, err(_) => { return ok(3) },
    }
}
fn main() -> i32 {
    path := platform.workspace()
    pem := [0u8; 4096]
    match fs.read_file("cert.pem", &mut pem, &mut path) {
        ok(report) => {
            if !report.eof { return 4 }
            match fetch(&pem[..report.read]) {
                ok(code) => { return code },
                err(reason) => {
                    errors := console.stderr()
                    match errors.println(&reason) {
                        ok(_) => {}, err(_) => {},
                    }
                    return 1
                },
            }
        },
        err(_) => { return 4 },
    }
}
```

```sh
dodo run tls_start.dodo
```

Expected stdout is `Hello, TLS!` followed by a newline; exit 0 means the local
server certificate was trusted, its IP identity verified, and the full HTTP 200
body received. Stop the peer with Ctrl+C. Delete this disposable example
directory, including `key.pem`, when finished; never use this test key in a service.

`fs` loads only the trust certificate, and `platform` supplies its path workspace.
The PEM array stays alive while `Trust` and the client borrow it. HTTP workspace
and response-body capacity are explicit; TLS staging is bounded separately, but
OpenSSL also allocates internal state. `https` selects that external backend;
plain `std/http/hosted` never selects TLS automatically.

Exit 4 means the certificate file is missing, unreadable, or exceeds 4096 bytes:
rerun setup or increase the explicit bound. Exit 1 means connection, protocol,
or TLS failure: inspect the hosted error's `kind` and nested `security` cause.
For verification failures, check the trust file, hostname/IP SAN, certificate
validity and OS wall clock; regenerate expired local credentials. Do not disable
verification. Exit 2 is a non-200 HTTP response; exit 3 means output failed.

`https.Trust.system()` uses OpenSSL's configured trust paths, not the Windows
certificate store. Prefer explicit `pem_roots` for reproducible applications.
`https.serve` and `https.Acceptor` add TLS to the [hosted web server](web.md)
with `openssl.Config.server(certificate, key)`; both identity arrays must remain
alive. Custom providers can use `tls/stream.Stream` without importing OpenSSL.

## API and contracts

`std/tls` contains portable provider contracts and typed errors. Importing it
loads no TLS implementation, network adapter, filesystem, clock, entropy source,
or scheduler. `std/tls/openssl` selects the independently linked OpenSSL provider;
`std/tls/stream` composes any compatible engine with an `std/io` polling transport.
HTTP has no dependency on a particular TLS backend.

## Backend and build

The implemented Linux and Windows x64 provider requires **OpenSSL 3.5 or later**;
the verification environment uses **3.5.8**. The maintained 3.5 LTS branch is
[supported through April 2030](https://openssl-library.org/post/2025-02-20-openssl-3.5-lts/).
OpenSSL 3.x uses the [Apache License 2.0](https://openssl-library.org/source/license/).
Dodo neither vendors OpenSSL nor implements cryptographic primitives.

Install target OpenSSL development headers, libssl, libcrypto, and a target C
toolchain. Hosted executable linking automatically compiles the embedded C
boundary and adds `-lssl -lcrypto`; normal toolchain dynamic linking applies.
`--link-arg` can select library/include directories or explicit static linking.
Static Windows linkage additionally needs OpenSSL's system dependencies, normally
crypt32, ws2_32, advapi32, bcrypt, and the matching CRT. Object emission requires
the caller to compile `stdlib/std/tls/runtime.c` and link these dependencies.
OpenSSL shared libraries and provider modules must match the deployment ABI.

CI builds the pinned OpenSSL 3.5.8 source archive with SHA-256 verification because
the compiler test suite needs this maintained backend even on hosts whose default
development package is older. OpenSSL is a dependency of TLS programs and fixtures,
not of the Dodo compiler executable or portable HTTP/routing imports.

Windows uses the same OpenSSL backend, not Schannel. System-root mode means
**OpenSSL's configured default trust paths**, not automatic import of the Windows
certificate store. Supply explicit PEM roots for reproducible cross-platform
trust. A TLS import on an unsupported hosted target receives a compiler adapter
diagnostic; portable `std/tls` and `std/tls/stream` can compile for freestanding
targets when composed with compatible custom providers.

## Verification and providers

There are three different certificate inputs:

| Input | What it contains | Used by |
| --- | --- | --- |
| Trust roots | Certificates for issuers you choose to trust | A client verifying a server, or an mTLS server verifying clients. |
| Certificate chain | The local leaf certificate, then intermediates | A server identity or optional client identity. |
| Private key | The private key matching that local leaf | The endpoint presenting the identity. |

The client needs the test certificate as a root in this self-signed quickstart;
it does not need the server's private key. A real CA-issued server normally
sends its leaf and intermediate certificates while the client independently
selects trusted roots. Loading an identity and trusting a peer are separate
configuration decisions.

`openssl.Config.client(hostname)` enables certificate-chain verification and
requires a nonempty hostname. DNS names use certificate hostname checks with
partial-label wildcards disabled; IP literals use iPAddress SAN matching. DNS
names also configure SNI. Verification failure permanently poisons the engine;
there is no option that silently disables client peer verification. See the
[OpenSSL hostname verification API](https://docs.openssl.org/3.5/man3/SSL_set1_host/).
The caller supplies an ASCII DNS A-label or IP literal; Unicode/IDNA conversion
belongs to an optional name-processing integration.

`trust_pem` adds a caller-provided PEM certificate bundle; `system_roots` chooses
whether to load platform OpenSSL defaults as well. Setting it false with an empty
bundle produces an empty trust store, so peer verification fails. Set
`certificate_pem` and `private_key_pem` to provide an optional client identity.
A certificate PEM may contain leaf followed by intermediate certificates.
Encrypted PEM keys are rejected without prompting or reading standard input.
Credential providers can load/decrypt bytes outside this package and supply them
at construction. The caller remains responsible for wiping its own key bytes.

`Config.server(certificate, key)` requires an identity. Ordinary TLS servers do
not request client identities. Set `require_client_certificate=true` and supply
trust roots for mandatory verified mutual TLS. ALPN is an explicit RFC 7301
length-prefixed byte list; server preference determines selection. If both sides
offer lists without overlap, the handshake fails. If a peer omits ALPN, the
negotiated protocol is empty, and the application decides whether to continue.

`verification_time=-1` uses OpenSSL's platform wall clock. Nonnegative Unix
seconds select an explicit certificate-verification time, allowing an external
time provider or deterministic tests. Entropy comes from OpenSSL's maintained
platform CSPRNG/provider configuration; failure aborts the operation. This backend
does not expose a custom entropy callback or allow weak test randomness. A
separate engine can implement the portable contract with different providers.

TLS 1.2 and TLS 1.3 are enabled. Compression, renegotiation, session caches and
server session tickets are disabled. Early data, resumption policy, DTLS, custom
cipher configuration, OCSP fetching, and certificate revocation policy are not
implemented by this API. Ordinary chain, validity, purpose and hostname checks
remain mandatory for clients.

## Incremental engine and ownership

An incremental engine makes a bounded amount of progress on each call. A
`NeedInput` result means more encrypted peer bytes are required; `NeedOutput`
means encrypted output must be drained and delivered. Neither means the
handshake failed. On either state, preserve offsets for already consumed or
accepted bytes and arrange transport progress before retrying.

The lifecycle is: construct an engine, finish the handshake, exchange plaintext,
flush accepted output, exchange TLS `close_notify`, and release resources.
Dropping an engine performs resource cleanup but does not send a TLS shutdown
message. Applications that require a clean protocol shutdown must drive it
explicitly before dropping the transport.

Create an `openssl.Engine` with `client(&config)` or `server(&config)`. Construction
copies/parses configuration; no configuration borrow survives. The engine owns
the context, SSL state, credentials, encrypted rings and plaintext retry storage.
It is neither implicitly Send nor Sync: serialize operations on one execution
lane. The backend never starts workers or accesses sockets.

* `handshake()` returns `Ready`, `NeedInput`, or `NeedOutput`.
* `feed(ciphertext)` accepts a prefix into the inbound ring; zero means the ring
  is full. Drive the engine before feeding more.
* `drain(destination)` removes an encrypted prefix; zero means no output is
  presently queued, not transport EOF.
* `read_plain(destination)` and `write_plain(source)` return state and exact
  initialized/accepted prefix counts. Empty requests succeed without I/O.
* `flush()` completes an accepted plaintext retry. Encrypted bytes still need
  `drain` and transport delivery.
* `shutdown()` incrementally exchanges close_notify. Keep draining until output
  reaches the peer; `Closed` means both TLS notifications were exchanged locally.
* `eof()` reports transport EOF after feeding all preceding ciphertext. Missing
  close_notify fails with `Truncated`, never a successful plaintext EOF.
* `abort()` destroys the engine immediately; it is idempotent. Destruction also
  aborts and does not perform network I/O.

Each call borrows its input/output only until return. The backend copies accepted
writes into a private 16 KiB buffer and uses that exact address/data for any
OpenSSL retry. This satisfies [OpenSSL's retry requirements](https://docs.openssl.org/3.5/man3/SSL_write/)
without asking callers to keep a borrowed source alive across calls. Accepted
bytes can still be lost if the connection subsequently fails or is aborted;
acceptance is not acknowledgement by the peer. Errors expose numeric backend
codes, never plaintext, keys, certificates, or secret diagnostic formatting.

## Transport composition and limits

TLS may need both read and write progress even when the application is doing
only one of them. For example, a plaintext write can require a peer handshake
message, and a read can generate encrypted output. Drive the stream's pending
ciphertext as well as the requested application operation; assuming that a
read can only need socket readability can stall a connection.

`stream.Stream.new(engine, &mut transport, incoming, outgoing)` returns a Result,
owns the engine, and exclusively borrows an `std/io` polling transport plus two
caller-supplied nonempty byte slices. The checked borrows prevent transport and
scratch reuse, movement or destruction while the wrapper lives. `pump()` performs at
most one transport write and one read, retains partial ciphertext progress, and
never busy-waits. Caller-selected scratch lengths bound staging without allocation; even one-byte
buffers are supported. Empty scratch fails `InvalidInput` during construction.
The engine uses two fixed 32 KiB BIO rings plus its 16 KiB plaintext retry buffer.

`transport()` provides checked shared access to the underlying transport for
provider-specific readiness queries; end that view before mutating the stream.
`output_pending()` reports staged ciphertext. These observations add no readiness
method requirement to the portable polling transport contract.

`Stream.suspend()` moves the engine and progress offsets into an opaque
`stream.State<E>`, releasing socket and buffer borrows. Resume it with
`Stream.resume(state, &mut transport, incoming, outgoing)` and the same
ciphertext buffer contents; pending ciphertext and partial records survive
between turns. Resumption checks that saved offsets fit the supplied buffers.
This lets bounded reactors keep TLS state without holding buffer borrows
between polling turns.

`poll_read`/`poll_write` integrate with `std/io`; one-attempt `read`/`write` report
`WouldBlock` when no progress is possible. `flush` reports `Ready` only after
accepted plaintext and all staged encrypted output reach the transport. The
caller chooses readiness polling, blocking threads, or another execution model.
`handshake_until(now, deadline, cancelled)` takes an explicit clock observation
and cancellation snapshot. Equality reaches the deadline. Timeout/cancellation
permanently aborts the stream; the underlying transport must then be closed by
its owner. The same caller-controlled policy should bound body I/O, flush and
shutdown attempts. No background operation or OS buffer borrow survives a call.

Maximum hostname length is 253 bytes; ALPN wire lists are at most 65,535 bytes
with nonempty elements of at most 255 bytes. Each PEM input is capped at 1 MiB.
The peer certificate-list limit is 64 KiB with verification depth 16. Invalid
configuration fails before creating a live connection. Full rings/staging return
partial progress or a pending state; they never grow. ALPN destination shortage
returns `LimitExceeded` without copying a partial protocol identifier.

OpenSSL allocates internal cryptographic and handshake state, so these caps do
not constitute a hard upper bound on total backend heap use. Direct wrapper
allocation failures report `AllocationFailed`; OpenSSL internal failures report
`Backend` or `Protocol` with an opaque code and permanently invalidate an active
session. Construction unwinds all partially acquired resources. There is no
Dodo global allocator choice hidden inside portable protocol packages.

## Verification

Common runtime failures have different remedies:

| Kind | Meaning and next step |
| --- | --- |
| `VerificationFailed` | Check trust roots, the URL/hostname or IP SAN, validity dates, and verification time. |
| `Truncated` | Transport EOF arrived without a valid TLS close notification; do not treat it as a clean end of plaintext. |
| `TimedOut` / `Cancelled` | The stream has aborted; close its transport and start a new exchange only under an explicit retry policy. |
| `LimitExceeded` | Check the documented field/buffer bounds; larger application buffers cannot remove backend hard caps. |
| `Backend` / `Protocol` | Inspect the numeric cause and discard the invalid session. |

At the HTTPS layer these causes appear in `hosting.Error.security` when
`kind == hosting.ErrorKind.Tls`. Preserve any reported delivered body prefix;
an error does not retract bytes already handed to a sink.

`cargo test --test tls_library` generates a local CA and short-lived leaf/key,
then compiles and runs Dodo fixtures at `-O0` and `-O3`. It checks one-byte
handshake fragmentation, ALPN selection/mismatch, mutual authentication and
missing-client-identity rejection, wrong-host/untrusted/expired
rejection, bounded write exhaustion, caller-buffer mutation during pending
writes, close_notify, truncation, cancellation, exact deadline boundaries,
idempotent cleanup, transport/scratch-borrow escape and reuse rejection, and
portable object output for
WebAssembly and Cortex-M0. No private credential is checked into the repository.

The Linux interoperability test composes `std/http/client.Client` over Dodo's
TLS stream and TCP adapter against Python's independent `ssl` server on controlled
IPv4 loopback. It checks ALPN and local trust, serialized requests, incremental
response parsing with three-byte input scratch, one-byte body consumption,
connection nonreuse, and bilateral close_notify at both optimization levels. Native allocation-failure injection checks constructor cleanup at both
levels. `scripts/test_tls_windows.py` cross-links the credential/engine fixture and the
HTTP/TLS/TCP loopback client with an explicitly supplied Windows OpenSSL build.
It executes four real PE fixtures under Wine: both client/server engines and
both HTTPS loopback optimization levels. Native Windows deployment still needs checks
of installed OpenSSL trust paths, provider loading and operating-system entropy
behavior; Wine cannot establish those native-environment properties.

## Hosted convenience layer

Use `std/http/https` for verified URL requests and a hosted HTTPS listener with
library-owned connection, readiness, deadline, and body-transfer loops.
See [Hosted HTTP and HTTPS](hosted-http.md) for complete small programs, explicit
storage bounds, resolver/deadline scope, cancellation, and a generated local
HTTPS setup. The TLS engine and transport APIs remain usable independently.

## Complete API reference

For every public type, field, constant, and function signature, see [std/tls](api/std/tls.md), [std/tls/stream](api/std/tls/stream.md), [std/tls/openssl](api/std/tls/openssl.md).
