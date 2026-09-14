/* OpenSSL 3.5+ backend. Apache-2.0 dependency, dynamically linked by default.
 * The external BIO and private retry buffer own all bytes between calls.
 * No OS operation or SSL retry ever retains a Dodo caller buffer. */
#include <openssl/ssl.h>
#include <openssl/err.h>
#include <openssl/pem.h>
#include <openssl/x509v3.h>
#include <stdint.h>
#include <string.h>
#include <limits.h>
#if OPENSSL_VERSION_NUMBER < 0x30500000L
#error "Dodo TLS requires a maintained OpenSSL 3.5 or later backend"
#endif

enum { DODO_TLS_READY=0, DODO_TLS_INPUT=1, DODO_TLS_OUTPUT=2, DODO_TLS_CLOSED=3, DODO_TLS_INVALID=-1, DODO_TLS_ALLOC=-2,
       DODO_TLS_VERIFY=-3, DODO_TLS_PROTOCOL=-4, DODO_TLS_TRUNCATED=-5, DODO_TLS_DEAD=-6, DODO_TLS_BACKEND=-9, DODO_TLS_LIMIT=-10 };
enum { DODO_TLS_RECORD=16384, DODO_TLS_WIRE=32768, DODO_TLS_PEM_LIMIT=1048576, DODO_TLS_CHAIN_LIMIT=65536 };
typedef struct {
    SSL_CTX *ctx;
    SSL *ssl;
    BIO *wire;
    unsigned char *alpn;
    unsigned int alpn_len;
    unsigned char pending[DODO_TLS_RECORD];
    size_t pending_len;
    int failed, eof;
} dodo_tls;

