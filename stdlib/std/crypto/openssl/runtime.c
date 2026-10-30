/* OpenSSL 3.5+ libcrypto backend. Apache-2.0 dependency, dynamically linked by
 * default. Each handle belongs to exactly one Dodo value, and no call retains
 * a caller buffer after it returns. */
#include <openssl/core_names.h>
#include <openssl/crypto.h>
#include <openssl/err.h>
#include <openssl/evp.h>
#include <openssl/rsa.h>
#include <openssl/x509.h>
#include <limits.h>
#include <stdint.h>
#include <string.h>
#if OPENSSL_VERSION_NUMBER < 0x30500000L
#error "Dodo crypto requires a maintained OpenSSL 3.5 or later backend"
#endif

enum { DODO_CRYPTO_OK=0, DODO_CRYPTO_INVALID=-1, DODO_CRYPTO_SHORT=-2, DODO_CRYPTO_ALLOC=-3, DODO_CRYPTO_BACKEND=-9 };
enum { DODO_CRYPTO_SHA1=1, DODO_CRYPTO_SHA256=2, DODO_CRYPTO_AES128_CFB8=1 };
enum { DODO_RSA_MIN_BITS=1024, DODO_RSA_MAX_BITS=16384, DODO_RSA_MAX_BYTES=DODO_RSA_MAX_BITS/8 };
/* EVP lengths are int; feed larger slices in pieces that stay far below it. */
enum { DODO_CRYPTO_CHUNK=1<<30 };

static const unsigned char empty[1];

/* The newest queued OpenSSL reason, for diagnostics only. */
int32_t dodo_crypto_error(void) {
    return (int32_t)(ERR_peek_last_error() & INT_MAX);
}
static const EVP_MD *digest_of(int algorithm) {
    if (algorithm == DODO_CRYPTO_SHA1) return EVP_sha1();
    if (algorithm == DODO_CRYPTO_SHA256) return EVP_sha256();
    return NULL;
}

void dodo_crypto_digest_free(void *state) {
    EVP_MD_CTX_free(state);
}
void *dodo_crypto_digest_new(int32_t algorithm, int32_t *status) {
    const EVP_MD *md = digest_of(algorithm);
    *status = DODO_CRYPTO_INVALID;
    if (!md) return NULL;
    ERR_clear_error();
    EVP_MD_CTX *ctx = EVP_MD_CTX_new();
    if (!ctx) { *status = DODO_CRYPTO_ALLOC; return NULL; }
    if (EVP_DigestInit_ex2(ctx, md, NULL) != 1) {
        EVP_MD_CTX_free(ctx);
        *status = DODO_CRYPTO_BACKEND;
        return NULL;
    }
    *status = DODO_CRYPTO_OK;
    return ctx;
}
int32_t dodo_crypto_digest_update(void *state, const unsigned char *bytes, size_t count) {
    if (!state) return DODO_CRYPTO_INVALID;
    if (!count) return DODO_CRYPTO_OK;
    ERR_clear_error();
    return EVP_DigestUpdate(state, bytes, count) == 1 ? DODO_CRYPTO_OK : DODO_CRYPTO_BACKEND;
}
/* Writes exactly the digest size and restarts the same algorithm. */
int32_t dodo_crypto_digest_finish(void *state, unsigned char *output, size_t capacity) {
    EVP_MD_CTX *ctx = state;
    if (!ctx) return DODO_CRYPTO_INVALID;
    const EVP_MD *md = EVP_MD_CTX_get0_md(ctx);
    if (!md || EVP_MD_get_size(md) <= 0 || (size_t)EVP_MD_get_size(md) != capacity) return DODO_CRYPTO_INVALID;
    ERR_clear_error();
    unsigned int written = 0;
    if (EVP_DigestFinal_ex(ctx, output, &written) != 1 || written != capacity) return DODO_CRYPTO_BACKEND;
    return EVP_DigestInit_ex2(ctx, md, NULL) == 1 ? DODO_CRYPTO_OK : DODO_CRYPTO_BACKEND;
}

