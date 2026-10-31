/* OpenSSL 3.5+ QUIC backend. Apache-2.0 dependency, dynamically linked by default.
 * OpenSSL owns the UDP socket, packet buffers, timers and stream buffers. Every
 * call is nonblocking and borrows Dodo memory only until it returns. */
#if defined(__APPLE__)
#define _DARWIN_C_SOURCE
#elif !defined(_WIN32)
#define _POSIX_C_SOURCE 200809L
#endif
#ifdef _WIN32
/* WSAPoll needs Vista; Winsock 2 must precede headers that pull in windows.h. */
#if !defined(_WIN32_WINNT) || _WIN32_WINNT < 0x0600
#undef _WIN32_WINNT
#define _WIN32_WINNT 0x0600
#endif
#define WIN32_LEAN_AND_MEAN
#include <winsock2.h>
#define dodo_quic_poll WSAPoll
#else
#include <sys/socket.h>
#include <netinet/in.h>
#include <poll.h>
#define dodo_quic_poll poll
#endif
#include <openssl/ssl.h>
#include <openssl/err.h>
#include <openssl/pem.h>
#include <openssl/x509v3.h>
#include <stdint.h>
#include <string.h>
#include <limits.h>
#if OPENSSL_VERSION_NUMBER < 0x30500000L
#error "Dodo QUIC requires a maintained OpenSSL 3.5 or later backend"
#endif

enum { DODO_QUIC_READY=0, DODO_QUIC_PENDING=1, DODO_QUIC_FINISHED=3, DODO_QUIC_INVALID=-1, DODO_QUIC_ALLOC=-2,
       DODO_QUIC_VERIFY=-3, DODO_QUIC_PROTOCOL=-4, DODO_QUIC_DEAD=-6, DODO_QUIC_RESET=-7,
       DODO_QUIC_CONN_CLOSED=-8, DODO_QUIC_BACKEND=-9, DODO_QUIC_LIMIT=-10, DODO_QUIC_NETWORK=-11 };
enum { DODO_QUIC_PEM_LIMIT=1048576, DODO_QUIC_CHAIN_LIMIT=65536, DODO_QUIC_WAIT_LIMIT=60000 };
enum { DODO_QUIC_LISTENER=1, DODO_QUIC_CONNECTION=2, DODO_QUIC_STREAM=3 };

/* Context and server ALPN list shared by a listener and its accepted
 * connections and streams. OpenSSL's ALPN callback borrows it, so it lives
 * until the last Dodo object built from it is released. Objects are not
 * Send, so the count is only touched by the owning execution lane. */
typedef struct {
    size_t references;
    SSL_CTX *ctx;
    unsigned char *alpn;
    unsigned int alpn_len;
} dodo_quic_shared;
typedef struct {
    dodo_quic_shared *shared;
    SSL *ssl;
    int kind, failed, concluded;
} dodo_quic;

static void release(dodo_quic_shared *shared) {
    if (!shared || --shared->references) return;
    SSL_CTX_free(shared->ctx);
    OPENSSL_free(shared->alpn);
    OPENSSL_free(shared);
}
static int native_code(void) {
    return (int)(ERR_peek_last_error() & INT_MAX);
}
/* Report the queued operating-system errno/WSA code when present (OpenSSL
 * queues it before its own BIO reason), otherwise an opaque OpenSSL code. */
static int socket_code(void) {
    unsigned long native, last = 0;
    while ((native = ERR_get_error()) != 0) {
        if (ERR_SYSTEM_ERROR(native)) { ERR_clear_error(); return (int)ERR_GET_REASON(native); }
        last = native;
    }
    return (int)(last & INT_MAX);
}
static int reject(dodo_quic *s, int kind, int *code) {
    if (code) *code = native_code();
    if (s) s->failed = kind;
    return kind;
}
/* Streams report their connection's termination. */
static int closed(dodo_quic *s, SSL_CONN_CLOSE_INFO *info) {
    if (s->kind == DODO_QUIC_LISTENER) return 0;
    int result = SSL_get_conn_close_info(SSL_get0_connection(s->ssl), info, sizeof(*info)) == 1;
    ERR_clear_error();
    return result;
}
enum { DODO_QUIC_ANY=0, DODO_QUIC_READING=1, DODO_QUIC_WRITING=2 };
/* Classify a failed nonblocking OpenSSL call. A stream outcome concerns only
 * the operation's direction. Verification and protocol failures are permanent;
 * resets and connection closure are re-derived so codes stay observable. */