static int reject(dodo_tls *s, int kind, int *code) {
    unsigned long native = ERR_peek_last_error();
    if (code) *code = (int)(native & INT_MAX);
    if (s) s->failed = kind;
    return kind;
}
static int ssl_result(dodo_tls *s, int result, int *code) {
    int error = SSL_get_error(s->ssl, result);
    if (error == SSL_ERROR_WANT_READ) return DODO_TLS_INPUT;
    if (error == SSL_ERROR_WANT_WRITE) return DODO_TLS_OUTPUT;
    if (error == SSL_ERROR_ZERO_RETURN) return DODO_TLS_CLOSED;
    if (SSL_get_verify_result(s->ssl) != X509_V_OK) {
        *code = (int)SSL_get_verify_result(s->ssl);
        s->failed = DODO_TLS_VERIFY;
        return DODO_TLS_VERIFY;
    }
    if (s->eof) return reject(s, DODO_TLS_TRUNCATED, code);
    return reject(s, DODO_TLS_PROTOCOL, code);
}
static int alpn_select(SSL *ssl, const unsigned char **out, unsigned char *length,
                       const unsigned char *in, unsigned int inlen, void *arg) {
    (void)ssl;
    dodo_tls *s = arg;
    unsigned char *selected = NULL;
    if (SSL_select_next_proto(&selected, length, s->alpn, s->alpn_len, in, inlen)
        != OPENSSL_NPN_NEGOTIATED) return SSL_TLSEXT_ERR_ALERT_FATAL;
    *out = selected;
    return SSL_TLSEXT_ERR_OK;
}
static int valid_alpn(const unsigned char *data, size_t len) {
    if (len > 65535) return 0;
    size_t i = 0;
    while (i < len) {
        size_t n = data[i++];
        if (!n || n > len-i) return 0;
        i += n;
    }
    return 1;
}
void dodo_tls_free(void *state) {
    dodo_tls *s = state;
    if (!s) return;
    SSL_free(s->ssl);
    BIO_free(s->wire);
    SSL_CTX_free(s->ctx);
    OPENSSL_free(s->alpn);
    OPENSSL_clear_free(s, sizeof(*s));
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
void *dodo_tls_new(int server, const unsigned char *hostname, size_t hlen,
                  const unsigned char *roots, size_t rlen,
                  const unsigned char *certificate, size_t clen,
                  const unsigned char *key, size_t klen,
                  const unsigned char *alpn, size_t alen,
                  int system_roots, int client_auth, int64_t verify_time, int *error) {
    *error = DODO_TLS_INVALID;
    if ((!server && (!hlen || hlen > 253)) || hlen > 253 ||
        (hlen && memchr(hostname, 0, hlen)) || rlen > DODO_TLS_PEM_LIMIT || clen > DODO_TLS_PEM_LIMIT ||
        klen > DODO_TLS_PEM_LIMIT || !valid_alpn(alpn, alen) || (!!clen != !!klen) ||
        (server && !clen) || verify_time < -1) return NULL;
    ERR_clear_error();
    dodo_tls *s = OPENSSL_zalloc(sizeof(*s));
    if (!s) { *error = DODO_TLS_ALLOC; return NULL; }
    s->ctx = SSL_CTX_new(server ? TLS_server_method() : TLS_client_method());
    if (!s->ctx) goto backend_error;
    if (!SSL_CTX_set_min_proto_version(s->ctx, TLS1_2_VERSION)) goto backend_error;
    SSL_CTX_set_options(s->ctx, SSL_OP_NO_COMPRESSION | SSL_OP_NO_RENEGOTIATION);
    SSL_CTX_set_max_cert_list(s->ctx, DODO_TLS_CHAIN_LIMIT);
    SSL_CTX_set_verify_depth(s->ctx, 16);
    SSL_CTX_set_session_cache_mode(s->ctx, SSL_SESS_CACHE_OFF);
    SSL_CTX_set_num_tickets(s->ctx, 0);
    SSL_CTX_set_verify(s->ctx, (!server || client_auth) ?
        (SSL_VERIFY_PEER | (server ? SSL_VERIFY_FAIL_IF_NO_PEER_CERT : 0)) : SSL_VERIFY_NONE, NULL);
    if (system_roots && SSL_CTX_set_default_verify_paths(s->ctx) != 1) goto backend_error;
    if (rlen && !load_roots(s->ctx, roots, rlen)) goto backend_error;
    if (clen && !load_identity(s->ctx, certificate, clen, key, klen)) goto backend_error;
    if (alen) {
        s->alpn = OPENSSL_memdup(alpn, alen);
        if (!s->alpn) { *error = DODO_TLS_ALLOC; goto fail; }
        s->alpn_len = (unsigned int)alen;
        if (server) SSL_CTX_set_alpn_select_cb(s->ctx, alpn_select, s);
        else if (SSL_CTX_set_alpn_protos(s->ctx, alpn, (unsigned int)alen)) goto backend_error;
    }
    s->ssl = SSL_new(s->ctx);
    if (!s->ssl) goto backend_error;
    if (verify_time >= 0) X509_VERIFY_PARAM_set_time(SSL_get0_param(s->ssl), (time_t)verify_time);
    if (!server) {
        char host[254];
        memcpy(host, hostname, hlen); host[hlen] = 0;
        SSL_set_hostflags(s->ssl, X509_CHECK_FLAG_NO_PARTIAL_WILDCARDS);
        /* IP literals require iPAddress SAN matching, never DNS wildcard matching. */
        ASN1_OCTET_STRING *ip = a2i_IPADDRESS(host);
        int ip_length = ip != NULL;
        ASN1_OCTET_STRING_free(ip);
        ERR_clear_error();
        if (ip_length) {
            if (!X509_VERIFY_PARAM_set1_ip_asc(SSL_get0_param(s->ssl), host)) goto backend_error;
        } else {
            if (!SSL_set1_host(s->ssl, host) || !SSL_set_tlsext_host_name(s->ssl, host)) goto backend_error;
        }
    }
    BIO *inside = NULL;
    if (BIO_new_bio_pair(&inside, DODO_TLS_WIRE, &s->wire, DODO_TLS_WIRE) != 1) goto backend_error;
    SSL_set_bio(s->ssl, inside, inside);
    if (server) SSL_set_accept_state(s->ssl); else SSL_set_connect_state(s->ssl);
    *error = 0;
    return s;
backend_error:
    *error = DODO_TLS_BACKEND;
fail:
    dodo_tls_free(s);
    return NULL;
}
int dodo_tls_handshake(void *state, int *error) {
    dodo_tls *s = state; *error = 0;
    if (!s) return DODO_TLS_DEAD;
    if (s->failed) return s->failed;
    ERR_clear_error();
    int result = SSL_do_handshake(s->ssl);
    return result == 1 ? DODO_TLS_READY : ssl_result(s, result, error);
}
int dodo_tls_feed(void *state, const unsigned char *bytes, size_t count, size_t *actual) {
    dodo_tls *s = state; *actual = 0;
    if (!s) return DODO_TLS_DEAD;
    if (s->failed) return s->failed;
    if (s->eof) return DODO_TLS_DEAD;
    if (!count) return DODO_TLS_READY;
    size_t room = BIO_ctrl_get_write_guarantee(s->wire);
    if (count > room) count = room;
    if (!count) return DODO_TLS_OUTPUT;
    int result = BIO_write(s->wire, bytes, (int)count);
    if (result > 0) { *actual = (size_t)result; return DODO_TLS_READY; }
    return BIO_should_retry(s->wire) ? DODO_TLS_OUTPUT : DODO_TLS_BACKEND;
}
int dodo_tls_drain(void *state, unsigned char *bytes, size_t count, size_t *actual) {
    dodo_tls *s = state; *actual = 0;
    if (!s) return DODO_TLS_DEAD;
    if (!count) return DODO_TLS_READY;
    if (count > DODO_TLS_WIRE) count = DODO_TLS_WIRE;
    int result = BIO_read(s->wire, bytes, (int)count);
    if (result > 0) { *actual = (size_t)result; return DODO_TLS_READY; }
    return BIO_should_retry(s->wire) || result == 0 ? DODO_TLS_INPUT : DODO_TLS_BACKEND;
}
int dodo_tls_flush(void *state, int *error) {
    dodo_tls *s = state; *error = 0;
    if (!s) return DODO_TLS_DEAD;
    if (s->failed) return s->failed;
    if (!s->pending_len) return DODO_TLS_READY;
    ERR_clear_error();
    size_t written = 0;
    int result = SSL_write_ex(s->ssl, s->pending, s->pending_len, &written);
    if (result != 1) return ssl_result(s, result, error);
    if (written != s->pending_len) return reject(s, DODO_TLS_PROTOCOL, error);
    OPENSSL_cleanse(s->pending, s->pending_len);
    s->pending_len = 0;
    return DODO_TLS_READY;
}
int dodo_tls_write(void *state, const unsigned char *bytes, size_t count, size_t *actual, int *error) {
    dodo_tls *s = state; *actual = 0; *error = 0;
    if (!s) return DODO_TLS_DEAD;
    if (s->failed) return s->failed;
    if (!count) return DODO_TLS_READY;
    if (!SSL_is_init_finished(s->ssl)) return DODO_TLS_INVALID;
    if (SSL_get_shutdown(s->ssl)) return DODO_TLS_DEAD;
    int status = dodo_tls_flush(s, error);
    if (status != DODO_TLS_READY) return status;
    if (count > DODO_TLS_RECORD) count = DODO_TLS_RECORD;
    memcpy(s->pending, bytes, count); s->pending_len = count;
    status = dodo_tls_flush(s, error);
    if (status < 0) return status;
    *actual = count; /* Accepted bytes now owned even when ciphertext is pending. */
    return status;
}
int dodo_tls_read(void *state, unsigned char *bytes, size_t count, size_t *actual, int *error) {
    dodo_tls *s = state; *actual = 0; *error = 0;
    if (!s) return DODO_TLS_DEAD;
    if (s->failed) return s->failed;
    if (!count) return DODO_TLS_READY;
    if (!SSL_is_init_finished(s->ssl)) return DODO_TLS_INVALID;
    if (s->pending_len) {
        int status = dodo_tls_flush(s, error);
        if (status != DODO_TLS_READY) return status;
    }
    ERR_clear_error();
    int result = SSL_read_ex(s->ssl, bytes, count, actual);
    return result == 1 ? DODO_TLS_READY : ssl_result(s, result, error);
}
int dodo_tls_shutdown(void *state, int *error) {
    dodo_tls *s = state; *error = 0;
    if (!s) return DODO_TLS_DEAD;
    if (s->failed) return s->failed;
    int status = dodo_tls_flush(s, error);
    if (status != DODO_TLS_READY) return status;
    ERR_clear_error();
    int result = SSL_shutdown(s->ssl);
    if (result == 1) return DODO_TLS_CLOSED;
    if (result == 0) return DODO_TLS_INPUT;
    return ssl_result(s, result, error);
}
void dodo_tls_eof(void *state) {
    dodo_tls *s = state;
    if (s && !s->eof) { s->eof = 1; (void)BIO_shutdown_wr(s->wire); }
}
int dodo_tls_alpn(void *state, unsigned char *bytes, size_t capacity) {
    dodo_tls *s = state;
    if (!s) return DODO_TLS_DEAD;
    const unsigned char *selected; unsigned int length;
    SSL_get0_alpn_selected(s->ssl, &selected, &length);
    if (length > capacity) return DODO_TLS_LIMIT;
    if (length) memcpy(bytes, selected, length);
    return (int)length;
}