void dodo_crypto_mac_free(void *state) {
    EVP_MAC_CTX_free(state);
}
void *dodo_crypto_hmac_new(int32_t algorithm, const unsigned char *key, size_t key_len, int32_t *status) {
    const char *name = algorithm == DODO_CRYPTO_SHA1 ? OSSL_DIGEST_NAME_SHA1 :
                       algorithm == DODO_CRYPTO_SHA256 ? OSSL_DIGEST_NAME_SHA2_256 : NULL;
    *status = DODO_CRYPTO_INVALID;
    if (!name || key_len > INT_MAX) return NULL;
    ERR_clear_error();
    EVP_MAC *mac = EVP_MAC_fetch(NULL, OSSL_MAC_NAME_HMAC, NULL);
    if (!mac) { *status = DODO_CRYPTO_BACKEND; return NULL; }
    EVP_MAC_CTX *ctx = EVP_MAC_CTX_new(mac);
    EVP_MAC_free(mac);
    if (!ctx) { *status = DODO_CRYPTO_ALLOC; return NULL; }
    OSSL_PARAM params[] = {
        OSSL_PARAM_construct_utf8_string(OSSL_MAC_PARAM_DIGEST, (char *)name, 0),
        OSSL_PARAM_construct_end()
    };
    /* RFC 2104 permits an empty key; pass a valid pointer for it. */
    if (EVP_MAC_init(ctx, key_len ? key : empty, key_len, params) != 1) {
        EVP_MAC_CTX_free(ctx);
        *status = DODO_CRYPTO_BACKEND;
        return NULL;
    }
    *status = DODO_CRYPTO_OK;
    return ctx;
}
int32_t dodo_crypto_mac_update(void *state, const unsigned char *bytes, size_t count) {
    if (!state) return DODO_CRYPTO_INVALID;
    if (!count) return DODO_CRYPTO_OK;
    ERR_clear_error();
    return EVP_MAC_update(state, bytes, count) == 1 ? DODO_CRYPTO_OK : DODO_CRYPTO_BACKEND;
}
/* Writes exactly the MAC size and restarts with the same key. */
int32_t dodo_crypto_mac_finish(void *state, unsigned char *output, size_t capacity) {
    EVP_MAC_CTX *ctx = state;
    if (!ctx || EVP_MAC_CTX_get_mac_size(ctx) != capacity) return DODO_CRYPTO_INVALID;
    ERR_clear_error();
    size_t written = 0;
    if (EVP_MAC_final(ctx, output, &written, capacity) != 1 || written != capacity) return DODO_CRYPTO_BACKEND;
    return EVP_MAC_init(ctx, NULL, 0, NULL) == 1 ? DODO_CRYPTO_OK : DODO_CRYPTO_BACKEND;
}

void dodo_crypto_cipher_free(void *state) {
    EVP_CIPHER_CTX_free(state);
}
void *dodo_crypto_cipher_new(int32_t algorithm, int32_t encrypt,
                             const unsigned char *key, size_t key_len,
                             const unsigned char *iv, size_t iv_len, int32_t *status) {
    const EVP_CIPHER *cipher = algorithm == DODO_CRYPTO_AES128_CFB8 ? EVP_aes_128_cfb8() : NULL;
    *status = DODO_CRYPTO_INVALID;
    if (!cipher || key_len != (size_t)EVP_CIPHER_get_key_length(cipher) ||
        iv_len != (size_t)EVP_CIPHER_get_iv_length(cipher)) return NULL;
    ERR_clear_error();
    EVP_CIPHER_CTX *ctx = EVP_CIPHER_CTX_new();
    if (!ctx) { *status = DODO_CRYPTO_ALLOC; return NULL; }
    if (EVP_CipherInit_ex2(ctx, cipher, key, iv, encrypt ? 1 : 0, NULL) != 1) {
        EVP_CIPHER_CTX_free(ctx);
        *status = DODO_CRYPTO_BACKEND;
        return NULL;
    }
    *status = DODO_CRYPTO_OK;
    return ctx;
}
/* Stream modes emit one output byte per input byte. input may equal output
 * for in-place use; OpenSSL rejects only partial overlap. */
int32_t dodo_crypto_cipher_update(void *state, const unsigned char *input, unsigned char *output, size_t count) {
    if (!state) return DODO_CRYPTO_INVALID;
    ERR_clear_error();
    while (count) {
        int chunk = count > DODO_CRYPTO_CHUNK ? DODO_CRYPTO_CHUNK : (int)count;
        int written = 0;
        if (EVP_CipherUpdate(state, output, &written, input, chunk) != 1 || written != chunk)
            return DODO_CRYPTO_BACKEND;
        input += chunk; output += chunk; count -= (size_t)chunk;
    }
    return DODO_CRYPTO_OK;
}