static int ssl_result(dodo_quic *s, int result, int direction, int *code, uint64_t *application) {
    int error = SSL_get_error(s->ssl, result);
    if (error == SSL_ERROR_ZERO_RETURN) return DODO_QUIC_FINISHED;
    if (direction == DODO_QUIC_READING) {
        int state = SSL_get_stream_read_state(s->ssl);
        if (state == SSL_STREAM_STATE_RESET_REMOTE) {
            (void)SSL_get_stream_read_error_code(s->ssl, application);
            return DODO_QUIC_RESET;
        }
        if (state == SSL_STREAM_STATE_RESET_LOCAL || state == SSL_STREAM_STATE_FINISHED) return DODO_QUIC_DEAD;
    }
    if (direction == DODO_QUIC_WRITING) {
        int state = SSL_get_stream_write_state(s->ssl);
        if (state == SSL_STREAM_STATE_RESET_REMOTE) {
            (void)SSL_get_stream_write_error_code(s->ssl, application);
            return DODO_QUIC_RESET;
        }
        if (state == SSL_STREAM_STATE_RESET_LOCAL || state == SSL_STREAM_STATE_FINISHED) return DODO_QUIC_DEAD;
    }
    long verified = SSL_get_verify_result(s->ssl);
    if (verified != X509_V_OK) {
        if (code) *code = (int)verified;
        s->failed = DODO_QUIC_VERIFY;
        return DODO_QUIC_VERIFY;
    }
    SSL_CONN_CLOSE_INFO info;
    if (closed(s, &info)) {
        if (code) *code = native_code();
        if (application) *application = info.error_code;
        return DODO_QUIC_CONN_CLOSED;
    }
    if (error == SSL_ERROR_WANT_READ || error == SSL_ERROR_WANT_WRITE) {
        ERR_clear_error();
        return DODO_QUIC_PENDING;
    }
    if (error == SSL_ERROR_SYSCALL) return reject(s, DODO_QUIC_NETWORK, code);
    return reject(s, DODO_QUIC_PROTOCOL, code);
}
static int alpn_select(SSL *ssl, const unsigned char **out, unsigned char *length,
                       const unsigned char *in, unsigned int inlen, void *arg) {
    (void)ssl;
    dodo_quic_shared *shared = arg;
    unsigned char *selected = NULL;
    if (SSL_select_next_proto(&selected, length, shared->alpn, shared->alpn_len, in, inlen)
        != OPENSSL_NPN_NEGOTIATED) return SSL_TLSEXT_ERR_ALERT_FATAL;
    *out = selected;
    return SSL_TLSEXT_ERR_OK;
}
static int valid_alpn(const unsigned char *data, size_t len) {
    if (!len || len > 65535) return 0;
    size_t i = 0;
    while (i < len) {
        size_t n = data[i++];
        if (!n || n > len-i) return 0;
        i += n;
    }
    return 1;
}
static int load_roots(SSL_CTX *ctx, const unsigned char *pem, size_t len) {
    BIO *input = BIO_new_mem_buf(pem, (int)len);
    if (!input) return 0;
    int count = 0;
    for (;;) {
        X509 *cert = PEM_read_bio_X509(input, NULL, NULL, NULL);
        if (!cert) break;
        if (X509_STORE_add_cert(SSL_CTX_get_cert_store(ctx), cert) != 1) {
            X509_free(cert); BIO_free(input); return 0;
        }
        X509_free(cert); ++count;
    }
    BIO_free(input);
    ERR_clear_error();
    return count != 0;
}
static int no_password(char *buf, int size, int writing, void *arg) {
    (void)buf; (void)size; (void)writing; (void)arg;
    return 0; /* Never prompt, read stdin, or expose key bytes. */
}
static int load_identity(SSL_CTX *ctx, const unsigned char *certificate, size_t clen,
                         const unsigned char *key, size_t klen) {
    BIO *input = BIO_new_mem_buf(certificate, (int)clen);
    if (!input) return 0;
    X509 *cert = PEM_read_bio_X509(input, NULL, NULL, NULL);
    if (!cert) { BIO_free(input); return 0; }
    int result = SSL_CTX_use_certificate(ctx, cert);
    X509_free(cert);
    while (result == 1) {
        cert = PEM_read_bio_X509(input, NULL, NULL, NULL);
        if (!cert) break;
        result = SSL_CTX_add1_chain_cert(ctx, cert);
        X509_free(cert);
    }
    BIO_free(input);
    ERR_clear_error();
    if (result != 1) return 0;
    input = BIO_new_mem_buf(key, (int)klen);
    if (!input) return 0;
    EVP_PKEY *private_key = PEM_read_bio_PrivateKey(input, NULL, no_password, NULL);
    BIO_free(input);
    if (!private_key) return 0;
    result = SSL_CTX_use_PrivateKey(ctx, private_key);
    EVP_PKEY_free(private_key);
    return result == 1 && SSL_CTX_check_private_key(ctx) == 1;
}
/* 24-byte Dodo socket address: family 4/6, pad, big-endian port, little-endian
 * IPv6 scope, then 16 address octets (IPv4 uses the first four). BIO_ADDR has
 * no scope setter, so construction rejects scoped link-local addresses. */
static BIO_ADDR *decode_address(const unsigned char *wire) {
    BIO_ADDR *address = BIO_ADDR_new();
    if (!address) return NULL;
    unsigned short port = (unsigned short)((wire[2] << 8) | wire[3]);
    int made = wire[0] == 4 ? BIO_ADDR_rawmake(address, AF_INET, wire + 8, 4, htons(port))
                            : BIO_ADDR_rawmake(address, AF_INET6, wire + 8, 16, htons(port));
    if (!made) { BIO_ADDR_free(address); return NULL; }
    return address;
}
static dodo_quic *wrap(dodo_quic_shared *shared, SSL *ssl, int kind) {
    dodo_quic *s = OPENSSL_zalloc(sizeof(*s));
    if (!s) return NULL;
    s->shared = shared; s->ssl = ssl; s->kind = kind;
    ++shared->references;
    return s;
}
void dodo_quic_free(void *state) {
    dodo_quic *s = state;
    if (!s) return;
    SSL_free(s->ssl);
    release(s->shared);
    OPENSSL_free(s);
}
/* Create, bind or connect a nonblocking UDP socket owned by a datagram BIO. */
static BIO *open_socket(const BIO_ADDR *address, int server, int *code) {
    int family = BIO_ADDR_family(address);
    int fd = BIO_socket(family, SOCK_DGRAM, IPPROTO_UDP, 0);
    if (fd == -1) { *code = socket_code(); return NULL; }
    int opened = BIO_socket_nbio(fd, 1) && (server
        ? BIO_bind(fd, address, family == AF_INET6 ? BIO_SOCK_V6_ONLY : 0)
        : BIO_connect(fd, address, 0));
    if (!opened) {
        *code = socket_code();
        BIO_closesocket(fd);
        return NULL;
    }
    BIO *bio = BIO_new_dgram(fd, BIO_CLOSE);
    if (!bio) { *code = native_code(); BIO_closesocket(fd); return NULL; }
    return bio;
}
void *dodo_quic_new(int server, const unsigned char *wire, const unsigned char *hostname, size_t hlen,
                    const unsigned char *roots, size_t rlen,
                    const unsigned char *certificate, size_t clen,
                    const unsigned char *key, size_t klen,
                    const unsigned char *alpn, size_t alen,
                    int system_roots, int client_auth, int64_t verify_time,
                    uint64_t idle_timeout, int *error, int *code) {
    *error = DODO_QUIC_INVALID; *code = 0;
    if ((!server && (!hlen || hlen > 253)) || hlen > 253 ||
        (hlen && memchr(hostname, 0, hlen)) || rlen > DODO_QUIC_PEM_LIMIT || clen > DODO_QUIC_PEM_LIMIT ||
        klen > DODO_QUIC_PEM_LIMIT || !valid_alpn(alpn, alen) || (!!clen != !!klen) ||
        (server && !clen) || verify_time < -1 || (server && idle_timeout) ||
        idle_timeout > ((uint64_t)1 << 62) - 1 ||
        (wire[0] != 4 && wire[0] != 6) || wire[4] || wire[5] || wire[6] || wire[7]) return NULL;
    ERR_clear_error();
    dodo_quic *s = NULL;
    BIO_ADDR *address = NULL;
    BIO *bio = NULL;
    dodo_quic_shared *shared = OPENSSL_zalloc(sizeof(*shared));
    if (!shared) { *error = DODO_QUIC_ALLOC; return NULL; }
    shared->references = 1;
    shared->ctx = SSL_CTX_new(server ? OSSL_QUIC_server_method() : OSSL_QUIC_client_method());
    if (!shared->ctx) goto backend_error;
    SSL_CTX_set_max_cert_list(shared->ctx, DODO_QUIC_CHAIN_LIMIT);
    SSL_CTX_set_verify_depth(shared->ctx, 16);
    SSL_CTX_set_session_cache_mode(shared->ctx, SSL_SESS_CACHE_OFF);
    SSL_CTX_set_num_tickets(shared->ctx, 0);
    SSL_CTX_set_verify(shared->ctx, (!server || client_auth) ?
        (SSL_VERIFY_PEER | (server ? SSL_VERIFY_FAIL_IF_NO_PEER_CERT : 0)) : SSL_VERIFY_NONE, NULL);
    if (verify_time >= 0) X509_VERIFY_PARAM_set_time(SSL_CTX_get0_param(shared->ctx), (time_t)verify_time);
    if (system_roots && SSL_CTX_set_default_verify_paths(shared->ctx) != 1) goto backend_error;
    if (rlen && !load_roots(shared->ctx, roots, rlen)) goto backend_error;
    if (clen && !load_identity(shared->ctx, certificate, clen, key, klen)) goto backend_error;
    shared->alpn = OPENSSL_memdup(alpn, alen);
    if (!shared->alpn) { *error = DODO_QUIC_ALLOC; goto fail; }
    shared->alpn_len = (unsigned int)alen;
    if (server) SSL_CTX_set_alpn_select_cb(shared->ctx, alpn_select, shared);
    else if (SSL_CTX_set_alpn_protos(shared->ctx, alpn, (unsigned int)alen)) goto backend_error;
    address = decode_address(wire);
    if (!address) goto backend_error;
    SSL *ssl = server ? SSL_new_listener(shared->ctx, 0) : SSL_new(shared->ctx);
    if (!ssl) goto backend_error;
    s = wrap(shared, ssl, server ? DODO_QUIC_LISTENER : DODO_QUIC_CONNECTION);
    if (!s) { SSL_free(ssl); *error = DODO_QUIC_ALLOC; goto fail; }
    if (!SSL_set_blocking_mode(ssl, 0)) goto backend_error;
    if (!server) {
        char host[254];
        memcpy(host, hostname, hlen); host[hlen] = 0;
        SSL_set_hostflags(ssl, X509_CHECK_FLAG_NO_PARTIAL_WILDCARDS);
        /* IP literals require iPAddress SAN matching, never DNS wildcard matching. */
        ASN1_OCTET_STRING *ip = a2i_IPADDRESS(host);
        int ip_length = ip != NULL;
        ASN1_OCTET_STRING_free(ip);
        ERR_clear_error();
        if (ip_length) {
            if (!X509_VERIFY_PARAM_set1_ip_asc(SSL_get0_param(ssl), host)) goto backend_error;
        } else {
            if (!SSL_set1_host(ssl, host) || !SSL_set_tlsext_host_name(ssl, host)) goto backend_error;
        }
        /* Every stream is explicit: no implicit default stream on the connection. */
        if (!SSL_set_default_stream_mode(ssl, SSL_DEFAULT_STREAM_MODE_NONE)) goto backend_error;
        if (!SSL_set_incoming_stream_policy(ssl, SSL_INCOMING_STREAM_POLICY_ACCEPT, 0)) goto backend_error;
        if (idle_timeout && !SSL_set_feature_request_uint(ssl, SSL_VALUE_QUIC_IDLE_TIMEOUT, idle_timeout))
            goto backend_error;
        if (!SSL_set1_initial_peer_addr(ssl, address)) goto backend_error;
    }
    bio = open_socket(address, server, code);
    if (!bio) { *error = DODO_QUIC_NETWORK; goto fail; }
    SSL_set_bio(ssl, bio, bio);
    if (server && !SSL_listen(ssl)) goto backend_error;
    BIO_ADDR_free(address);
    release(shared);
    *error = 0;
    return s;
backend_error:
    *error = DODO_QUIC_BACKEND;
    *code = native_code();
fail:
    BIO_ADDR_free(address);
    dodo_quic_free(s);
    release(shared);
    return NULL;
}
static int usable(dodo_quic *s, int kind) {
    if (!s) return DODO_QUIC_DEAD;
    if (s->kind != kind) return DODO_QUIC_INVALID;
    return s->failed;
}
int dodo_quic_handshake(void *state, int *code) {
    dodo_quic *s = state; *code = 0;
    int status = usable(s, DODO_QUIC_CONNECTION);
    if (status) return status;
    ERR_clear_error();
    int result = SSL_do_handshake(s->ssl);
    if (result != 1) return ssl_result(s, result, DODO_QUIC_ANY, code, NULL);
    /* A server can still reject a TLS 1.3 client after the client finished. */
    SSL_CONN_CLOSE_INFO info;
    return closed(s, &info) ? DODO_QUIC_CONN_CLOSED : DODO_QUIC_READY;
}
/* Returns a new object, or NULL with *status PENDING when nothing is ready. */
void *dodo_quic_accept_connection(void *state, int *status, int *code) {
    dodo_quic *s = state; *code = 0;
    *status = usable(s, DODO_QUIC_LISTENER);
    if (*status) return NULL;
    ERR_clear_error();
    SSL *connection = SSL_accept_connection(s->ssl, SSL_ACCEPT_CONNECTION_NO_BLOCK);
    if (!connection) {
        if (ERR_peek_error()) { *status = reject(s, DODO_QUIC_BACKEND, code); return NULL; }
        *status = DODO_QUIC_PENDING;
        return NULL;
    }
    if (!SSL_set_blocking_mode(connection, 0) ||
        !SSL_set_incoming_stream_policy(connection, SSL_INCOMING_STREAM_POLICY_ACCEPT, 0)) {
        *code = native_code();
        SSL_free(connection);
        *status = DODO_QUIC_BACKEND;
        return NULL;
    }
    dodo_quic *accepted = wrap(s->shared, connection, DODO_QUIC_CONNECTION);
    if (!accepted) { SSL_free(connection); *status = DODO_QUIC_ALLOC; return NULL; }
    *status = DODO_QUIC_READY;
    return accepted;
}
static void *stream_object(dodo_quic *s, SSL *stream, int *status, int *code) {
    if (!SSL_set_blocking_mode(stream, 0)) {
        *code = native_code();
        SSL_free(stream);
        *status = DODO_QUIC_BACKEND;
        return NULL;
    }
    /* Partial acceptance; OpenSSL copies accepted bytes into its stream buffer. */
    SSL_set_mode(stream, SSL_MODE_ENABLE_PARTIAL_WRITE | SSL_MODE_ACCEPT_MOVING_WRITE_BUFFER);
    dodo_quic *wrapped = wrap(s->shared, stream, DODO_QUIC_STREAM);
    if (!wrapped) { SSL_free(stream); *status = DODO_QUIC_ALLOC; return NULL; }
    *status = DODO_QUIC_READY;
    return wrapped;
}
/* PENDING means the peer's stream limit is exhausted, or the handshake has
 * not finished; retry after event processing. */