void dodo_crypto_key_free(void *key) {
    EVP_PKEY_free(key);
}
static int rsa_bits_supported(const EVP_PKEY *key) {
    int bits = EVP_PKEY_get_bits(key);
    return EVP_PKEY_is_a(key, "RSA") && bits >= DODO_RSA_MIN_BITS && bits <= DODO_RSA_MAX_BITS;
}
/* Public exponent 65537 from OpenSSL's default provider and CSPRNG. */
void *dodo_crypto_rsa_generate(uint32_t bits, int32_t *status) {
    *status = DODO_CRYPTO_INVALID;
    if (bits < DODO_RSA_MIN_BITS || bits > DODO_RSA_MAX_BITS) return NULL;
    ERR_clear_error();
    EVP_PKEY *key = EVP_RSA_gen(bits);
    if (!key) { *status = DODO_CRYPTO_BACKEND; return NULL; }
    *status = DODO_CRYPTO_OK;
    return key;
}
/* Accepts exactly one DER SubjectPublicKeyInfo holding an RSA key. */
void *dodo_crypto_rsa_public_from_der(const unsigned char *der, size_t length, int32_t *status) {
    *status = DODO_CRYPTO_INVALID;
    if (!length || length > LONG_MAX) return NULL;
    ERR_clear_error();
    const unsigned char *cursor = der;
    EVP_PKEY *key = d2i_PUBKEY(NULL, &cursor, (long)length);
    if (!key) return NULL;
    if (cursor != der + length || !rsa_bits_supported(key)) {
        EVP_PKEY_free(key);
        return NULL;
    }
    *status = DODO_CRYPTO_OK;
    return key;
}
int32_t dodo_crypto_key_bits(void *key) {
    return key ? EVP_PKEY_get_bits(key) : DODO_CRYPTO_INVALID;
}
/* Modulus bytes: the ciphertext length for encryption and decryption. */
int32_t dodo_crypto_key_size(void *key) {
    return key ? EVP_PKEY_get_size(key) : DODO_CRYPTO_INVALID;
}
/* *actual always receives the full DER length, so a short destination can be
 * resized and retried; nothing is written to it in that case. */
int32_t dodo_crypto_key_public_der(void *key, unsigned char *output, size_t capacity, size_t *actual) {
    *actual = 0;
    if (!key) return DODO_CRYPTO_INVALID;
    ERR_clear_error();
    int length = i2d_PUBKEY(key, NULL);
    if (length <= 0) return DODO_CRYPTO_BACKEND;
    *actual = (size_t)length;
    if ((size_t)length > capacity) return DODO_CRYPTO_SHORT;
    unsigned char *cursor = output;
    return i2d_PUBKEY(key, &cursor) == length ? DODO_CRYPTO_OK : DODO_CRYPTO_BACKEND;
}
static EVP_PKEY_CTX *pkcs1(EVP_PKEY *key, int decrypt) {
    EVP_PKEY_CTX *ctx = EVP_PKEY_CTX_new_from_pkey(NULL, key, NULL);
    if (!ctx) return NULL;
    if ((decrypt ? EVP_PKEY_decrypt_init(ctx) : EVP_PKEY_encrypt_init(ctx)) != 1 ||
        EVP_PKEY_CTX_set_rsa_padding(ctx, RSA_PKCS1_PADDING) != 1) {
        EVP_PKEY_CTX_free(ctx);
        return NULL;
    }
    return ctx;
}
/* RSAES-PKCS1-v1_5 with OpenSSL's implicit rejection: a malformed ciphertext
 * of the right length yields a deterministic pseudorandom message instead of
 * an error, so callers must validate the plaintext they expect. */
int32_t dodo_crypto_rsa_decrypt(void *key, const unsigned char *input, size_t input_len,
                                unsigned char *output, size_t capacity, size_t *actual) {
    *actual = 0;
    if (!key || EVP_PKEY_get_size(key) <= 0 || input_len != (size_t)EVP_PKEY_get_size(key))
        return DODO_CRYPTO_INVALID;
    ERR_clear_error();
    EVP_PKEY_CTX *ctx = pkcs1(key, 1);
    if (!ctx) return DODO_CRYPTO_BACKEND;
    /* Decrypt into private storage first: the message length is only known
     * afterwards, and a short caller buffer must not receive a prefix. */
    unsigned char plain[DODO_RSA_MAX_BYTES];
    size_t length = sizeof(plain);
    int result = EVP_PKEY_decrypt(ctx, plain, &length, input, input_len);
    EVP_PKEY_CTX_free(ctx);
    int32_t status = DODO_CRYPTO_BACKEND;
    if (result == 1) {
        *actual = length;
        status = length > capacity ? DODO_CRYPTO_SHORT : DODO_CRYPTO_OK;
        if (status == DODO_CRYPTO_OK && length) memcpy(output, plain, length);
    }
    OPENSSL_cleanse(plain, sizeof(plain));
    return status;
}
/* Messages may be at most the modulus size minus 11 bytes of padding. */
int32_t dodo_crypto_rsa_encrypt(void *key, const unsigned char *input, size_t input_len,
                                unsigned char *output, size_t capacity, size_t *actual) {
    *actual = 0;
    int size = key ? EVP_PKEY_get_size(key) : 0;
    if (size <= 11 || input_len > (size_t)size - 11) return DODO_CRYPTO_INVALID;
    *actual = (size_t)size;
    if (capacity < (size_t)size) return DODO_CRYPTO_SHORT;
    ERR_clear_error();
    EVP_PKEY_CTX *ctx = pkcs1(key, 0);
    if (!ctx) return DODO_CRYPTO_BACKEND;
    size_t length = capacity;
    int result = EVP_PKEY_encrypt(ctx, output, &length, input_len ? input : empty, input_len);
    EVP_PKEY_CTX_free(ctx);
    if (result != 1 || length != (size_t)size) return DODO_CRYPTO_BACKEND;
    return DODO_CRYPTO_OK;
}