void *dodo_quic_open_stream(void *state, int unidirectional, int *status, int *code) {
    dodo_quic *s = state; *code = 0;
    *status = usable(s, DODO_QUIC_CONNECTION);
    if (*status) return NULL;
    if (!SSL_is_init_finished(s->ssl)) { *status = DODO_QUIC_PENDING; return NULL; }
    uint64_t available = 0;
    if (!SSL_get_generic_value_uint(s->ssl, unidirectional ? SSL_VALUE_QUIC_STREAM_UNI_LOCAL_AVAIL
                                    : SSL_VALUE_QUIC_STREAM_BIDI_LOCAL_AVAIL, &available)) {
        *status = ssl_result(s, 0, DODO_QUIC_ANY, code, NULL);
        if (*status == DODO_QUIC_PENDING) *status = reject(s, DODO_QUIC_BACKEND, code);
        return NULL;
    }
    if (!available) { *status = DODO_QUIC_PENDING; return NULL; }
    ERR_clear_error();
    SSL *stream = SSL_new_stream(s->ssl, SSL_STREAM_FLAG_NO_BLOCK | (unidirectional ? SSL_STREAM_FLAG_UNI : 0));
    if (!stream) {
        *status = ssl_result(s, 0, DODO_QUIC_ANY, code, NULL);
        return NULL;
    }
    return stream_object(s, stream, status, code);
}
void *dodo_quic_accept_stream(void *state, int *status, int *code) {
    dodo_quic *s = state; *code = 0;
    *status = usable(s, DODO_QUIC_CONNECTION);
    if (*status) return NULL;
    ERR_clear_error();
    SSL *stream = SSL_accept_stream(s->ssl, SSL_ACCEPT_STREAM_NO_BLOCK);
    if (!stream) {
        SSL_CONN_CLOSE_INFO info;
        if (closed(s, &info)) {
            *status = DODO_QUIC_CONN_CLOSED;
            return NULL;
        }
        if (ERR_peek_error()) { *status = reject(s, DODO_QUIC_BACKEND, code); return NULL; }
        *status = DODO_QUIC_PENDING;
        return NULL;
    }
    return stream_object(s, stream, status, code);
}
int dodo_quic_read(void *state, unsigned char *bytes, size_t count, size_t *actual, int *code, uint64_t *application) {
    dodo_quic *s = state; *actual = 0; *code = 0; *application = 0;
    int status = usable(s, DODO_QUIC_STREAM);
    if (status) return status;
    if (!(SSL_get_stream_type(s->ssl) & SSL_STREAM_TYPE_READ)) return DODO_QUIC_INVALID;
    if (!count) return DODO_QUIC_READY;
    ERR_clear_error();
    int result = SSL_read_ex(s->ssl, bytes, count, actual);
    return result == 1 ? DODO_QUIC_READY : ssl_result(s, result, DODO_QUIC_READING, code, application);
}
int dodo_quic_write(void *state, const unsigned char *bytes, size_t count, size_t *actual, int *code, uint64_t *application) {
    dodo_quic *s = state; *actual = 0; *code = 0; *application = 0;
    int status = usable(s, DODO_QUIC_STREAM);
    if (status) return status;
    if (!(SSL_get_stream_type(s->ssl) & SSL_STREAM_TYPE_WRITE)) return DODO_QUIC_INVALID;
    if (s->concluded) return DODO_QUIC_DEAD;
    if (!count) return DODO_QUIC_READY;
    ERR_clear_error();
    int result = SSL_write_ex(s->ssl, bytes, count, actual);
    return result == 1 ? DODO_QUIC_READY : ssl_result(s, result, DODO_QUIC_WRITING, code, application);
}
/* Send FIN after every accepted byte. Idempotent; later writes fail. */
int dodo_quic_conclude(void *state, int *code) {
    dodo_quic *s = state; *code = 0;
    int status = usable(s, DODO_QUIC_STREAM);
    if (status) return status;
    if (!(SSL_get_stream_type(s->ssl) & SSL_STREAM_TYPE_WRITE)) return DODO_QUIC_INVALID;
    /* OpenSSL rejects a repeated conclusion and reports FINISHED only once the
     * FIN is queued, so the wrapper remembers its own conclusion. */
    if (s->concluded) return DODO_QUIC_READY;
    ERR_clear_error();
    uint64_t ignored = 0;
    if (SSL_stream_conclude(s->ssl, 0) != 1) return ssl_result(s, 0, DODO_QUIC_WRITING, code, &ignored);
    s->concluded = 1;
    return DODO_QUIC_READY;
}
int dodo_quic_reset(void *state, uint64_t application, int *code) {
    dodo_quic *s = state; *code = 0;
    int status = usable(s, DODO_QUIC_STREAM);
    if (status) return status;
    if (application > ((uint64_t)1 << 62) - 1) return DODO_QUIC_INVALID;
    if (!(SSL_get_stream_type(s->ssl) & SSL_STREAM_TYPE_WRITE)) return DODO_QUIC_INVALID;
    SSL_STREAM_RESET_ARGS args = { application };
    ERR_clear_error();
    uint64_t ignored = 0;
    return SSL_stream_reset(s->ssl, &args, sizeof(args)) == 1 ? DODO_QUIC_READY : ssl_result(s, 0, DODO_QUIC_WRITING, code, &ignored);
}
/* id, then flags: bit 0 readable, bit 1 writable, bit 2 locally initiated. */
int dodo_quic_stream_info(void *state, uint64_t *id, uint32_t *flags) {
    dodo_quic *s = state; *id = 0; *flags = 0;
    if (!s) return DODO_QUIC_DEAD;
    if (s->kind != DODO_QUIC_STREAM) return DODO_QUIC_INVALID;
    *id = SSL_get_stream_id(s->ssl);
    int type = SSL_get_stream_type(s->ssl);
    *flags = (type & SSL_STREAM_TYPE_READ ? 1u : 0u) | (type & SSL_STREAM_TYPE_WRITE ? 2u : 0u)
        | (SSL_is_stream_local(s->ssl) ? 4u : 0u);
    return DODO_QUIC_READY;
}
/* Graceful CONNECTION_CLOSE with an application error code after flushing
 * concluded streams. FINISHED means the connection has terminated. */
int dodo_quic_shutdown(void *state, uint64_t application, int *code) {
    dodo_quic *s = state; *code = 0;
    if (!s) return DODO_QUIC_DEAD;
    if (s->kind != DODO_QUIC_CONNECTION || application > ((uint64_t)1 << 62) - 1) return DODO_QUIC_INVALID;
    SSL_SHUTDOWN_EX_ARGS args = { application, NULL };
    ERR_clear_error();
    int result = SSL_shutdown_ex(s->ssl, SSL_SHUTDOWN_FLAG_NO_BLOCK, &args, sizeof(args));
    if (result == 1) return DODO_QUIC_FINISHED;
    if (result == 0) return DODO_QUIC_PENDING;
    return reject(s, DODO_QUIC_BACKEND, code);
}
/* 1 when closed, with flags bit 0 local, bit 1 transport-level error. */
int dodo_quic_close_info(void *state, uint64_t *application, uint32_t *flags) {
    dodo_quic *s = state; *application = 0; *flags = 0;
    if (!s) return DODO_QUIC_DEAD;
    if (s->kind != DODO_QUIC_CONNECTION) return DODO_QUIC_INVALID;
    SSL_CONN_CLOSE_INFO info;
    if (!closed(s, &info)) return 0;
    *application = info.error_code;
    *flags = (info.flags & SSL_CONN_CLOSE_FLAG_LOCAL ? 1u : 0u)
        | (info.flags & SSL_CONN_CLOSE_FLAG_TRANSPORT ? 2u : 0u);
    return 1;
}
int dodo_quic_events(void *state, int *code) {
    dodo_quic *s = state; *code = 0;
    if (!s) return DODO_QUIC_DEAD;
    ERR_clear_error();
    if (SSL_handle_events(s->ssl) != 1) return reject(NULL, DODO_QUIC_BACKEND, code);
    return DODO_QUIC_READY;
}
/* Milliseconds until OpenSSL's next timer, rounded up; -1 when none. */
int64_t dodo_quic_timeout(void *state) {
    dodo_quic *s = state;
    if (!s) return -1;
    struct timeval tv;
    int infinite = 1;
    if (!SSL_get_event_timeout(s->ssl, &tv, &infinite) || infinite) return -1;
    return (int64_t)tv.tv_sec * 1000 + ((int64_t)tv.tv_usec + 999) / 1000;
}
/* Wait up to the caller's bound or OpenSSL's next timer for socket readiness,
 * then process events. The socket is shared by a listener's connections. */
int dodo_quic_wait(void *state, uint32_t milliseconds, int *code) {
    dodo_quic *s = state; *code = 0;
    if (!s) return DODO_QUIC_DEAD;
    if (milliseconds > DODO_QUIC_WAIT_LIMIT) milliseconds = DODO_QUIC_WAIT_LIMIT;
    int64_t timer = dodo_quic_timeout(s);
    int bound = (int)milliseconds;
    if (timer >= 0 && timer < bound) bound = (int)timer;
    BIO_POLL_DESCRIPTOR descriptor;
    if (bound > 0 && SSL_get_rpoll_descriptor(s->ssl, &descriptor) &&
        descriptor.type == BIO_POLL_DESCRIPTOR_TYPE_SOCK_FD) {
#ifdef _WIN32
        WSAPOLLFD entry;
        entry.fd = (SOCKET)descriptor.value.fd;
#else
        struct pollfd entry;
        entry.fd = descriptor.value.fd;
#endif
        entry.events = POLLIN | (SSL_net_write_desired(s->ssl) ? POLLOUT : 0);
        entry.revents = 0;
        (void)dodo_quic_poll(&entry, 1, bound);
    }
    return dodo_quic_events(s, code);
}
int dodo_quic_alpn(void *state, unsigned char *bytes, size_t capacity) {
    dodo_quic *s = state;
    if (!s) return DODO_QUIC_DEAD;
    if (s->kind != DODO_QUIC_CONNECTION) return DODO_QUIC_INVALID;
    const unsigned char *selected; unsigned int length;
    SSL_get0_alpn_selected(s->ssl, &selected, &length);
    if (length > capacity) return DODO_QUIC_LIMIT;
    if (length) memcpy(bytes, selected, length);
    return (int)length;
}
/* The bound local address of a listener or client connection's socket. */
int dodo_quic_local_address(void *state, unsigned char *wire, int *code) {
    dodo_quic *s = state; *code = 0;
    memset(wire, 0, 24);
    if (!s) return DODO_QUIC_DEAD;
    if (s->kind == DODO_QUIC_STREAM) return DODO_QUIC_INVALID;
    int fd = SSL_get_fd(s->ssl);
    if (fd < 0) return DODO_QUIC_INVALID;
    BIO_ADDR *address = BIO_ADDR_new();
    if (!address) return DODO_QUIC_ALLOC;
    union BIO_sock_info_u info;
    info.addr = address;
    if (!BIO_sock_info(fd, BIO_SOCK_INFO_ADDRESS, &info)) {
        BIO_ADDR_free(address);
        *code = native_code();
        return DODO_QUIC_NETWORK;
    }
    int family = BIO_ADDR_family(address);
    size_t length = 0;
    unsigned short port = ntohs(BIO_ADDR_rawport(address));
    if ((family != AF_INET && family != AF_INET6) || !BIO_ADDR_rawaddress(address, NULL, &length) ||
        length > 16 || !BIO_ADDR_rawaddress(address, wire + 8, &length)) {
        BIO_ADDR_free(address);
        return DODO_QUIC_BACKEND;
    }
    wire[0] = family == AF_INET ? 4 : 6;
    wire[2] = (unsigned char)(port >> 8);
    wire[3] = (unsigned char)port;
    BIO_ADDR_free(address);
    return DODO_QUIC_READY;
}
